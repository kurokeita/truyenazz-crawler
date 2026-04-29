import { load } from "cheerio"
import { cleanText, fetchHtml, slugify } from "./utils.js"

export async function fetchMainHtmlForCli(url: string): Promise<string> {
	return fetchHtml(url)
}

export function extractNovelTitleFromMainPageForCli(
	htmlSource: string,
): string {
	const $ = load(htmlSource)

	const h1 = cleanText($("h1").first().text())
	if (h1) {
		return h1
	}

	const title = cleanText($("title").first().text())
	if (title) {
		return title.replace(/\s*-\s*truyenazz\s*$/i, "")
	}

	return "Unknown Novel"
}

export function slugifyForCli(text: string, fallback = "book"): string {
	return slugify(text, fallback)
}
