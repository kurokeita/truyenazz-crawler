import { promises as fs } from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { decode } from "html-entities"

const currentDir = path.dirname(fileURLToPath(import.meta.url))

export const HEADERS = {
	"User-Agent":
		"Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 " +
		"(KHTML, like Gecko) Chrome/123.0 Safari/537.36",
}

export const NON_CONTENT_ATTRS = new Set([
	"class",
	"style",
	"id",
	"onmousedown",
	"onselectstart",
	"oncopy",
	"oncut",
])

export const NOISE_PREFIXES = [
	"Bạn đang đọc truyện mới tại",
	"Nhấn Mở Bình Luận",
	"Tham gia group Facebook",
	"Các bạn thông cảm vì website có hiện quảng cáo",
	"Website hoạt động dưới Giấy phép",
]

function normalizeForPrefixMatch(text: string): string {
	return text
		.normalize("NFKD")
		.replace(/[\u0300-\u036f]/g, "")
		.replace(/[đĐ]/g, "d")
		.toLowerCase()
		.replace(/\s+/g, " ")
		.trim()
}

export function cleanText(text: string): string {
	return decode(text ?? "")
		.replace(/\u00a0/g, " ")
		.replace(/\s+/g, " ")
		.trim()
}

export function isNoise(text: string): boolean {
	if (!text) {
		return true
	}
	const normalizedText = normalizeForPrefixMatch(text)
	return NOISE_PREFIXES.some((prefix) =>
		normalizedText.startsWith(normalizeForPrefixMatch(prefix)),
	)
}

export function slugify(text: string, fallback = "novel"): string {
	const normalized = text
		.normalize("NFKD")
		.replace(/[\u0300-\u036f]/g, "")
		.replace(/[^\p{ASCII}]/gu, "")
		.toLowerCase()
		.trim()
		.replace(/[^\w\s-]/g, "")
		.replace(/[-\s]+/g, "_")
	return normalized.slice(0, 120) || fallback
}

export async function fetchHtml(
	url: string,
	timeout = 30_000,
): Promise<string> {
	const controller = new AbortController()
	const timer = setTimeout(() => controller.abort(), timeout)

	try {
		const response = await fetch(url, {
			headers: HEADERS,
			signal: controller.signal,
		})

		if (!response.ok) {
			throw new Error(`HTTP ${response.status} while fetching ${url}`)
		}

		return await response.text()
	} finally {
		clearTimeout(timer)
	}
}

export async function downloadBinary(
	url: string,
	timeout = 30_000,
): Promise<{ content: Buffer; contentType: string }> {
	const controller = new AbortController()
	const timer = setTimeout(() => controller.abort(), timeout)

	try {
		const response = await fetch(url, {
			headers: HEADERS,
			signal: controller.signal,
		})

		if (!response.ok) {
			throw new Error(`HTTP ${response.status} while fetching ${url}`)
		}

		const arrayBuffer = await response.arrayBuffer()
		const contentType =
			response.headers.get("content-type")?.split(";")[0]?.trim() ?? ""

		return {
			content: Buffer.from(arrayBuffer),
			contentType,
		}
	} finally {
		clearTimeout(timer)
	}
}

export async function sleep(seconds: number): Promise<void> {
	if (seconds <= 0) {
		return
	}
	await new Promise((resolve) => setTimeout(resolve, seconds * 1000))
}

export async function ensureDir(dirPath: string): Promise<void> {
	await fs.mkdir(dirPath, { recursive: true })
}

export async function fileExists(filePath: string): Promise<boolean> {
	try {
		await fs.access(filePath)
		return true
	} catch {
		return false
	}
}

export async function findFontFile(
	explicitFontPath?: string,
): Promise<string | null> {
	if (explicitFontPath) {
		const resolvedFontPath = path.resolve(explicitFontPath)
		if (await fileExists(resolvedFontPath)) {
			return resolvedFontPath
		}

		throw new Error(`Font file not found: ${resolvedFontPath}`)
	}

	const candidates = [
		path.resolve(currentDir, "Bokerlam.ttf"),
		path.resolve(process.cwd(), "Bokerlam.ttf"),
		path.resolve(currentDir, "..", "Bokerlam.ttf"),
		path.resolve(currentDir, "..", "..", "Bokerlam.ttf"),
		path.resolve(currentDir, "..", "..", "..", "Bokerlam.ttf"),
	]

	for (const candidate of candidates) {
		if (await fileExists(candidate)) {
			return candidate
		}
	}

	return null
}
