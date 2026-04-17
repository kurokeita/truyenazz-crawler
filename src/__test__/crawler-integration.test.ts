import { promises as fs } from "node:fs"
import os from "node:os"
import path from "node:path"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

const tempDirs: string[] = []
const originalStdinTty = process.stdin.isTTY
const originalStdoutTty = process.stdout.isTTY

vi.mock("node:readline/promises", () => ({
	createInterface: vi.fn().mockReturnValue({
		question: vi.fn().mockResolvedValue("s"),
		close: vi.fn(),
	}),
}))

async function loadCrawlerWithUiDecision(
	decision: "redownload" | "skip" | "skip_all",
) {
	vi.resetModules()
	vi.doMock("../ui.js", () => ({
		promptExistingChapterAction: vi.fn().mockResolvedValue(decision),
	}))
	return import("../crawler.js")
}

function chapterHtml(paragraphs: string[]): string {
	return `<!DOCTYPE html>
<html>
	<body>
		<div class="rv-full-story-title"><h1>Ten Truyen</h1></div>
		<div class="rv-chapt-title"><h2>Chuong 1</h2></div>
		<div class="chapter-c">
			${paragraphs.map((line) => `<p>${line}</p>`).join("")}
		</div>
	</body>
</html>`
}

beforeEach(() => {
	vi.restoreAllMocks()
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

afterEach(async () => {
	vi.unstubAllGlobals()
	Object.defineProperty(process.stdin, "isTTY", {
		value: originalStdinTty,
		configurable: true,
	})
	Object.defineProperty(process.stdout, "isTTY", {
		value: originalStdoutTty,
		configurable: true,
	})
	await Promise.all(
		tempDirs
			.splice(0)
			.map((dir) => fs.rm(dir, { recursive: true, force: true })),
	)
})

describe("crawler integration", () => {
	it("builds chapter urls and html documents", async () => {
		const { buildChapterUrl, buildHtmlDocument } = await import("../crawler.js")

		expect(buildChapterUrl("https://example.com/book/", 12)).toBe(
			"https://example.com/book/chuong-12/",
		)
		expect(
			buildHtmlDocument("Ten Truyen", "Chuong 1", ["Dong 1", "Dong 2"]),
		).toContain('<h1 class="chapter-title">Chuong 1</h1>')
	})

	it("writes a crawled chapter to disk", async () => {
		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-crawl-"))
		tempDirs.push(tempDir)
		const { crawlChapter, ExistingFilePolicy } = await import("../crawler.js")

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValue(
					new Response(chapterHtml(["Dong 1"]), { status: 200 }),
				),
		)

		const result = await crawlChapter({
			baseUrl: "https://example.com/book",
			chapterNumber: 1,
			outputRoot: tempDir,
			ifExists: ExistingFilePolicy.OVERWRITE,
			existingPolicy: ExistingFilePolicy.ASK,
			delay: 0,
		})

		expect(result.status).toBe("written")
		expect(await fs.readFile(result.outputPath, "utf8")).toContain("Dong 1")
	})

	it("overwrites an existing file when the policy is overwrite", async () => {
		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-crawl-"))
		tempDirs.push(tempDir)
		const { crawlChapter, ExistingFilePolicy } = await import("../crawler.js")

		const outputDir = path.join(tempDir, "ten_truyen")
		await fs.mkdir(outputDir, { recursive: true })
		const outputPath = path.join(outputDir, "chapter_0001.html")
		await fs.writeFile(outputPath, "old", "utf8")

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValue(new Response(chapterHtml(["new"]), { status: 200 })),
		)

		const result = await crawlChapter({
			baseUrl: "https://example.com/book",
			chapterNumber: 1,
			outputRoot: tempDir,
			ifExists: ExistingFilePolicy.OVERWRITE,
			existingPolicy: ExistingFilePolicy.ASK,
			delay: 0,
		})

		expect(result.status).toBe("written")
		expect(await fs.readFile(outputPath, "utf8")).toContain("new")
	})

	it("skips an existing file when the policy is skip", async () => {
		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-crawl-"))
		tempDirs.push(tempDir)
		const { crawlChapter, ExistingFilePolicy } = await import("../crawler.js")

		const outputDir = path.join(tempDir, "ten_truyen")
		await fs.mkdir(outputDir, { recursive: true })
		const outputPath = path.join(outputDir, "chapter_0001.html")
		await fs.writeFile(outputPath, "existing", "utf8")

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValue(
					new Response(chapterHtml(["Dong 1"]), { status: 200 }),
				),
		)

		const result = await crawlChapter({
			baseUrl: "https://example.com/book",
			chapterNumber: 1,
			outputRoot: tempDir,
			ifExists: ExistingFilePolicy.SKIP,
			existingPolicy: ExistingFilePolicy.ASK,
			delay: 0,
		})

		expect(result.status).toBe("skipped")
		expect(await fs.readFile(outputPath, "utf8")).toBe("existing")
	})

	it("skips all subsequent chapters when skip_all policy is set", async () => {
		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-crawl-"))
		tempDirs.push(tempDir)
		const { crawlChapter, ExistingFilePolicy } = await import("../crawler.js")

		const outputDir = path.join(tempDir, "ten_truyen")
		await fs.mkdir(outputDir, { recursive: true })
		const outputPath = path.join(outputDir, "chapter_0001.html")
		await fs.writeFile(outputPath, "existing", "utf8")

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValue(new Response(chapterHtml(["D"]), { status: 200 })),
		)

		const result = await crawlChapter({
			baseUrl: "https://example.com/book",
			chapterNumber: 1,
			outputRoot: tempDir,
			ifExists: ExistingFilePolicy.ASK,
			existingPolicy: ExistingFilePolicy.SKIP_ALL,
			delay: 0,
		})

		expect(result.status).toBe("skipped")
	})

	it("uses the interactive decision when an existing file is encountered", async () => {
		Object.defineProperty(process.stdin, "isTTY", {
			value: true,
			configurable: true,
		})
		Object.defineProperty(process.stdout, "isTTY", {
			value: true,
			configurable: true,
		})

		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-crawl-"))
		tempDirs.push(tempDir)
		const { crawlChapter, ExistingFilePolicy } =
			await loadCrawlerWithUiDecision("skip_all")

		const outputDir = path.join(tempDir, "ten_truyen")
		await fs.mkdir(outputDir, { recursive: true })
		await fs.writeFile(
			path.join(outputDir, "chapter_0001.html"),
			"existing",
			"utf8",
		)

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValue(
					new Response(chapterHtml(["Dong 1"]), { status: 200 }),
				),
		)

		const result = await crawlChapter({
			baseUrl: "https://example.com/book",
			chapterNumber: 1,
			outputRoot: tempDir,
			ifExists: ExistingFilePolicy.ASK,
			existingPolicy: ExistingFilePolicy.ASK,
			delay: 0,
		})

		expect(result.status).toBe("skip_all")
	})

	it("uses the interactive skip decision", async () => {
		Object.defineProperty(process.stdin, "isTTY", {
			value: true,
			configurable: true,
		})
		Object.defineProperty(process.stdout, "isTTY", {
			value: true,
			configurable: true,
		})

		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-crawl-"))
		tempDirs.push(tempDir)
		const { crawlChapter, ExistingFilePolicy } =
			await loadCrawlerWithUiDecision("skip")

		const outputDir = path.join(tempDir, "ten_truyen")
		await fs.mkdir(outputDir, { recursive: true })
		await fs.writeFile(path.join(outputDir, "chapter_0001.html"), "ex", "utf8")

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValue(new Response(chapterHtml(["D"]), { status: 200 })),
		)

		const result = await crawlChapter({
			baseUrl: "https://example.com/book",
			chapterNumber: 1,
			outputRoot: tempDir,
			ifExists: ExistingFilePolicy.ASK,
			existingPolicy: ExistingFilePolicy.ASK,
			delay: 0,
		})

		expect(result.status).toBe("skipped")
	})

	it("redownloads an existing chapter when the interactive decision requests overwrite", async () => {
		Object.defineProperty(process.stdin, "isTTY", {
			value: true,
			configurable: true,
		})
		Object.defineProperty(process.stdout, "isTTY", {
			value: true,
			configurable: true,
		})

		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-crawl-"))
		tempDirs.push(tempDir)
		const { crawlChapter, ExistingFilePolicy } =
			await loadCrawlerWithUiDecision("redownload")

		const outputDir = path.join(tempDir, "ten_truyen")
		await fs.mkdir(outputDir, { recursive: true })
		const outputPath = path.join(outputDir, "chapter_0001.html")
		await fs.writeFile(outputPath, "existing", "utf8")

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValue(
					new Response(chapterHtml(["Dong moi"]), { status: 200 }),
				),
		)

		const result = await crawlChapter({
			baseUrl: "https://example.com/book",
			chapterNumber: 1,
			outputRoot: tempDir,
			ifExists: ExistingFilePolicy.ASK,
			existingPolicy: ExistingFilePolicy.ASK,
			delay: 0,
		})

		expect(result.status).toBe("written")
		expect(await fs.readFile(outputPath, "utf8")).toContain("Dong moi")
	})

	it("uses readline for existing file decision when not a TTY", async () => {
		Object.defineProperty(process.stdin, "isTTY", {
			value: false,
			configurable: true,
		})
		Object.defineProperty(process.stdout, "isTTY", {
			value: false,
			configurable: true,
		})

		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-crawl-"))
		tempDirs.push(tempDir)

		vi.resetModules()
		const readline = await import("node:readline/promises")
		const mockQuestion = vi
			.fn()
			.mockResolvedValueOnce("invalid")
			.mockResolvedValueOnce("a")
		vi.spyOn(readline, "createInterface").mockReturnValue({
			question: mockQuestion,
			close: vi.fn(),
		} as any)

		const { crawlChapter, ExistingFilePolicy } = await import("../crawler.js")

		const outputDir = path.join(tempDir, "ten_truyen")
		await fs.mkdir(outputDir, { recursive: true })
		await fs.writeFile(path.join(outputDir, "chapter_0001.html"), "ex", "utf8")

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValue(new Response(chapterHtml(["D"]), { status: 200 })),
		)

		const result = await crawlChapter({
			baseUrl: "https://example.com/book",
			chapterNumber: 1,
			outputRoot: tempDir,
			ifExists: ExistingFilePolicy.ASK,
			existingPolicy: ExistingFilePolicy.ASK,
			delay: 0,
		})

		expect(result.status).toBe("skip_all")
		expect(mockQuestion).toHaveBeenCalledTimes(2)
	})

	it("fails when no chapter content can be extracted", async () => {
		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-crawl-"))
		tempDirs.push(tempDir)
		const { crawlChapter, ExistingFilePolicy } = await import("../crawler.js")

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValue(
					new Response(
						`<html><body><div class="chapter-c"></div><h1>Ten Truyen</h1><h2>Chuong 1</h2></body></html>`,
						{ status: 200 },
					),
				),
		)

		await expect(
			crawlChapter({
				baseUrl: "https://example.com/book",
				chapterNumber: 1,
				outputRoot: tempDir,
				ifExists: ExistingFilePolicy.OVERWRITE,
				existingPolicy: ExistingFilePolicy.ASK,
				delay: 0,
			}),
		).rejects.toThrow(
			"No chapter content extracted from https://example.com/book/chuong-1/",
		)
	})

	it("fails latest chapter discovery when the latest section is missing", async () => {
		const { discoverLastChapterNumber } = await import("../crawler.js")
		vi.stubGlobal(
			"fetch",
			vi.fn().mockResolvedValue(
				new Response("<html><body><h3>Khac</h3></body></html>", {
					status: 200,
				}),
			),
		)

		await expect(
			discoverLastChapterNumber("https://example.com/book"),
		).rejects.toThrow("Could not find the 'Chương Mới Nhất' section.")
	})
})
