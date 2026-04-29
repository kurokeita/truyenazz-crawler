import { describe, expect, it, vi } from "vitest"
import {
	discoverLastChapterNumber,
	extractFullChapterText,
} from "../crawler.js"

describe("extractFullChapterText", () => {
	it("extracts readable chapter content and removes duplicate noise", () => {
		const html = `<!DOCTYPE html>
<html>
	<head>
		<title>Fallback title - truyenazz</title>
	</head>
	<body>
		<div class="rv-full-story-title"><h1>Ten Truyen</h1></div>
		<div class="rv-chapt-title"><h2>Chuong 12: Thu nghiem</h2></div>
		<div class="chapter-c">
			<p>Doan mo dau</p>
			<p>Doan mo dau</p>
			<div id="data-content-truyen-backup"></div>
		</div>
		<script>
			var contentS = '<p>Ban dang doc truyen moi tai spam</p><p>Dong hop le</p><p>Dong hop le</p>';
			div.innerHTML = contentS;
		</script>
	</body>
</html>`

		const chapter = extractFullChapterText(html)

		expect(chapter).toEqual({
			novelTitle: "Ten Truyen",
			chapterTitle: "Chuong 12: Thu nghiem",
			paragraphs: ["Doan mo dau", "Dong hop le"],
		})
	})

	it("falls back to generic heading selectors and extracts content from attributes", () => {
		const html = `<!DOCTYPE html>
<html>
	<head><title>Fallback title - truyenazz</title></head>
	<body>
		<h2>Fallback chapter</h2>
		<div class="chapter-c">
			<span title="Noi dung trong attribute"></span>
		</div>
	</body>
</html>`

		const chapter = extractFullChapterText(html)

		expect(chapter).toEqual({
			novelTitle: "Unknown Novel",
			chapterTitle: "Fallback chapter",
			paragraphs: ["Noi dung trong attribute"],
		})
	})

	it("discovers the latest available chapter number from the main page", async () => {
		vi.stubGlobal(
			"fetch",
			vi.fn().mockResolvedValue(
				new Response(
					`<!DOCTYPE html>
<html>
	<body>
		<div>
			<h3>Chương Mới Nhất</h3>
		</div>
		<div>
			<ul>
				<li><a href="/ten-truyen/chuong-41/">Chuong 41</a></li>
				<li><a href="/ten-truyen/chuong-42/">Chuong 42</a></li>
			</ul>
		</div>
	</body>
</html>`,
					{ status: 200 },
				),
			),
		)

		await expect(
			discoverLastChapterNumber("https://example.com/ten-truyen"),
		).resolves.toBe(42)
	})

	it("fails latest chapter discovery when headings or containers are missing", async () => {
		vi.stubGlobal(
			"fetch",
			vi.fn().mockResolvedValueOnce(new Response("<html><body></body></html>")),
		)
		await expect(
			discoverLastChapterNumber("https://example.com/ten-truyen"),
		).rejects.toThrow("Could not find the 'Chương Mới Nhất' section")

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValueOnce(
					new Response("<html><body><h3>Chương Mới Nhất</h3></body></html>"),
				),
		)
		await expect(
			discoverLastChapterNumber("https://example.com/ten-truyen"),
		).rejects.toThrow("Could not find the container for 'Chương Mới Nhất'")

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValueOnce(
					new Response(
						"<html><body><div><h3>Chương Mới Nhất</h3></div></body></html>",
					),
				),
		)
		await expect(
			discoverLastChapterNumber("https://example.com/ten-truyen"),
		).rejects.toThrow(
			"Could not find the chapter list next to 'Chương Mới Nhất'",
		)

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValueOnce(
					new Response(
						"<html><body><div><h3>Chương Mới Nhất</h3></div><div><ul></ul></div></body></html>",
					),
				),
		)
		await expect(
			discoverLastChapterNumber("https://example.com/ten-truyen"),
		).rejects.toThrow("Could not find any latest chapter entries")

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValueOnce(
					new Response(
						`<html><body><div><h3>Chương Mới Nhất</h3></div><div><ul><li>Chuong 1</li></ul></div></body></html>`,
					),
				),
		)
		await expect(
			discoverLastChapterNumber("https://example.com/ten-truyen"),
		).rejects.toThrow("Could not find a link for the last chapter entry")

		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValueOnce(
					new Response(
						`<html><body><div><h3>Chương Mới Nhất</h3></div><div><ul><li><a href="/bad">Chuong 1</a></li></ul></div></body></html>`,
					),
				),
		)
		await expect(
			discoverLastChapterNumber("https://example.com/ten-truyen"),
		).rejects.toThrow("Could not extract the last chapter number")
	})

	it("throws when .chapter-c is missing and handles malformed script content", () => {
		expect(() => extractFullChapterText("<html></html>")).toThrow(
			"Could not find .chapter-c in the HTML",
		)

		const html = `
			<div class="chapter-c">
				<p></p> <!-- empty element to test no content -->
				<span></span> <!-- empty span to test no content -->
				No text here
			</div>
			<script>var contentS = 'malformed; div.innerHTML = contentS;</script>
		`
		const chapter = extractFullChapterText(html)
		expect(chapter.paragraphs).toEqual([])
	})

	it("handles multiple headings in discoverLastChapterNumber and skips if heading is found", async () => {
		vi.stubGlobal(
			"fetch",
			vi.fn().mockResolvedValue(
				new Response(
					`<html>
						<body>
							<h3>Random heading</h3>
							<div><h3>${"Chương Mới Nhất".normalize("NFC")}</h3></div>
							<div><ul><li><a href="/ten-truyen/chuong-10/">10</a></li></ul></div>
							<h3>Extra heading to trigger coverage</h3>
						</body>
					</html>`,
					{ status: 200 },
				),
			),
		)

		await expect(
			discoverLastChapterNumber("https://example.com/ten-truyen"),
		).resolves.toBe(10)
	})
})
