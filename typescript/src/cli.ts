#!/usr/bin/env node
import path from "node:path"
import { pathToFileURL } from "node:url"
import { Command } from "commander"
import {
	crawlChapter,
	discoverLastChapterNumber,
	ExistingFilePolicy,
} from "./crawler.js"
import { buildEpub } from "./epub.js"
import {
	createSpinner,
	isPromptCancel,
	promptConfirm,
	promptPath,
	promptSelect,
	promptText,
	showCancel,
	showDone,
	showNote,
	showWelcome,
} from "./ui.js"

type ExistingPolicy =
	(typeof ExistingFilePolicy)[keyof typeof ExistingFilePolicy]
type CrawlMode = "crawl" | "crawl_epub" | "epub_only"

interface CliOptions {
	start?: number
	end?: number
	outputRoot: string
	delay: number
	workers: number
	epub: boolean
	epubOnly: boolean
	chapterDir?: string
	fontPath?: string
	ifExists: ExistingPolicy
	interactive: boolean
	fastSkip: boolean
}

interface RunPlan {
	baseUrl: string
	mode: CrawlMode
	outputRoot: string
	chapterNumbers?: number[]
	delay: number
	workers: number
	epub: boolean
	chapterDir?: string
	fontPath?: string
	ifExists: ExistingPolicy
	fastSkip: boolean
	novelTitle?: string
}

interface DiscoveryResult {
	novelTitle: string
	lastAvailableChapter: number
}

async function inferChapterDirFromBaseUrl(
	baseUrl: string,
	outputRoot: string,
): Promise<string> {
	const {
		extractNovelTitleFromMainPageForCli,
		fetchMainHtmlForCli,
		slugifyForCli,
	} = await import("./internal-cli-helpers.js")
	const mainHtml = await fetchMainHtmlForCli(`${baseUrl.replace(/\/+$/, "")}/`)
	const novelTitle = extractNovelTitleFromMainPageForCli(mainHtml)
	return path.join(outputRoot, slugifyForCli(novelTitle, "book"))
}

async function crawlChaptersSequential(params: {
	chapterNumbers: number[]
	baseUrl: string
	outputRoot: string
	ifExists: ExistingPolicy
	delay: number
	novelTitle?: string
	fastSkip?: boolean
}): Promise<{ outputDir: string | null; failures: Array<[number, string]> }> {
	let outputDir: string | null = null
	let existingPolicy: ExistingPolicy = ExistingFilePolicy.ASK
	const failures: Array<[number, string]> = []

	for (const chapterNumber of params.chapterNumbers) {
		try {
			const result = await crawlChapter({
				baseUrl: params.baseUrl,
				chapterNumber,
				outputRoot: params.outputRoot,
				ifExists: params.ifExists,
				existingPolicy,
				delay: params.delay,
				novelTitle: params.novelTitle,
				fastSkip: params.fastSkip,
			})

			outputDir = result.outputDir
			if (result.status === "skip_all") {
				existingPolicy = ExistingFilePolicy.SKIP_ALL
			}
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error)
			failures.push([chapterNumber, message])
			console.error(`[FAIL] Chapter ${chapterNumber}: ${message}`)
		}
	}

	return { outputDir, failures }
}

async function crawlChaptersParallel(params: {
	chapterNumbers: number[]
	baseUrl: string
	outputRoot: string
	ifExists: ExistingPolicy
	workers: number
	novelTitle?: string
	fastSkip?: boolean
}): Promise<{ outputDir: string | null; failures: Array<[number, string]> }> {
	let outputDir: string | null = null
	const failures: Array<[number, string]> = []
	const queue = [...params.chapterNumbers]

	const worker = async (): Promise<void> => {
		while (queue.length > 0) {
			const chapterNumber = queue.shift()
			if (chapterNumber === undefined) {
				return
			}

			try {
				const result = await crawlChapter({
					baseUrl: params.baseUrl,
					chapterNumber,
					outputRoot: params.outputRoot,
					ifExists: params.ifExists,
					existingPolicy: ExistingFilePolicy.ASK,
					delay: 0,
					novelTitle: params.novelTitle,
					fastSkip: params.fastSkip,
				})

				if (!outputDir) {
					outputDir = result.outputDir
				}
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error)
				failures.push([chapterNumber, message])
				console.error(`[FAIL] Chapter ${chapterNumber}: ${message}`)
			}
		}
	}

	await Promise.all(Array.from({ length: params.workers }, () => worker()))
	failures.sort((a, b) => a[0] - b[0])
	return { outputDir, failures }
}

function parseIntSafe(value: string): number {
	return Number.parseInt(value, 10)
}

function range(start: number, end: number): number[] {
	return Array.from({ length: end - start + 1 }, (_, index) => start + index)
}

async function discoverNovel(baseUrl: string): Promise<DiscoveryResult> {
	const { extractNovelTitleFromMainPageForCli, fetchMainHtmlForCli } =
		await import("./internal-cli-helpers.js")
	const mainHtml = await fetchMainHtmlForCli(`${baseUrl.replace(/\/+$/, "")}/`)
	const [novelTitle, lastAvailableChapter] = await Promise.all([
		Promise.resolve(extractNovelTitleFromMainPageForCli(mainHtml)),
		discoverLastChapterNumber(baseUrl),
	])
	return { novelTitle, lastAvailableChapter }
}

function validateSharedOptions(options: CliOptions): string | null {
	if (!Object.values(ExistingFilePolicy).includes(options.ifExists)) {
		return "Error: --if-exists must be ask, skip, or overwrite."
	}

	if (options.workers <= 0) {
		return "Error: --workers must be a positive integer."
	}

	if (options.workers > 1 && options.ifExists === ExistingFilePolicy.ASK) {
		return "Error: --workers > 1 requires --if-exists skip or --if-exists overwrite."
	}

	return null
}

function validateChapterRange(start: number, end: number): string | null {
	if (start <= 0 || end <= 0) {
		return "Error: chapter numbers must be positive integers."
	}

	if (start > end) {
		return "Error: --start must be less than or equal to --end."
	}

	return null
}

async function resolveChapterNumbers(
	baseUrl: string,
	options: Pick<CliOptions, "start" | "end">,
): Promise<number[]> {
	if (options.start !== undefined && options.end !== undefined) {
		const validationError = validateChapterRange(options.start, options.end)
		if (validationError) {
			throw new Error(validationError.replace(/^Error:\s*/, ""))
		}
		return range(options.start, options.end)
	}

	const lastAvailableChapter = await discoverLastChapterNumber(baseUrl)
	const startChapter = options.start ?? 1
	let endChapter = options.end ?? lastAvailableChapter

	const validationError = validateChapterRange(startChapter, endChapter)
	if (validationError) {
		throw new Error(validationError.replace(/^Error:\s*/, ""))
	}

	if (endChapter > lastAvailableChapter) {
		console.log(
			`[INFO] Requested end chapter ${endChapter} exceeds the last available chapter ${lastAvailableChapter}; stopping at ${lastAvailableChapter}.`,
		)
		endChapter = lastAvailableChapter
	}

	if (startChapter > endChapter) {
		throw new Error(
			`start chapter ${startChapter} is greater than the last available chapter ${endChapter}.`,
		)
	}

	return range(startChapter, endChapter)
}

function buildSummary(plan: RunPlan): string {
	const lines = [
		`Base URL: ${plan.baseUrl}`,
		`Mode: ${
			plan.mode === "crawl"
				? "Crawl chapters"
				: plan.mode === "crawl_epub"
					? "Crawl chapters and build EPUB"
					: "Build EPUB from existing chapters"
		}`,
		`Output root: ${plan.outputRoot}`,
	]

	if (plan.chapterNumbers && plan.chapterNumbers.length > 0) {
		lines.push(
			`Chapters: ${plan.chapterNumbers[0]} -> ${plan.chapterNumbers.at(-1)} (${plan.chapterNumbers.length} total)`,
		)
		lines.push(`Workers: ${plan.workers}`)
		lines.push(`Delay: ${plan.delay}s`)
		lines.push(`If chapter exists: ${plan.ifExists}`)
		if (plan.fastSkip) {
			lines.push("Fast skip enabled: yes")
		}
	}

	if (plan.chapterDir) {
		lines.push(`Chapter directory: ${plan.chapterDir}`)
	}

	if (plan.epub) {
		lines.push(`Build EPUB: yes`)
		lines.push(`Font path: ${plan.fontPath?.trim() || "default packaged font"}`)
	}

	return lines.join("\n")
}

async function promptPositiveInteger(params: {
	message: string
	initialValue: number
	max?: number
}): Promise<number | null> {
	const answer = await promptText({
		message: params.message,
		initialValue: String(params.initialValue),
		validate(value) {
			if (!value) {
				return "Enter a positive integer."
			}
			const parsed = Number.parseInt(value, 10)
			if (!Number.isInteger(parsed) || parsed <= 0) {
				return "Enter a positive integer."
			}
			if (params.max !== undefined && parsed > params.max) {
				return `Value must be less than or equal to ${params.max}.`
			}
			return undefined
		},
	})

	if (await isPromptCancel(answer)) {
		return null
	}

	return Number.parseInt(answer as string, 10)
}

async function promptOptionalPath(params: {
	message: string
	initialValue?: string
	placeholder?: string
}): Promise<string | undefined | null> {
	const answer = await promptText({
		message: params.message,
		initialValue: params.initialValue,
		placeholder: params.placeholder,
	})

	if (await isPromptCancel(answer)) {
		return null
	}

	const trimmed = (answer as string).trim()
	return trimmed ? trimmed : undefined
}

async function promptOptionalFontPath(
	initialValue?: string,
): Promise<string | undefined | null> {
	const fontMode = await promptSelect<"default" | "custom">({
		message: "EPUB font",
		options: [
			{
				label: "Use the packaged default font",
				value: "default",
			},
			{
				label: "Select a custom font file",
				value: "custom",
			},
		],
		initialValue: initialValue ? "custom" : "default",
	})

	if (await isPromptCancel(fontMode)) {
		return null
	}

	if (fontMode === "default") {
		return undefined
	}

	const fontPath = await promptPath({
		message: "Select font file for EPUB",
		initialValue: initialValue,
		root: process.cwd(),
		validate(value) {
			if (!value?.trim()) {
				return "Select a font file."
			}
			return undefined
		},
	})

	if (await isPromptCancel(fontPath)) {
		return null
	}

	return fontPath as string
}

async function buildInteractivePlan(
	initialBaseUrl: string | undefined,
	options: CliOptions,
): Promise<RunPlan | null> {
	await showWelcome()

	const baseUrlAnswer =
		initialBaseUrl ??
		(await promptText({
			message: "Novel base URL",
			placeholder: "https://truyenazz.me/your-novel",
			validate(value) {
				if (!value?.trim()) {
					return "Enter a valid http:// or https:// URL."
				}
				if (!/^https?:\/\//i.test(value.trim())) {
					return "Enter a valid http:// or https:// URL."
				}
				return undefined
			},
		}))

	if (!initialBaseUrl && (await isPromptCancel(baseUrlAnswer))) {
		await showCancel("Interactive crawl cancelled.")
		return null
	}

	const baseUrl = (initialBaseUrl ?? (baseUrlAnswer as string)).trim()

	const action = await promptSelect<CrawlMode>({
		message: "What do you want to do?",
		options: [
			{ label: "Crawl chapters", value: "crawl" },
			{ label: "Crawl chapters and build an EPUB", value: "crawl_epub" },
			{
				label: "Build an EPUB from existing chapter files",
				value: "epub_only",
			},
		],
		initialValue: options.epubOnly
			? "epub_only"
			: options.epub
				? "crawl_epub"
				: "crawl",
	})

	if (await isPromptCancel(action)) {
		await showCancel("Interactive crawl cancelled.")
		return null
	}

	const outputRootAnswer = await promptText({
		message: "Output root directory",
		initialValue: options.outputRoot,
		validate(value) {
			if (!value) {
				return "Enter an output directory."
			}
			return value.trim() ? undefined : "Enter an output directory."
		},
	})

	if (await isPromptCancel(outputRootAnswer)) {
		await showCancel("Interactive crawl cancelled.")
		return null
	}

	const outputRoot = path.resolve((outputRootAnswer as string).trim())
	const spinner = await createSpinner()
	spinner.start("Discovering novel information...")

	let discovery: DiscoveryResult
	try {
		discovery = await discoverNovel(baseUrl)
		spinner.stop("Novel information loaded")
	} catch (error) {
		spinner.stop("Failed to discover novel information")
		const message = error instanceof Error ? error.message : String(error)
		await showCancel(`Error: ${message}`)
		return null
	}

	await showNote(
		[
			`Title: ${discovery.novelTitle}`,
			`Latest chapter: ${discovery.lastAvailableChapter}`,
		].join("\n"),
		"Novel",
	)

	let chapterNumbers: number[] | undefined
	let workers = options.workers
	let delay = options.delay
	let ifExists = options.ifExists
	let chapterDir = options.chapterDir
		? path.resolve(options.chapterDir)
		: undefined

	if (action !== "epub_only") {
		const startChapter =
			(await promptPositiveInteger({
				message: "Start chapter",
				initialValue: options.start ?? 1,
				max: discovery.lastAvailableChapter,
			})) ?? null
		if (startChapter === null) {
			await showCancel("Interactive crawl cancelled.")
			return null
		}

		const endChapter =
			(await promptPositiveInteger({
				message: "End chapter",
				initialValue: Math.min(
					options.end ?? discovery.lastAvailableChapter,
					discovery.lastAvailableChapter,
				),
				max: discovery.lastAvailableChapter,
			})) ?? null
		if (endChapter === null) {
			await showCancel("Interactive crawl cancelled.")
			return null
		}

		const rangeError = validateChapterRange(startChapter, endChapter)
		if (rangeError) {
			await showCancel(rangeError)
			return null
		}

		chapterNumbers = range(startChapter, endChapter)

		const workersAnswer = await promptPositiveInteger({
			message: "Download workers",
			initialValue: options.workers,
		})
		if (workersAnswer === null) {
			await showCancel("Interactive crawl cancelled.")
			return null
		}
		workers = workersAnswer

		const delayAnswer = await promptText({
			message: "Delay between chapter requests (seconds)",
			initialValue: String(options.delay),
			validate(value) {
				if (!value) {
					return "Enter a number greater than or equal to 0."
				}
				const parsed = Number.parseFloat(value)
				if (Number.isNaN(parsed) || parsed < 0) {
					return "Enter a number greater than or equal to 0."
				}
				return undefined
			},
		})
		if (await isPromptCancel(delayAnswer)) {
			await showCancel("Interactive crawl cancelled.")
			return null
		}
		delay = Number.parseFloat(delayAnswer as string)

		const allowedPolicies =
			workers > 1
				? [
						{
							label: "Skip existing chapter files",
							value: ExistingFilePolicy.SKIP,
						},
						{
							label: "Overwrite existing chapter files",
							value: ExistingFilePolicy.OVERWRITE,
						},
					]
				: [
						{
							label: "Ask what to do for each existing chapter",
							value: ExistingFilePolicy.ASK,
						},
						{
							label: "Skip existing chapter files",
							value: ExistingFilePolicy.SKIP,
						},
						{
							label: "Overwrite existing chapter files",
							value: ExistingFilePolicy.OVERWRITE,
						},
					]

		const policyAnswer = await promptSelect<ExistingPolicy>({
			message: "If a chapter file already exists",
			options: allowedPolicies,
			initialValue:
				workers > 1 && ifExists === ExistingFilePolicy.ASK
					? ExistingFilePolicy.SKIP
					: ifExists,
		})
		if (await isPromptCancel(policyAnswer)) {
			await showCancel("Interactive crawl cancelled.")
			return null
		}
		ifExists = policyAnswer as ExistingPolicy
	} else {
		const inferredChapterDir = await inferChapterDirFromBaseUrl(
			baseUrl,
			outputRoot,
		)
		const chapterDirAnswer = await promptOptionalPath({
			message: "Existing chapter directory",
			initialValue: chapterDir ?? inferredChapterDir,
			placeholder: inferredChapterDir,
		})
		if (chapterDirAnswer === null) {
			await showCancel("Interactive crawl cancelled.")
			return null
		}
		chapterDir = path.resolve(chapterDirAnswer ?? inferredChapterDir)
	}

	let fastSkip = options.fastSkip
	if (action !== "epub_only") {
		const fastSkipAnswer = await promptConfirm({
			message:
				"Enable Fast Skip? (Bypasses remote URL checks if the chapter file exists locally)",
			initialValue: options.fastSkip,
		})
		if (await isPromptCancel(fastSkipAnswer)) {
			await showCancel("Interactive crawl cancelled.")
			return null
		}
		fastSkip = fastSkipAnswer as boolean
	}

	let fontPath = options.fontPath
	if (action !== "crawl" || options.fontPath) {
		const fontAnswer = await promptOptionalFontPath(options.fontPath)
		if (fontAnswer === null) {
			await showCancel("Interactive crawl cancelled.")
			return null
		}
		fontPath = fontAnswer
	}

	const plan: RunPlan = {
		baseUrl,
		mode: action as CrawlMode,
		outputRoot,
		chapterNumbers,
		delay,
		workers,
		epub: action !== "crawl",
		chapterDir,
		fontPath,
		ifExists,
		fastSkip,
		novelTitle: discovery.novelTitle,
	}

	await showNote(buildSummary(plan), "Plan")

	const confirmed = await promptConfirm({
		message: "Run this job now?",
		initialValue: true,
	})
	if ((await isPromptCancel(confirmed)) || !confirmed) {
		await showCancel("Interactive crawl cancelled.")
		return null
	}

	return plan
}

async function buildRunPlan(
	baseUrl: string | undefined,
	options: CliOptions,
): Promise<RunPlan | null> {
	const sharedError = validateSharedOptions(options)
	if (sharedError) {
		console.error(sharedError)
		return null
	}

	const shouldUseInteractive = options.interactive || !baseUrl
	if (shouldUseInteractive) {
		if (!process.stdin.isTTY || !process.stdout.isTTY) {
			console.error("Error: interactive mode requires a TTY terminal.")
			return null
		}
		return buildInteractivePlan(baseUrl, options)
	}

	if (!baseUrl) {
		console.error("Error: a base URL is required.")
		return null
	}

	const outputRoot = path.resolve(options.outputRoot)
	const mode: CrawlMode = options.epubOnly
		? "epub_only"
		: options.epub
			? "crawl_epub"
			: "crawl"

	let chapterNumbers: number[] | undefined
	let novelTitle: string | undefined

	if (!options.epubOnly) {
		try {
			const discovery = await discoverNovel(baseUrl)
			novelTitle = discovery.novelTitle
			chapterNumbers = await resolveChapterNumbers(baseUrl, options)
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error)
			console.error(`Error: ${message}`)
			return null
		}
	}

	return {
		baseUrl,
		mode,
		outputRoot,
		chapterNumbers,
		delay: options.delay,
		workers: options.workers,
		epub: options.epub || options.epubOnly,
		chapterDir: options.chapterDir
			? path.resolve(options.chapterDir)
			: undefined,
		fontPath: options.fontPath,
		ifExists: options.ifExists,
		fastSkip: options.fastSkip,
		novelTitle,
	}
}

async function executePlan(
	plan: RunPlan,
): Promise<{ code: number; epubPath?: string }> {
	let outputDir: string | null = null
	let failures: Array<[number, string]> = []

	if (plan.mode === "epub_only") {
		outputDir =
			plan.chapterDir ??
			(await inferChapterDirFromBaseUrl(plan.baseUrl, plan.outputRoot))
	} else {
		const chapterNumbers = plan.chapterNumbers ?? []
		console.log(
			`[INFO] Downloading chapters ${chapterNumbers[0]} -> ${chapterNumbers.at(-1)} (${chapterNumbers.length} chapters)`,
		)
		console.log(`[INFO] Using ${plan.workers} worker(s)`)

		if (plan.workers === 1) {
			const result = await crawlChaptersSequential({
				chapterNumbers,
				baseUrl: plan.baseUrl,
				outputRoot: plan.outputRoot,
				ifExists: plan.ifExists,
				delay: plan.delay,
				novelTitle: plan.novelTitle,
				fastSkip: plan.fastSkip,
			})
			outputDir = result.outputDir
			failures = result.failures
		} else {
			const result = await crawlChaptersParallel({
				chapterNumbers,
				baseUrl: plan.baseUrl,
				outputRoot: plan.outputRoot,
				ifExists: plan.ifExists,
				workers: plan.workers,
				novelTitle: plan.novelTitle,
				fastSkip: plan.fastSkip,
			})
			outputDir = result.outputDir
			failures = result.failures
		}
	}

	if (failures.length > 0) {
		console.error("\nSome chapters failed:")
		for (const [chapterNumber, message] of failures) {
			console.error(`  - Chapter ${chapterNumber}: ${message}`)
		}
	}

	if (plan.epub && outputDir) {
		try {
			const epubPath = await buildEpub({
				novelMainUrl: `${plan.baseUrl.replace(/\/+$/, "")}/`,
				chapterDir: outputDir,
				fontPath: plan.fontPath,
			})
			return {
				code: failures.length > 0 ? 2 : 0,
				epubPath,
			}
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error)
			console.error(`[FAIL] EPUB build failed: ${message}`)
			return { code: 3 }
		}
	}

	return { code: failures.length > 0 ? 2 : 0 }
}

export async function main(argv = process.argv): Promise<number> {
	const program = new Command()
	program
		.name("truyenazz-crawl")
		.description("Crawl a chapter range and optionally build an EPUB.")
		.argument(
			"[baseUrl]",
			"Novel base URL, e.g. https://truyenazz.me/nguoi-chong-vo-dung-cua-nu-than",
		)
		.option("--start <number>", "Start chapter number", parseIntSafe)
		.option("--end <number>", "End chapter number", parseIntSafe)
		.option("--output-root <dir>", "Root output directory", "output")
		.option(
			"--delay <seconds>",
			"Delay in seconds between requests",
			parseFloat,
			0.5,
		)
		.option(
			"--workers <number>",
			"Number of download worker threads",
			parseIntSafe,
			1,
		)
		.option("--epub", "Also build an EPUB after crawling finishes", false)
		.option(
			"--epub-only",
			"Skip chapter downloads and build the EPUB from existing saved chapter HTML files",
			false,
		)
		.option(
			"--chapter-dir <dir>",
			"Existing chapter directory to use when building EPUB without crawling",
		)
		.option(
			"--font-path <file>",
			"Font file to embed in the EPUB instead of the default packaged font",
		)
		.option(
			"--if-exists <mode>",
			"What to do if a chapter file already exists",
			ExistingFilePolicy.ASK,
		)
		.option(
			"--fast-skip",
			"Skip checking the remote URL if the chapter file already exists locally",
			false,
		)
		.option("-i, --interactive", "Launch the interactive TUI", false)

	program.parse(argv)

	const baseUrl = program.args[0] as string | undefined
	const options = program.opts<CliOptions>()

	if (options.epubOnly && !options.epub) {
		options.epub = true
	}

	const plan = await buildRunPlan(baseUrl, options)
	if (!plan) {
		return 1
	}

	if (options.interactive || !baseUrl) {
		await showNote(buildSummary(plan), "Running")
	}

	const result = await executePlan(plan)
	if (options.interactive || !baseUrl) {
		if (result.epubPath) {
			await showNote(`EPUB created at:\n${result.epubPath}`, "EPUB")
		}

		if (result.code === 0) {
			await showDone("Job completed successfully.")
		} else {
			await showCancel(`Job completed with exit code ${result.code}.`)
		}
	} else if (result.epubPath) {
		console.log(`[OK] EPUB -> ${result.epubPath}`)
	}

	return result.code
}

if (
	process.argv[1] &&
	import.meta.url === pathToFileURL(process.argv[1]).href
) {
	void main().then((code) => {
		process.exitCode = code
	})
}
