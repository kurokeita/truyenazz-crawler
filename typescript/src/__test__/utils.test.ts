import { promises as fs } from "node:fs"
import os from "node:os"
import path from "node:path"
import { afterEach, describe, expect, it, vi } from "vitest"
import {
	cleanText,
	downloadBinary,
	ensureDir,
	fetchHtml,
	fileExists,
	findFontFile,
	isNoise,
	sleep,
	slugify,
} from "../utils.js"

const tempDirs: string[] = []

afterEach(async () => {
	vi.unstubAllGlobals()
	await Promise.all(
		tempDirs.map((dir) => fs.rm(dir, { recursive: true, force: true })),
	)
	tempDirs.length = 0
})

describe("utils", () => {
	it("normalizes text, slugifies names, and matches noise prefixes accent-insensitively", () => {
		expect(cleanText(" A&nbsp;&nbsp;B \n C ")).toBe("A B C")
		expect(cleanText(null as any)).toBe("")
		expect(slugify("Người Chồng Vô Dụng")).toBe("nguoi_chong_vo_dung")
		expect(slugify("###", "fallback")).toBe("fallback")
		expect(isNoise("Ban dang doc truyen moi tai example")).toBe(true)
		expect(isNoise("Noi dung hop le")).toBe(false)
	})

	it("fetches html and binary payloads and reports HTTP errors", async () => {
		vi.stubGlobal(
			"fetch",
			vi
				.fn()
				.mockResolvedValueOnce(new Response("<html>ok</html>", { status: 200 }))
				.mockResolvedValueOnce(
					new Response(new Uint8Array([1, 2, 3]), {
						status: 200,
						headers: { "content-type": "image/png" },
					}),
				)
				.mockResolvedValueOnce(
					new Response(new Uint8Array([4, 5, 6]), {
						status: 200,
						headers: {},
					}),
				)
				.mockResolvedValueOnce(new Response("nope", { status: 404 })),
		)

		await expect(fetchHtml("https://example.com")).resolves.toBe(
			"<html>ok</html>",
		)
		await expect(downloadBinary("https://example.com/image")).resolves.toEqual({
			content: Buffer.from([1, 2, 3]),
			contentType: "image/png",
		})
		await expect(
			downloadBinary("https://example.com/no-type"),
		).resolves.toEqual({
			content: Buffer.from([4, 5, 6]),
			contentType: "",
		})
		await expect(fetchHtml("https://example.com/missing")).rejects.toThrow(
			"HTTP 404 while fetching https://example.com/missing",
		)
	})

	it("creates directories, checks file existence, and finds font files", async () => {
		const rawTempDir = await fs.mkdtemp(
			path.join(os.tmpdir(), "truyenazz-utils-"),
		)
		const tempDir = await fs.realpath(rawTempDir)
		tempDirs.push(tempDir)

		const nestedDir = path.join(tempDir, "nested", "dir")
		await ensureDir(nestedDir)
		expect(await fileExists(nestedDir)).toBe(true)

		const fontFile = path.join(tempDir, "custom.ttf")
		await fs.writeFile(fontFile, "font", "utf8")

		expect(await findFontFile(fontFile)).toBe(path.resolve(fontFile))

		const cwdFont = path.join(tempDir, "Bokerlam.ttf")
		await fs.writeFile(cwdFont, "font", "utf8")

		const originalCwd = process.cwd()
		process.chdir(tempDir)
		try {
			expect(await findFontFile()).toBe(cwdFont)
		} finally {
			process.chdir(originalCwd)
		}
	})

	it("throws for an explicit missing font and sleep resolves immediately for non-positive values", async () => {
		await expect(findFontFile("/missing/font.ttf")).rejects.toThrow(
			"Font file not found:",
		)
		await expect(sleep(0)).resolves.toBeUndefined()
		expect(isNoise("")).toBe(true)
	})

	it("waits for positive sleep durations and returns null when no packaged font exists", async () => {
		vi.useFakeTimers()
		const sleepPromise = sleep(0.01)
		await vi.advanceTimersByTimeAsync(10)
		await expect(sleepPromise).resolves.toBeUndefined()
		vi.useRealTimers()

		vi.resetModules()
		vi.doMock("node:fs", () => ({
			promises: {
				access: vi.fn().mockRejectedValue(new Error("missing")),
				mkdir: vi.fn(),
			},
		}))
		const { findFontFile: findFontFileWithoutCandidates } = await import(
			"../utils.js"
		)
		await expect(findFontFileWithoutCandidates()).resolves.toBeNull()
	})
})
