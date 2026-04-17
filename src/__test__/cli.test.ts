import path from "node:path"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

const ExistingFilePolicy = {
	ASK: "ask",
	SKIP: "skip",
	OVERWRITE: "overwrite",
	SKIP_ALL: "skip_all",
} as const

function createSpinnerMock() {
	return {
		start: vi.fn(),
		stop: vi.fn(),
	}
}

async function loadCli() {
	vi.resetModules()

	const crawlChapter = vi.fn()
	const discoverLastChapterNumber = vi.fn()
	const buildEpub = vi.fn()
	const spinner = createSpinnerMock()
	const showWelcome = vi.fn()
	const showDone = vi.fn()
	const showCancel = vi.fn()
	const showNote = vi.fn()
	const promptText = vi.fn()
	const promptSelect = vi.fn()
	const promptConfirm = vi.fn()
	const promptPath = vi.fn()
	const isPromptCancel = vi.fn().mockResolvedValue(false)
	const fetchMainHtmlForCli = vi.fn().mockResolvedValue("<html></html>")
	const extractNovelTitleFromMainPageForCli = vi.fn().mockReturnValue("My Book")
	const slugifyForCli = vi.fn().mockReturnValue("my_book")

	vi.doMock("../crawler.js", () => ({
		ExistingFilePolicy,
		crawlChapter,
		discoverLastChapterNumber,
	}))
	vi.doMock("../epub.js", () => ({ buildEpub }))
	vi.doMock("../ui.js", () => ({
		createSpinner: vi.fn(async () => spinner),
		isPromptCancel,
		promptConfirm,
		promptPath,
		promptSelect,
		promptText,
		showCancel,
		showDone,
		showNote,
		showWelcome,
	}))
	vi.doMock("../internal-cli-helpers.js", () => ({
		extractNovelTitleFromMainPageForCli,
		fetchMainHtmlForCli,
		slugifyForCli,
	}))

	const cli = await import("../cli.js")
	return {
		main: cli.main,
		mocks: {
			buildEpub,
			crawlChapter,
			discoverLastChapterNumber,
			extractNovelTitleFromMainPageForCli,
			fetchMainHtmlForCli,
			isPromptCancel,
			promptConfirm,
			promptPath,
			promptSelect,
			promptText,
			showCancel,
			showDone,
			showNote,
			showWelcome,
			slugifyForCli,
			spinner,
		},
	}
}

const originalStdinTty = process.stdin.isTTY
const originalStdoutTty = process.stdout.isTTY

beforeEach(() => {
	vi.restoreAllMocks()
	vi.spyOn(console, "error").mockImplementation(() => undefined)
	vi.spyOn(console, "log").mockImplementation(() => undefined)
	Object.defineProperty(process.stdin, "isTTY", {
		value: originalStdinTty,
		configurable: true,
	})
	Object.defineProperty(process.stdout, "isTTY", {
		value: originalStdoutTty,
		configurable: true,
	})
})

afterEach(() => {
	Object.defineProperty(process.stdin, "isTTY", {
		value: originalStdinTty,
		configurable: true,
	})
	Object.defineProperty(process.stdout, "isTTY", {
		value: originalStdoutTty,
		configurable: true,
	})
})

describe("cli main", () => {
	it("fails fast for an unsupported existing-file mode", async () => {
		const { main } = await loadCli()

		await expect(
			main(["node", "cli", "https://example.com/book", "--if-exists", "bad"]),
		).resolves.toBe(1)

		expect(console.error).toHaveBeenCalledWith(
			"Error: --if-exists must be ask, skip, or overwrite.",
		)
	})

	it("fails fast for invalid worker counts and unsupported ask mode in parallel", async () => {
		const { main } = await loadCli()

		await expect(
			main(["node", "cli", "https://example.com/book", "--workers", "0"]),
		).resolves.toBe(1)
		expect(console.error).toHaveBeenCalledWith(
			"Error: --workers must be a positive integer.",
		)

		await expect(
			main([
				"node",
				"cli",
				"https://example.com/book",
				"--workers",
				"2",
				"--if-exists",
				"ask",
			]),
		).resolves.toBe(1)
		expect(console.error).toHaveBeenCalledWith(
			"Error: --workers > 1 requires --if-exists skip or --if-exists overwrite.",
		)
	})

	it("crawls the discovered chapter range sequentially", async () => {
		const { main, mocks } = await loadCli()
		mocks.discoverLastChapterNumber.mockResolvedValue(3)
		mocks.crawlChapter.mockImplementation(async ({ chapterNumber }) => ({
			novelTitle: "My Book",
			outputDir: "/tmp/book",
			outputPath: `/tmp/book/chapter_${chapterNumber}.html`,
			status: "written",
		}))

		await expect(
			main(["node", "cli", "https://example.com/book"]),
		).resolves.toBe(0)

		expect(mocks.crawlChapter).toHaveBeenCalledTimes(3)
		expect(mocks.crawlChapter).toHaveBeenNthCalledWith(
			1,
			expect.objectContaining({ chapterNumber: 1 }),
		)
		expect(mocks.crawlChapter).toHaveBeenNthCalledWith(
			3,
			expect.objectContaining({ chapterNumber: 3 }),
		)
	})

	it("fails for an invalid explicit chapter range", async () => {
		const { main } = await loadCli()

		await expect(
			main([
				"node",
				"cli",
				"https://example.com/book",
				"--start",
				"5",
				"--end",
				"3",
			]),
		).resolves.toBe(1)
		expect(console.error).toHaveBeenCalledWith(
			"Error: --start must be less than or equal to --end.",
		)
	})

	it("truncates the requested end chapter to the discovered latest chapter", async () => {
		const { main, mocks } = await loadCli()

		mocks.discoverLastChapterNumber.mockResolvedValue(3)
		mocks.crawlChapter.mockImplementation(async ({ chapterNumber }) => ({
			novelTitle: "My Book",
			outputDir: "/tmp/book",
			outputPath: `/tmp/book/chapter_${chapterNumber}.html`,
			status: "written",
		}))
		await expect(
			main(["node", "cli", "https://example.com/book", "--end", "9"]),
		).resolves.toBe(0)
		expect(console.log).toHaveBeenCalledWith(
			"[INFO] Requested end chapter 9 exceeds the last available chapter 3; stopping at 3.",
		)
		expect(mocks.crawlChapter).toHaveBeenCalledTimes(3)
	})

	it("builds an epub-only run using the inferred chapter directory", async () => {
		const { main, mocks } = await loadCli()
		mocks.buildEpub.mockResolvedValue("/tmp/book.epub")

		const outputRoot = path.resolve("output")
		await expect(
			main(["node", "cli", "https://example.com/book", "--epub-only"]),
		).resolves.toBe(0)

		expect(mocks.fetchMainHtmlForCli).toHaveBeenCalled()
		expect(mocks.buildEpub).toHaveBeenCalledWith({
			novelMainUrl: "https://example.com/book/",
			chapterDir: path.join(outputRoot, "my_book"),
			fontPath: undefined,
		})
		expect(console.log).toHaveBeenCalledWith("[OK] EPUB -> /tmp/book.epub")
	})

	it("runs the interactive epub-only flow and shows structured success output", async () => {
		Object.defineProperty(process.stdin, "isTTY", {
			value: true,
			configurable: true,
		})
		Object.defineProperty(process.stdout, "isTTY", {
			value: true,
			configurable: true,
		})

		const { main, mocks } = await loadCli()
		mocks.discoverLastChapterNumber.mockResolvedValue(42)
		mocks.promptText
			.mockResolvedValueOnce("https://example.com/book")
			.mockResolvedValueOnce("output")
			.mockResolvedValueOnce("/tmp/existing-chapters")
		mocks.promptSelect
			.mockResolvedValueOnce("epub_only")
			.mockResolvedValueOnce("default")
		mocks.promptConfirm.mockResolvedValue(true)
		mocks.buildEpub.mockResolvedValue("/tmp/book.epub")

		await expect(main(["node", "cli"])).resolves.toBe(0)

		expect(mocks.showWelcome).toHaveBeenCalled()
		expect(mocks.showNote).toHaveBeenCalledWith(
			expect.stringContaining("Latest chapter: 42"),
			"Novel",
		)
		expect(mocks.showDone).toHaveBeenCalledWith("Job completed successfully.")
		expect(mocks.showNote).toHaveBeenCalledWith(
			"EPUB created at:\n/tmp/book.epub",
			"EPUB",
		)
	})

	it("runs the interactive crawl-and-epub flow with custom font selection", async () => {
		Object.defineProperty(process.stdin, "isTTY", {
			value: true,
			configurable: true,
		})
		Object.defineProperty(process.stdout, "isTTY", {
			value: true,
			configurable: true,
		})

		const { main, mocks } = await loadCli()
		mocks.discoverLastChapterNumber.mockResolvedValue(12)
		mocks.promptText.mockImplementation(async (params) => {
			if (params.message === "Novel base URL") {
				expect(params.validate?.("https://example.com/book")).toBeUndefined()
				return "https://example.com/book"
			}
			if (params.message === "Output root directory") {
				expect(params.validate?.("output")).toBeUndefined()
				return "output"
			}
			if (params.message === "Delay between chapter requests (seconds)") {
				expect(params.validate?.("0")).toBeUndefined()
				return "0"
			}
			throw new Error(`Unexpected promptText message: ${params.message}`)
		})
		mocks.promptSelect
			.mockResolvedValueOnce("crawl_epub")
			.mockResolvedValueOnce("overwrite")
			.mockResolvedValueOnce("custom")
		mocks.promptPath.mockImplementation(async (params) => {
			expect(params.root).toBe(process.cwd())
			expect(params.validate?.("Bokerlam.ttf")).toBeUndefined()
			return "Bokerlam.ttf"
		})
		mocks.promptConfirm.mockResolvedValue(true)
		mocks.buildEpub.mockResolvedValue("/tmp/book.epub")
		mocks.crawlChapter.mockImplementation(async ({ chapterNumber }) => ({
			novelTitle: "My Book",
			outputDir: "/tmp/book",
			outputPath: `/tmp/book/chapter_${chapterNumber}.html`,
			status: "written",
		}))

		const positiveIntegers = [2, 3, 2]
		let positiveIndex = 0
		mocks.isPromptCancel.mockResolvedValue(false)
		mocks.promptText.mockImplementation(async (params) => {
			if (params.message === "Novel base URL") {
				expect(params.validate?.("https://example.com/book")).toBeUndefined()
				return "https://example.com/book"
			}
			if (params.message === "Output root directory") {
				expect(params.validate?.("output")).toBeUndefined()
				return "output"
			}
			if (params.message === "Delay between chapter requests (seconds)") {
				expect(params.validate?.("0")).toBeUndefined()
				return "0"
			}
			if (
				params.message === "Start chapter" ||
				params.message === "End chapter" ||
				params.message === "Download workers"
			) {
				const value = String(positiveIntegers[positiveIndex++])
				expect(params.validate?.(value)).toBeUndefined()
				return value
			}
			throw new Error(`Unexpected promptText message: ${params.message}`)
		})

		await expect(main(["node", "cli"])).resolves.toBe(0)
		expect(mocks.crawlChapter).toHaveBeenCalledTimes(2)
		expect(mocks.buildEpub).toHaveBeenCalledWith({
			novelMainUrl: "https://example.com/book/",
			chapterDir: "/tmp/book",
			fontPath: "Bokerlam.ttf",
		})
	})

	it("resolves chapter numbers with various starts and limits", async () => {
		const { main, mocks } = await loadCli()
		mocks.discoverLastChapterNumber.mockResolvedValue(10)
		mocks.crawlChapter.mockImplementation(async ({ chapterNumber }) => ({
			novelTitle: "My Book",
			outputDir: "/tmp/book",
			outputPath: `/tmp/book/chapter_${chapterNumber}.html`,
			status: "written",
		}))

		const code1 = await main([
			"node",
			"cli",
			"https://example.com/book",
			"--start",
			"8",
		])
		expect(code1).toBe(0)
		expect(mocks.crawlChapter).toHaveBeenCalledTimes(3) // 8, 9, 10

		const codeEndExceeds = await main([
			"node",
			"cli",
			"https://example.com/book",
			"--end",
			"12",
		])
		expect(codeEndExceeds).toBe(0)
		expect(console.log).toHaveBeenCalledWith(
			expect.stringContaining(
				"Requested end chapter 12 exceeds the last available chapter 10",
			),
		)

		const code2 = await main([
			"node",
			"cli",
			"https://example.com/book",
			"--start",
			"11",
		])
		expect(code2).toBe(1)

		const code3 = await main([
			"node",
			"cli",
			"https://example.com/book",
			"--start",
			"0",
		])
		expect(code3).toBe(1)
	})

	it("handles skip_all policy transition in sequential downloads", async () => {
		const { main, mocks } = await loadCli()
		mocks.discoverLastChapterNumber.mockResolvedValue(2)
		mocks.crawlChapter
			.mockResolvedValueOnce({
				novelTitle: "My Book",
				outputDir: "/tmp/book",
				outputPath: "/tmp/book/chapter_1.html",
				status: "skip_all",
			})
			.mockResolvedValueOnce({
				novelTitle: "My Book",
				outputDir: "/tmp/book",
				outputPath: "/tmp/book/chapter_2.html",
				status: "skipped",
			})

		await expect(
			main(["node", "cli", "https://example.com/book"]),
		).resolves.toBe(0)
		expect(mocks.crawlChapter).toHaveBeenNthCalledWith(
			2,
			expect.objectContaining({ existingPolicy: "skip_all" }),
		)
	})

	it("rejects interactive mode when no tty is available", async () => {
		Object.defineProperty(process.stdin, "isTTY", {
			value: false,
			configurable: true,
		})
		Object.defineProperty(process.stdout, "isTTY", {
			value: false,
			configurable: true,
		})

		const { main } = await loadCli()

		await expect(main(["node", "cli", "--interactive"])).resolves.toBe(1)
		expect(console.error).toHaveBeenCalledWith(
			"Error: interactive mode requires a TTY terminal.",
		)
	})

	it("stops the interactive flow when novel discovery fails", async () => {
		Object.defineProperty(process.stdin, "isTTY", {
			value: true,
			configurable: true,
		})
		Object.defineProperty(process.stdout, "isTTY", {
			value: true,
			configurable: true,
		})

		const { main, mocks } = await loadCli()
		mocks.promptText
			.mockResolvedValueOnce("https://example.com/book")
			.mockResolvedValueOnce("output")
		mocks.promptSelect.mockResolvedValueOnce("epub_only")
		mocks.discoverLastChapterNumber.mockRejectedValue(
			new Error("discover failed"),
		)

		await expect(main(["node", "cli"])).resolves.toBe(1)

		expect(mocks.spinner.start).toHaveBeenCalledWith(
			"Discovering novel information...",
		)
		expect(mocks.spinner.stop).toHaveBeenCalledWith(
			"Failed to discover novel information",
		)
		expect(mocks.showCancel).toHaveBeenCalledWith("Error: discover failed")
	})

	it("cancels the interactive flow at the base url, action, and confirmation steps", async () => {
		Object.defineProperty(process.stdin, "isTTY", {
			value: true,
			configurable: true,
		})
		Object.defineProperty(process.stdout, "isTTY", {
			value: true,
			configurable: true,
		})

		{
			const { main, mocks } = await loadCli()
			const canceled = Symbol("cancel")
			mocks.promptText.mockResolvedValueOnce(canceled)
			mocks.isPromptCancel.mockResolvedValue(true)

			await expect(main(["node", "cli"])).resolves.toBe(1)
			expect(mocks.showCancel).toHaveBeenCalledWith(
				"Interactive crawl cancelled.",
			)
		}

		{
			const { main, mocks } = await loadCli()
			const canceled = Symbol("cancel")
			mocks.promptText.mockResolvedValueOnce("https://example.com/book")
			mocks.promptSelect.mockResolvedValueOnce(canceled)
			mocks.isPromptCancel
				.mockResolvedValueOnce(false)
				.mockResolvedValueOnce(true)

			await expect(main(["node", "cli"])).resolves.toBe(1)
			expect(mocks.showCancel).toHaveBeenCalledWith(
				"Interactive crawl cancelled.",
			)
		}

		{
			const { main, mocks } = await loadCli()
			mocks.discoverLastChapterNumber.mockResolvedValue(10)
			mocks.promptText
				.mockResolvedValueOnce("https://example.com/book")
				.mockResolvedValueOnce("output")
				.mockResolvedValueOnce("/tmp/existing")
			mocks.promptSelect
				.mockResolvedValueOnce("epub_only")
				.mockResolvedValueOnce("default")
			mocks.promptConfirm.mockResolvedValue(false)
			mocks.isPromptCancel.mockResolvedValue(false)

			await expect(main(["node", "cli"])).resolves.toBe(1)
			expect(mocks.showCancel).toHaveBeenCalledWith(
				"Interactive crawl cancelled.",
			)
		}
	})

	it("returns exit code 3 when epub generation fails", async () => {
		const { main, mocks } = await loadCli()
		mocks.buildEpub.mockRejectedValue(new Error("epub failed"))

		await expect(
			main([
				"node",
				"cli",
				"https://example.com/book",
				"--epub-only",
				"--chapter-dir",
				"/tmp/book",
			]),
		).resolves.toBe(3)

		expect(console.error).toHaveBeenCalledWith(
			"[FAIL] EPUB build failed: epub failed",
		)
	})

	it("returns exit code 2 and prints a failure summary when sequential chapter downloads fail", async () => {
		const { main, mocks } = await loadCli()
		mocks.discoverLastChapterNumber.mockResolvedValue(1)
		mocks.crawlChapter.mockRejectedValue(new Error("download failed"))

		await expect(
			main(["node", "cli", "https://example.com/book"]),
		).resolves.toBe(2)

		expect(console.error).toHaveBeenCalledWith(
			"[FAIL] Chapter 1: download failed",
		)
		expect(console.error).toHaveBeenCalledWith("\nSome chapters failed:")
		expect(console.error).toHaveBeenCalledWith("  - Chapter 1: download failed")
	})

	it("returns exit code 2 and sorts failures when parallel chapter downloads fail", async () => {
		const { main, mocks } = await loadCli()
		mocks.crawlChapter.mockImplementation(async ({ chapterNumber }) => {
			if (chapterNumber === 2) {
				throw new Error("download failed")
			}
			return {
				novelTitle: "My Book",
				outputDir: "/tmp/book",
				outputPath: `/tmp/book/chapter_${chapterNumber}.html`,
				status: "written",
			}
		})

		await expect(
			main([
				"node",
				"cli",
				"https://example.com/book",
				"--start",
				"1",
				"--end",
				"2",
				"--workers",
				"2",
				"--if-exists",
				"skip",
			]),
		).resolves.toBe(2)

		expect(console.error).toHaveBeenCalledWith(
			"[FAIL] Chapter 2: download failed",
		)
		expect(console.error).toHaveBeenCalledWith("  - Chapter 2: download failed")
	})

	it("cancels at later interactive steps and exercises prompt validators", async () => {
		Object.defineProperty(process.stdin, "isTTY", {
			value: true,
			configurable: true,
		})
		Object.defineProperty(process.stdout, "isTTY", {
			value: true,
			configurable: true,
		})

		{
			const { main, mocks } = await loadCli()
			mocks.discoverLastChapterNumber.mockResolvedValue(10)
			mocks.promptText.mockImplementation(async (params) => {
				if (params.message === "Novel base URL") {
					expect(params.validate?.("bad-url")).toBe(
						"Enter a valid http:// or https:// URL.",
					)
					return "https://example.com/book"
				}
				if (params.message === "Output root directory") {
					expect(params.validate?.("")).toBe("Enter an output directory.")
					return Symbol("cancel")
				}
				throw new Error(`Unexpected promptText message: ${params.message}`)
			})
			mocks.promptSelect.mockResolvedValueOnce("crawl")
			mocks.isPromptCancel
				.mockResolvedValueOnce(false)
				.mockResolvedValueOnce(false)
				.mockResolvedValueOnce(true)

			await expect(main(["node", "cli"])).resolves.toBe(1)
			expect(mocks.showCancel).toHaveBeenCalledWith(
				"Interactive crawl cancelled.",
			)
		}

		{
			const { main, mocks } = await loadCli()
			mocks.discoverLastChapterNumber.mockResolvedValue(10)
			mocks.promptText.mockImplementation(async (params) => {
				if (params.message === "Novel base URL") {
					return "https://example.com/book"
				}
				if (params.message === "Output root directory") {
					return "output"
				}
				if (params.message === "Start chapter") {
					expect(params.validate?.("0")).toBe("Enter a positive integer.")
					expect(params.validate?.("not-a-number")).toBe(
						"Enter a positive integer.",
					)
					expect(params.validate?.("11")).toBe(
						"Value must be less than or equal to 10.",
					)
					return Symbol("cancel")
				}
				throw new Error(`Unexpected promptText message: ${params.message}`)
			})
			mocks.promptSelect.mockResolvedValueOnce("crawl")
			mocks.isPromptCancel.mockImplementation(
				async (value) => typeof value === "symbol",
			)

			await expect(main(["node", "cli"])).resolves.toBe(1)
			expect(mocks.showCancel).toHaveBeenCalledWith(
				"Interactive crawl cancelled.",
			)
		}

		{
			const { main, mocks } = await loadCli()
			mocks.discoverLastChapterNumber.mockResolvedValue(10)
			const promptValues = ["https://example.com/book", "output", "2", "1"]
			mocks.promptText.mockImplementation(async (params) => {
				if (params.message === "Delay between chapter requests (seconds)") {
					expect(params.validate?.("-1")).toBe(
						"Enter a number greater than or equal to 0.",
					)
					return Symbol("cancel")
				}
				return promptValues.shift() as string
			})
			mocks.promptSelect.mockResolvedValueOnce("crawl")
			mocks.isPromptCancel.mockImplementation(
				async (value) => typeof value === "symbol",
			)

			await expect(main(["node", "cli"])).resolves.toBe(1)
			expect(mocks.showCancel).toHaveBeenCalled()
		}

		{
			const { main, mocks } = await loadCli()
			mocks.discoverLastChapterNumber.mockResolvedValue(10)
			const promptValues = ["https://example.com/book", "output", "1", "1", "1"]
			mocks.promptText.mockImplementation(
				async () => promptValues.shift() as string,
			)
			mocks.promptSelect
				.mockResolvedValueOnce("crawl")
				.mockResolvedValueOnce(Symbol("cancel"))
			mocks.isPromptCancel.mockImplementation(
				async (value) => typeof value === "symbol",
			)

			await expect(main(["node", "cli"])).resolves.toBe(1)
			expect(mocks.showCancel).toHaveBeenCalled()
		}

		{
			const { main, mocks } = await loadCli()
			mocks.discoverLastChapterNumber.mockResolvedValue(10)
			mocks.promptText
				.mockResolvedValueOnce("https://example.com/book")
				.mockResolvedValueOnce("output")
				.mockResolvedValueOnce("/tmp/chapters")
			mocks.promptSelect
				.mockResolvedValueOnce("epub_only")
				.mockResolvedValueOnce(Symbol("cancel"))
			mocks.isPromptCancel.mockImplementation(
				async (value) => typeof value === "symbol",
			)

			await expect(main(["node", "cli"])).resolves.toBe(1)
			expect(mocks.showCancel).toHaveBeenCalled()
		}

		{
			const { main, mocks } = await loadCli()
			mocks.discoverLastChapterNumber.mockResolvedValue(10)
			mocks.promptText
				.mockResolvedValueOnce("https://example.com/book")
				.mockResolvedValueOnce("output")
				.mockResolvedValueOnce("/tmp/chapters")
			mocks.promptSelect
				.mockResolvedValueOnce("epub_only")
				.mockResolvedValueOnce("custom")
			mocks.promptPath.mockImplementation(async (params) => {
				expect(params.validate?.("")).toBe("Select a font file.")
				return Symbol("cancel")
			})
			mocks.isPromptCancel.mockImplementation(
				async (value) => typeof value === "symbol",
			)

			await expect(main(["node", "cli"])).resolves.toBe(1)
			expect(mocks.showCancel).toHaveBeenCalled()
		}
	})

	it("exercises fallback paths and interactive summary", async () => {
		Object.defineProperty(process.stdin, "isTTY", {
			value: true,
			configurable: true,
		})
		Object.defineProperty(process.stdout, "isTTY", {
			value: true,
			configurable: true,
		})

		const { main, mocks } = await loadCli()
		mocks.discoverLastChapterNumber.mockResolvedValue(1)
		mocks.crawlChapter.mockImplementation(async () => ({
			novelTitle: "My Book",
			outputDir: "/tmp/book",
			outputPath: "/tmp/book/chapter_0001.html",
			status: "skip_all",
		}))
		mocks.promptText
			.mockResolvedValueOnce("https://example.com/book")
			.mockResolvedValueOnce("output")
			.mockResolvedValueOnce("1") // start
			.mockResolvedValueOnce("1") // end
			.mockResolvedValueOnce("1") // workers
			.mockResolvedValueOnce("0.5") // delay
		mocks.promptSelect
			.mockResolvedValueOnce("crawl")
			.mockResolvedValueOnce("skip_all")
		mocks.promptConfirm.mockResolvedValue(true)

		await expect(main(["node", "cli"])).resolves.toBe(0)
		expect(mocks.showNote).toHaveBeenCalledWith(
			expect.stringContaining("If chapter exists: skip_all"),
			"Plan",
		)
		expect(mocks.showNote).toHaveBeenCalledWith(
			expect.stringContaining("Base URL: https://example.com/book"),
			"Running",
		)
	})

	it("shows cancel for non-zero exit code in interactive mode with baseUrl", async () => {
		Object.defineProperty(process.stdin, "isTTY", {
			value: true,
			configurable: true,
		})
		Object.defineProperty(process.stdout, "isTTY", {
			value: true,
			configurable: true,
		})

		const { main, mocks } = await loadCli()
		mocks.discoverLastChapterNumber.mockResolvedValue(1)
		mocks.crawlChapter.mockRejectedValue(new Error("failed"))
		mocks.promptSelect
			.mockResolvedValueOnce("crawl")
			.mockResolvedValueOnce("skip")
		mocks.promptText
			.mockResolvedValueOnce("output")
			.mockResolvedValueOnce("1") // start
			.mockResolvedValueOnce("1") // end
			.mockResolvedValueOnce("1") // workers
			.mockResolvedValueOnce("0") // delay
		mocks.promptConfirm.mockResolvedValue(true)

		await expect(
			main(["node", "cli", "https://example.com/book", "--interactive"]),
		).resolves.toBe(2)
		expect(mocks.showCancel).toHaveBeenCalledWith(
			"Job completed with exit code 2.",
		)
	})

	it("triggers inferChapterDirFromBaseUrl via epub_only execution without chapterDir", async () => {
		const { main, mocks } = await loadCli()
		mocks.buildEpub.mockResolvedValue("/tmp/book.epub")

		await expect(
			main(["node", "cli", "https://example.com/book", "--epub-only"]),
		).resolves.toBe(0)

		expect(mocks.fetchMainHtmlForCli).toHaveBeenCalled()
	})
})
