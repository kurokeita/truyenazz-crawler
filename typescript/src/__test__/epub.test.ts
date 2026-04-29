import { promises as fs } from "node:fs"
import os from "node:os"
import path from "node:path"
import JSZip from "jszip"
import { afterEach, describe, expect, it, vi } from "vitest"
import { buildEpub } from "../epub.js"

const tempDirs: string[] = []

afterEach(async () => {
	vi.unstubAllGlobals()

	await Promise.all(
		tempDirs
			.splice(0)
			.map((dir) => fs.rm(dir, { recursive: true, force: true })),
	)
})

describe("buildEpub", () => {
	it("builds an epub from saved chapters and includes navigation metadata", async () => {
		const chapterDir = await fs.mkdtemp(
			path.join(os.tmpdir(), "truyenazz-crawler-epub-"),
		)
		tempDirs.push(chapterDir)

		await fs.writeFile(
			path.join(chapterDir, "chapter_0001.html"),
			`<!DOCTYPE html>
<html>
	<body>
		<h1 class="chapter-title">Chuong 1</h1>
		<div class="chapter-content"><p>Mo dau</p></div>
	</body>
</html>`,
			"utf8",
		)
		await fs.writeFile(
			path.join(chapterDir, "chapter_0002.html"),
			`<!DOCTYPE html>
<html>
	<body>
		<h1 class="chapter-title">Chuong 2</h1>
		<div class="chapter-content"><p>Tiep theo</p></div>
	</body>
</html>`,
			"utf8",
		)

		vi.stubGlobal(
			"fetch",
			vi.fn().mockImplementation(() =>
				Promise.resolve(
					new Response(
						`<!DOCTYPE html>
<html>
	<head><title>Ten Truyen - truyenazz</title></head>
	<body>
		<h1>Ten Truyen</h1>
		<div>Tác giả: Tac Gia</div>
	</body>
</html>`,
						{ status: 200 },
					),
				),
			),
		)

		const outputEpub = path.join(chapterDir, "ten_truyen.epub")
		const epubPath = await buildEpub({
			novelMainUrl: "https://example.com/ten-truyen/",
			chapterDir,
			outputEpub,
			fontPath: path.resolve("Bokerlam.ttf"),
		})

		expect(epubPath).toBe(outputEpub)

		const archive = await JSZip.loadAsync(await fs.readFile(epubPath))
		expect(await archive.file("mimetype")?.async("string")).toBe(
			"application/epub+zip",
		)
		expect(await archive.file("EPUB/nav.xhtml")?.async("string")).toContain(
			"Chuong 1",
		)
		expect(await archive.file("EPUB/nav.xhtml")?.async("string")).toContain(
			"Chuong 2",
		)
		expect(await archive.file("EPUB/content.opf")?.async("string")).toContain(
			"<dc:creator>Tac Gia</dc:creator>",
		)
		expect(
			await archive.file("EPUB/text/titlepage.xhtml")?.async("string"),
		).toContain("Ten Truyen")
		expect(archive.file("EPUB/fonts/epub-font.ttf")).toBeTruthy()

		// Test default outputEpub path
		const epubPathDefault = await buildEpub({
			novelMainUrl: "https://example.com/ten-truyen/",
			chapterDir,
			fontPath: path.resolve("Bokerlam.ttf"),
		})
		expect(epubPathDefault).toBe(path.join(chapterDir, "ten_truyen.epub"))
	})

	it("falls back to the page title and includes a downloaded cover image", async () => {
		const chapterDir = await fs.mkdtemp(
			path.join(os.tmpdir(), "truyenazz-crawler-cover-"),
		)
		tempDirs.push(chapterDir)

		await fs.writeFile(
			path.join(chapterDir, "chapter_0001.html"),
			`<!DOCTYPE html>
<html>
	<body>
		<h1>Chuong fallback</h1>
		<div class="chapter-content"><p>Mo dau</p></div>
	</body>
</html>`,
			"utf8",
		)

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValueOnce(
					new Response(
						`<!DOCTYPE html>
<html>
	<head><title>Fallback Title - truyenazz</title></head>
	<body><img src="/cover.png" /></body>
</html>`,
						{ status: 200 },
					),
				)
				.mockResolvedValueOnce(
					new Response(new Uint8Array([1, 2, 3]), {
						status: 200,
						headers: { "content-type": "image/png" },
					}),
				),
		)

		const outputEpub = path.join(chapterDir, "with-cover.epub")
		await buildEpub({
			novelMainUrl: "https://example.com/ten-truyen/",
			chapterDir,
			outputEpub,
			fontPath: path.resolve("Bokerlam.ttf"),
		})

		const archive = await JSZip.loadAsync(await fs.readFile(outputEpub))
		expect(await archive.file("EPUB/content.opf")?.async("string")).toContain(
			'media-type="image/png"',
		)
		expect(archive.file("EPUB/cover.png")).toBeTruthy()
		expect(
			await archive.file("EPUB/text/titlepage.xhtml")?.async("string"),
		).toContain("Fallback Title")
	})

	it("rejects when the chapter directory does not exist", async () => {
		await expect(
			buildEpub({
				novelMainUrl: "https://example.com/ten-truyen/",
				chapterDir: "/missing/chapter-dir",
			}),
		).rejects.toThrow("Chapter directory not found:")
	})

	it("rejects when no saved chapter html files are present", async () => {
		const chapterDir = await fs.mkdtemp(
			path.join(os.tmpdir(), "truyenazz-crawler-empty-"),
		)
		tempDirs.push(chapterDir)

		await fs.writeFile(path.join(chapterDir, "ignore.txt"), "noop", "utf8")
		vi.stubGlobal(
			"fetch",
			vi.fn().mockImplementation(() =>
				Promise.resolve(
					new Response("<html><body><h1>Ten Truyen</h1></body></html>", {
						status: 200,
					}),
				),
			),
		)

		await expect(
			buildEpub({
				novelMainUrl: "https://example.com/ten-truyen/",
				chapterDir,
				fontPath: path.resolve("Bokerlam.ttf"),
			}),
		).rejects.toThrow(`No chapter_*.html files found in ${chapterDir}`)
	})

	it("falls back when cover download fails and no font is found", async () => {
		const chapterDir = await fs.mkdtemp(
			path.join(os.tmpdir(), "truyenazz-crawler-fallback-"),
		)
		tempDirs.push(chapterDir)

		await fs.writeFile(
			path.join(chapterDir, "chapter_0001.html"),
			`<!DOCTYPE html>
<html>
	<body>
		<h1 class="chapter-title">Chuong 1</h1>
		<div class="chapter-content"><p>Mo dau</p></div>
	</body>
</html>`,
			"utf8",
		)

		const warning = vi
			.spyOn(console, "warn")
			.mockImplementation(() => undefined)
		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValueOnce(
					new Response(
						`<!DOCTYPE html>
<html>
	<body>
		<h1>Ten Truyen</h1>
		<img src="https://example.com/cover.jpg" />
	</body>
</html>`,
						{ status: 200 },
					),
				)
				.mockResolvedValueOnce(new Response("missing", { status: 404 })),
		)

		vi.resetModules()
		vi.doMock("../utils.js", async () => {
			const actual =
				await vi.importActual<typeof import("../utils.js")>("../utils.js")
			return {
				...actual,
				findFontFile: vi.fn().mockResolvedValue(null),
			}
		})
		const { buildEpub: buildEpubWithoutFont } = await import("../epub.js")

		const epubPath = await buildEpubWithoutFont({
			novelMainUrl: "https://example.com/ten-truyen/",
			chapterDir,
			outputEpub: path.join(chapterDir, "fallback.epub"),
		})

		expect(epubPath).toBe(path.join(chapterDir, "fallback.epub"))
		expect(warning).toHaveBeenCalledWith(
			"[WARN] No EPUB font found, fallback to serif",
		)
	})

	it("rejects malformed saved chapter files", async () => {
		const chapterDir = await fs.mkdtemp(
			path.join(os.tmpdir(), "truyenazz-crawler-malformed-"),
		)
		tempDirs.push(chapterDir)

		await fs.writeFile(
			path.join(chapterDir, "chapter_0001.html"),
			'<html><body><div class="chapter-content"></div></body></html>',
			"utf8",
		)
		vi.stubGlobal(
			"fetch",
			vi.fn().mockImplementation(() =>
				Promise.resolve(
					new Response("<html><body><h1>Ten Truyen</h1></body></html>", {
						status: 200,
					}),
				),
			),
		)

		await expect(
			buildEpub({
				novelMainUrl: "https://example.com/ten-truyen/",
				chapterDir,
				fontPath: path.resolve("Bokerlam.ttf"),
			}),
		).rejects.toThrow("Missing .chapter-title or .chapter-content")
	})

	it("rejects empty saved chapter bodies", async () => {
		const chapterDir = await fs.mkdtemp(
			path.join(os.tmpdir(), "truyenazz-crawler-empty-body-"),
		)
		tempDirs.push(chapterDir)

		await fs.writeFile(
			path.join(chapterDir, "chapter_0001.html"),
			`<html><body><h1 class="chapter-title">Chuong 1</h1><div class="chapter-content"></div></body></html>`,
			"utf8",
		)
		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockImplementation(() =>
					Promise.resolve(new Response("<html></html>", { status: 200 })),
				),
		)

		await expect(
			buildEpub({
				novelMainUrl: "https://example.com/ten-truyen/",
				chapterDir,
				fontPath: path.resolve("Bokerlam.ttf"),
			}),
		).rejects.toThrow("Empty .chapter-content")
	})

	it("handles missing title, author, and complex cover image scenarios", async () => {
		const chapterDir = await fs.mkdtemp(
			path.join(os.tmpdir(), "truyenazz-crawler-complex-"),
		)
		tempDirs.push(chapterDir)

		await fs.writeFile(
			path.join(chapterDir, "chapter_0001.html"),
			'<html><body><h1>Chuong 1</h1><div class="chapter-content"><p>Ok</p></div></body></html>',
			"utf8",
		)

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValueOnce(
					new Response(
						`<html>
							<body>
								<img src="data:image/png;base64,noop" />
								<img class="lazyload" data-lazy-src="https://example.com/complex-cover" />
							</body>
						</html>`,
						{ status: 200 },
					),
				)
				.mockResolvedValueOnce(
					new Response(new Uint8Array([1]), {
						status: 200,
						headers: { "content-type": "application/octet-stream" },
					}),
				),
		)

		const epubPath = await buildEpub({
			novelMainUrl: "https://example.com/complex/",
			chapterDir,
		})

		const archive = await JSZip.loadAsync(await fs.readFile(epubPath))
		expect(await archive.file("EPUB/content.opf")?.async("string")).toContain(
			"Unknown Novel",
		)
		expect(
			await archive.file("EPUB/content.opf")?.async("string"),
		).not.toContain("<dc:creator>")
		expect(
			archive.file("EPUB/cover.bin") || archive.file("EPUB/cover.jpg"),
		).toBeTruthy()

		// Test case for no cover found
		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockImplementation(() =>
					Promise.resolve(new Response("<html><body></body></html>")),
				),
		)
		const epubPathNoCover = await buildEpub({
			novelMainUrl: "https://example.com/no-cover/",
			chapterDir,
			outputEpub: path.join(chapterDir, "no-cover.epub"),
		})
		const archiveNoCover = await JSZip.loadAsync(
			await fs.readFile(epubPathNoCover),
		)
		expect(archiveNoCover.file("EPUB/cover.jpg")).toBeFalsy()

		// Test case for missing extension in cover URL
		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValueOnce(
					new Response(
						`<html><body><img src="https://example.com/cover-no-ext" /></body></html>`,
					),
				)
				.mockResolvedValueOnce(
					new Response(new Uint8Array([1]), {
						status: 200,
						headers: { "content-type": "invalid/type" },
					}),
				),
		)
		const epubPathNoExt = await buildEpub({
			novelMainUrl: "https://example.com/no-ext/",
			chapterDir,
			outputEpub: path.join(chapterDir, "no-ext.epub"),
		})
		const archiveNoExt = await JSZip.loadAsync(await fs.readFile(epubPathNoExt))
		expect(archiveNoExt.file("EPUB/cover.jpg")).toBeTruthy()
	})

	it("throws when ZIP folder creation fails", async () => {
		const chapterDir = await fs.mkdtemp(
			path.join(os.tmpdir(), "truyenazz-crawler-zip-fail-"),
		)
		tempDirs.push(chapterDir)
		await fs.writeFile(
			path.join(chapterDir, "chapter_0001.html"),
			"<html><body><h1>C1</h1><div class='chapter-content'><p>P</p></div></body></html>",
		)

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockImplementation(() =>
					Promise.resolve(new Response("<html></html>", { status: 200 })),
				),
		)

		vi.resetModules()
		vi.doMock("jszip", () => {
			return {
				default: class {
					file = vi.fn()
					folder = vi.fn().mockReturnValue(null)
				},
			}
		})

		const { buildEpub: buildEpubFailingZip } = await import("../epub.js")
		await expect(
			buildEpubFailingZip({
				novelMainUrl: "https://example.com/fail/",
				chapterDir,
			}),
		).rejects.toThrow("Could not initialize EPUB archive.")
	})
})
