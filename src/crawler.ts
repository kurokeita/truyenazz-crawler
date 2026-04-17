import { promises as fs } from "node:fs"
import path from "node:path"
import { stdin as input, stdout as output } from "node:process"
import { createInterface } from "node:readline/promises"
import { type AnyNode, type CheerioAPI, type Element, load } from "cheerio"
import {
	type ChapterContent,
	type CrawlResult,
	ExistingFilePolicy,
} from "./types.js"
import {
	cleanText,
	ensureDir,
	fetchHtml,
	fileExists,
	isNoise,
	sleep,
	slugify,
} from "./utils.js"

export { ExistingFilePolicy }

type ExistingPolicy =
	(typeof ExistingFilePolicy)[keyof typeof ExistingFilePolicy]

const NON_CONTENT_ATTRS = new Set([
	"class",
	"style",
	"id",
	"onmousedown",
	"onselectstart",
	"oncopy",
	"oncut",
])

function escapeHtml(text: string): string {
	return text
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;")
		.replace(/'/g, "&apos;")
}

function extractTextFromElement(
	$: CheerioAPI,
	element: AnyNode,
): string | null {
	const normalText = cleanText($(element).text())
	if (normalText) {
		return normalText
	}

	const attribs = "attribs" in element ? (element.attribs ?? {}) : {}
	for (const [name, value] of Object.entries(attribs)) {
		if (NON_CONTENT_ATTRS.has(name)) {
			continue
		}
		const candidate = cleanText(String(value))
		if (candidate) {
			return candidate
		}
	}

	return null
}

function extractInjectedContentFromScript(fullHtml: string): string[] {
	const match = /var\s+contentS\s*=\s*'(.*?)';\s*div\.innerHTML/s.exec(fullHtml)
	if (!match) {
		return []
	}

	const jsHtml = match[1].replace(/\\'/g, "'").replace(/\\"/g, '"')
	const $ = load(jsHtml)
	const out: string[] = []

	$("p").each((_, element) => {
		const text = extractTextFromElement($, element)
		if (text && !isNoise(text)) {
			out.push(text)
		}
	})

	return out
}

function extractNovelTitle($: CheerioAPI): string {
	const direct = cleanText($(".rv-full-story-title h1").first().text())
	if (direct) {
		return direct
	}

	for (const element of $("h1").toArray()) {
		const title = cleanText($(element).text())
		if (title) {
			return title
		}
	}

	return "Unknown Novel"
}

function extractChapterTitle($: CheerioAPI): string {
	const direct = cleanText($(".rv-chapt-title h2").first().text())
	if (direct) {
		return direct
	}

	for (const element of $("h1, h2").toArray()) {
		const title = cleanText($(element).text())
		if (title) {
			return title
		}
	}

	return "Untitled Chapter"
}

export function extractFullChapterText(fullHtml: string): ChapterContent {
	const $ = load(fullHtml)
	const chapter = $(".chapter-c").first()
	if (chapter.length === 0) {
		throw new Error("Could not find .chapter-c in the HTML")
	}

	const novelTitle = extractNovelTitle($)
	const chapterTitle = extractChapterTitle($)
	const injectedLines = extractInjectedContentFromScript(fullHtml)
	const lines: string[] = []

	chapter.contents().each((_, node) => {
		if (node.type === "text" || node.type !== "tag") {
			return
		}

		const child = $(node)
		if (
			node.tagName === "div" &&
			child.attr("id") === "data-content-truyen-backup"
		) {
			for (const line of injectedLines) {
				if (line && !isNoise(line)) {
					lines.push(line)
				}
			}
			return
		}

		if (node.tagName === "p" || node.tagName === "span") {
			const text = extractTextFromElement($, node)
			if (text && !isNoise(text)) {
				lines.push(text)
			}
			return
		}

		child.find("p").each((__, paragraph) => {
			const text = extractTextFromElement($, paragraph)
			if (text && !isNoise(text)) {
				lines.push(text)
			}
		})
	})

	const normalized: string[] = []
	for (const line of lines) {
		const cleaned = cleanText(line)
		if (!cleaned || isNoise(cleaned)) {
			continue
		}
		if (normalized.at(-1) === cleaned) {
			continue
		}
		normalized.push(cleaned)
	}

	return {
		novelTitle,
		chapterTitle,
		paragraphs: normalized,
	}
}

export function buildChapterUrl(
	baseUrl: string,
	chapterNumber: number,
): string {
	return `${baseUrl.replace(/\/+$/, "")}/chuong-${chapterNumber}/`
}

export function buildHtmlDocument(
	novelTitle: string,
	chapterTitle: string,
	paragraphs: string[],
): string {
	const safeNovelTitle = escapeHtml(novelTitle)
	const safeChapterTitle = escapeHtml(chapterTitle)
	const bodyParagraphs = paragraphs
		.map((paragraph) => `        <p>${escapeHtml(paragraph)}</p>`)
		.join("\n")

	return `<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>${safeChapterTitle}</title>
    <link
        href="https://fonts.googleapis.com/css2?family=Literata&display=swap"
        rel="stylesheet"
    >
    <style>
        body {
            margin: 0;
            padding: 0;
            background: #f6f1e7;
            color: #222;
            font-family: "Bookerly", "Literata", "Georgia", "Times New Roman", serif;
            line-height: 1.9;
        }

        .container {
            max-width: 860px;
            margin: 0 auto;
            padding: 48px 28px 72px;
        }

        .novel-title {
            text-align: center;
            font-size: 1rem;
            color: #666;
            margin-bottom: 12px;
        }

        .chapter-title {
            text-align: center;
            font-size: 2.2rem;
            font-weight: 700;
            line-height: 1.3;
            margin: 0 0 36px;
        }

        .chapter-content p {
            font-size: 1.2rem;
            margin: 0 0 1.15em;
            text-align: justify;
            text-indent: 2em;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="novel-title">${safeNovelTitle}</div>
        <h1 class="chapter-title">${safeChapterTitle}</h1>
        <div class="chapter-content">
${bodyParagraphs}
        </div>
    </div>
</body>
</html>`
}

async function promptExistingChapter(
	chapterPath: string,
): Promise<"redownload" | "skip" | "skip_all"> {
	if (process.stdin.isTTY && process.stdout.isTTY) {
		const { promptExistingChapterAction } = await import("./ui.js")
		return promptExistingChapterAction(chapterPath)
	}

	const rl = createInterface({ input, output })
	try {
		while (true) {
			const answer = (
				await rl.question(
					`[EXISTS] ${chapterPath}\nChoose: [r]edownload / [s]kip / skip [a]ll existing: `,
				)
			)
				.trim()
				.toLowerCase()

			if (answer === "r" || answer === "redownload") {
				return "redownload"
			}
			if (answer === "s" || answer === "skip") {
				return "skip"
			}
			if (answer === "a" || answer === "all" || answer === "skip_all") {
				return "skip_all"
			}

			console.log("Please enter r, s, or a.")
		}
	} finally {
		rl.close()
	}
}

async function resolveExistingFileAction(
	outputPath: string,
	ifExists: ExistingPolicy,
	existingPolicy: ExistingPolicy,
): Promise<"write" | "skip" | "skip_all"> {
	if (!(await fileExists(outputPath))) {
		return "write"
	}

	if (existingPolicy === ExistingFilePolicy.SKIP_ALL) {
		return "skip"
	}

	if (ifExists === ExistingFilePolicy.SKIP) {
		return "skip"
	}

	if (ifExists === ExistingFilePolicy.OVERWRITE) {
		return "write"
	}

	const decision = await promptExistingChapter(outputPath)
	if (decision === "redownload") {
		return "write"
	}
	if (decision === "skip") {
		return "skip"
	}
	return "skip_all"
}

async function saveChapterFile(params: {
	outputRoot: string
	novelTitle: string
	chapterNumber: number
	htmlDoc: string
	ifExists: ExistingPolicy
	existingPolicy: ExistingPolicy
}): Promise<{
	outputDir: string
	outputPath: string
	status: CrawlResult["status"]
}> {
	const novelSlug = slugify(params.novelTitle)
	const outputDir = path.join(params.outputRoot, novelSlug)
	await ensureDir(outputDir)

	const outputPath = path.join(
		outputDir,
		`chapter_${String(params.chapterNumber).padStart(4, "0")}.html`,
	)
	const action = await resolveExistingFileAction(
		outputPath,
		params.ifExists,
		params.existingPolicy,
	)

	if (action === "skip") {
		return { outputDir, outputPath, status: "skipped" }
	}

	if (action === "skip_all") {
		return { outputDir, outputPath, status: "skip_all" }
	}

	await fs.writeFile(outputPath, params.htmlDoc, "utf8")
	return { outputDir, outputPath, status: "written" }
}

export async function crawlChapter(params: {
	baseUrl: string
	chapterNumber: number
	outputRoot: string
	ifExists: ExistingPolicy
	existingPolicy: ExistingPolicy
	delay: number
}): Promise<CrawlResult> {
	const url = buildChapterUrl(params.baseUrl, params.chapterNumber)
	const fullHtml = await fetchHtml(url)
	const chapter = extractFullChapterText(fullHtml)

	if (chapter.paragraphs.length === 0) {
		throw new Error(`No chapter content extracted from ${url}`)
	}

	const htmlDoc = buildHtmlDocument(
		chapter.novelTitle,
		chapter.chapterTitle,
		chapter.paragraphs,
	)

	const saved = await saveChapterFile({
		outputRoot: params.outputRoot,
		novelTitle: chapter.novelTitle,
		chapterNumber: params.chapterNumber,
		htmlDoc,
		ifExists: params.ifExists,
		existingPolicy: params.existingPolicy,
	})

	if (saved.status === "skip_all") {
		console.log(`[SKIP] Chapter ${params.chapterNumber} -> ${saved.outputPath}`)
		return {
			novelTitle: chapter.novelTitle,
			outputDir: saved.outputDir,
			outputPath: saved.outputPath,
			status: "skip_all",
		}
	}

	if (saved.status === "skipped") {
		console.log(`[SKIP] Chapter ${params.chapterNumber} -> ${saved.outputPath}`)
		return {
			novelTitle: chapter.novelTitle,
			outputDir: saved.outputDir,
			outputPath: saved.outputPath,
			status: "skipped",
		}
	}

	await sleep(params.delay)
	console.log(`[OK] Chapter ${params.chapterNumber} -> ${saved.outputPath}`)
	return {
		novelTitle: chapter.novelTitle,
		outputDir: saved.outputDir,
		outputPath: saved.outputPath,
		status: "written",
	}
}

export async function discoverLastChapterNumber(
	baseUrl: string,
): Promise<number> {
	const mainUrl = `${baseUrl.replace(/\/+$/, "")}/`
	const htmlSource = await fetchHtml(mainUrl)
	const $ = load(htmlSource)

	let latestHeading: Element | undefined
	$("h3").each((_, element) => {
		if (latestHeading) {
			return
		}
		const text = cleanText($(element).text()).normalize("NFC")
		if (text === "Chương Mới Nhất".normalize("NFC")) {
			latestHeading = element
		}
	})

	if (!latestHeading) {
		throw new Error("Could not find the 'Chương Mới Nhất' section.")
	}

	const latestSection = $(latestHeading).parent("div")
	if (latestSection.length === 0) {
		throw new Error("Could not find the container for 'Chương Mới Nhất'.")
	}

	const chapterListContainer = latestSection.next("div")
	if (chapterListContainer.length === 0) {
		throw new Error(
			"Could not find the chapter list next to 'Chương Mới Nhất'.",
		)
	}

	const chapterItems = chapterListContainer.find("ul li").toArray()
	if (chapterItems.length === 0) {
		throw new Error("Could not find any latest chapter entries.")
	}

	const lastChapterItem = chapterItems.at(-1)
	if (!lastChapterItem) {
		throw new Error("Could not find the last chapter entry.")
	}

	const lastLink = $(lastChapterItem).find("a[href]").first()
	const href = lastLink.attr("href")
	if (!href) {
		throw new Error("Could not find a link for the last chapter entry.")
	}

	const absoluteUrl = new URL(href, mainUrl).toString()
	const match = /\/chuong-(\d+)\/?$/.exec(absoluteUrl)
	if (!match) {
		throw new Error(
			`Could not extract the last chapter number from ${absoluteUrl}.`,
		)
	}

	return Number.parseInt(match[1], 10)
}
