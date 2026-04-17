import { describe, expect, it, vi } from "vitest"
import {
	extractNovelTitleFromMainPageForCli,
	fetchMainHtmlForCli,
	slugifyForCli,
} from "../internal-cli-helpers.js"

describe("internal CLI helpers", () => {
	it("prefers the h1 title and falls back to the page title", () => {
		expect(
			extractNovelTitleFromMainPageForCli(
				"<html><body><h1>Ten Truyen</h1></body></html>",
			),
		).toBe("Ten Truyen")

		expect(
			extractNovelTitleFromMainPageForCli(
				"<html><head><title>Fallback - truyenazz</title></head></html>",
			),
		).toBe("Fallback")

		expect(extractNovelTitleFromMainPageForCli("<html></html>")).toBe(
			"Unknown Novel",
		)
	})

	it("slugifies titles for CLI output directories", () => {
		expect(slugifyForCli("Người Chồng Vô Dụng")).toBe("nguoi_chong_vo_dung")
		expect(slugifyForCli("###", "book")).toBe("book")
	})

	it("fetches main page html through the shared fetch helper", async () => {
		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValue(new Response("<html>ok</html>", { status: 200 })),
		)

		await expect(
			fetchMainHtmlForCli("https://example.com/novel"),
		).resolves.toBe("<html>ok</html>")
	})
})
