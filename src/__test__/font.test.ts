import { promises as fs } from "node:fs"
import os from "node:os"
import path from "node:path"
import { afterEach, describe, expect, it } from "vitest"
import { extractFontMetadata } from "../font.js"

const tempDirs: string[] = []

afterEach(async () => {
	await Promise.all(
		tempDirs
			.splice(0)
			.map((dir) => fs.rm(dir, { recursive: true, force: true })),
	)
})

function utf16be(value: string): Buffer {
	const bytes: number[] = []
	for (const char of value) {
		const code = char.charCodeAt(0)
		bytes.push((code >> 8) & 0xff, code & 0xff)
	}
	return Buffer.from(bytes)
}

function createNameTableFont(familyName: string): Buffer {
	const stringData = utf16be(familyName)
	const nameTable = Buffer.alloc(18 + stringData.length)
	nameTable.writeUInt16BE(0, 0)
	nameTable.writeUInt16BE(1, 2)
	nameTable.writeUInt16BE(18, 4)
	nameTable.writeUInt16BE(3, 6)
	nameTable.writeUInt16BE(1, 8)
	nameTable.writeUInt16BE(0x0409, 10)
	nameTable.writeUInt16BE(1, 12)
	nameTable.writeUInt16BE(stringData.length, 14)
	nameTable.writeUInt16BE(0, 16)
	stringData.copy(nameTable, 18)

	const offset = 28
	const buffer = Buffer.alloc(offset + nameTable.length)
	buffer.writeUInt32BE(0x00010000, 0)
	buffer.writeUInt16BE(1, 4)
	buffer.write("name", 12, 4, "ascii")
	buffer.writeUInt32BE(offset, 20)
	buffer.writeUInt32BE(nameTable.length, 24)
	nameTable.copy(buffer, offset)
	return buffer
}

function createAsciiNameTableFont(familyName: string): Buffer {
	const stringData = Buffer.from(familyName, "latin1")
	const nameTable = Buffer.alloc(18 + stringData.length)
	nameTable.writeUInt16BE(1, 0)
	nameTable.writeUInt16BE(1, 2)
	nameTable.writeUInt16BE(18, 4)
	nameTable.writeUInt16BE(1, 6)
	nameTable.writeUInt16BE(0, 8)
	nameTable.writeUInt16BE(0, 10)
	nameTable.writeUInt16BE(1, 12)
	nameTable.writeUInt16BE(stringData.length, 14)
	nameTable.writeUInt16BE(0, 16)
	stringData.copy(nameTable, 18)

	const offset = 28
	const buffer = Buffer.alloc(offset + nameTable.length)
	buffer.writeUInt32BE(0x00010000, 0)
	buffer.writeUInt16BE(1, 4)
	buffer.write("name", 12, 4, "ascii")
	buffer.writeUInt32BE(offset, 20)
	buffer.writeUInt32BE(nameTable.length, 24)
	nameTable.copy(buffer, offset)
	return buffer
}

describe("extractFontMetadata", () => {
	it("reads the preferred family name from the font name table", async () => {
		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-font-"))
		tempDirs.push(tempDir)

		const fontPath = path.join(tempDir, "sample.ttf")
		await fs.writeFile(fontPath, createNameTableFont("Test Family"))

		await expect(extractFontMetadata(fontPath)).resolves.toEqual({
			familyName: "Test Family",
			extension: ".ttf",
		})
	})

	it("falls back to the file name when no valid name table is available", async () => {
		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-font-"))
		tempDirs.push(tempDir)

		const fontPath = path.join(tempDir, "fallback.otf")
		const buffer = Buffer.alloc(32)
		buffer.writeUInt32BE(0x00010000, 0)
		buffer.writeUInt16BE(0, 4)
		await fs.writeFile(fontPath, buffer)

		await expect(extractFontMetadata(fontPath)).resolves.toEqual({
			familyName: "fallback",
			extension: ".otf",
		})
	})

	it("supports non-unicode name records, invalid name table formats, and name ID fallbacks", async () => {
		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-font-"))
		tempDirs.push(tempDir)

		const asciiFontPath = path.join(tempDir, "ascii.ttf")
		await fs.writeFile(asciiFontPath, createAsciiNameTableFont("Ascii Family"))

		await expect(extractFontMetadata(asciiFontPath)).resolves.toEqual({
			familyName: "Ascii Family",
			extension: ".ttf",
		})

		const invalidFormatPath = path.join(tempDir, "invalid-format.ttf")
		const buffer = createNameTableFont("Ignored")
		buffer.writeUInt16BE(2, 28)
		await fs.writeFile(invalidFormatPath, buffer)

		await expect(extractFontMetadata(invalidFormatPath)).resolves.toEqual({
			familyName: "invalid-format",
			extension: ".ttf",
		})

		const fullNameOnlyPath = path.join(tempDir, "full-name.ttf")
		const fullNameBuffer = createNameTableFont("Full Name")
		fullNameBuffer.writeUInt16BE(4, 28 + 12) // Change nameId to 4
		await fs.writeFile(fullNameOnlyPath, fullNameBuffer)

		await expect(extractFontMetadata(fullNameOnlyPath)).resolves.toEqual({
			familyName: "Full Name",
			extension: ".ttf",
		})
	})

	it("falls back when the name table record is truncated or contains no usable names", async () => {
		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-font-"))
		tempDirs.push(tempDir)

		const truncatedPath = path.join(tempDir, "truncated.ttf")
		const truncated = Buffer.alloc(20)
		truncated.writeUInt32BE(0x00010000, 0)
		truncated.writeUInt16BE(1, 4)
		truncated.write("name", 12, 4, "ascii")
		await fs.writeFile(truncatedPath, truncated)

		await expect(extractFontMetadata(truncatedPath)).resolves.toEqual({
			familyName: "truncated",
			extension: ".ttf",
		})

		const nameTableMismatchPath = path.join(tempDir, "mismatch.ttf")
		const mismatchBuffer = createNameTableFont("Mismatch")
		mismatchBuffer.writeUInt32BE(1000, 20) // Invalid offset
		await fs.writeFile(nameTableMismatchPath, mismatchBuffer)

		await expect(extractFontMetadata(nameTableMismatchPath)).resolves.toEqual({
			familyName: "mismatch",
			extension: ".ttf",
		})

		const storageMismatchPath = path.join(tempDir, "storage-mismatch.ttf")
		const storageBuffer = createNameTableFont("Storage")
		storageBuffer.writeUInt16BE(0, 28 + 16) // Invalid storage offset
		storageBuffer.writeUInt16BE(1000, 28 + 14) // Too long
		await fs.writeFile(storageMismatchPath, storageBuffer)

		await expect(extractFontMetadata(storageMismatchPath)).resolves.toEqual({
			familyName: "storage-mismatch",
			extension: ".ttf",
		})

		const emptyNamePath = path.join(tempDir, "empty-name.ttf")
		const buffer = createNameTableFont("")
		await fs.writeFile(emptyNamePath, buffer)

		await expect(extractFontMetadata(emptyNamePath)).resolves.toEqual({
			familyName: "empty-name",
			extension: ".ttf",
		})

		const zeroLengthPath = path.join(tempDir, "zero-length.ttf")
		const zeroLengthBuffer = createNameTableFont("Zero")
		zeroLengthBuffer.writeUInt16BE(0, 28 + 14) // length 0
		await fs.writeFile(zeroLengthPath, zeroLengthBuffer)
		await expect(extractFontMetadata(zeroLengthPath)).resolves.toEqual({
			familyName: "zero-length",
			extension: ".ttf",
		})

		const shortUtf16Path = path.join(tempDir, "short-utf16.ttf")
		const shortBuffer = createNameTableFont("A")
		shortBuffer.writeUInt16BE(1, 28 + 14) // length 1
		await fs.writeFile(shortUtf16Path, shortBuffer)
		await expect(extractFontMetadata(shortUtf16Path)).resolves.toEqual({
			familyName: "short-utf16",
			extension: ".ttf",
		})

		const invalidStoragePath = path.join(tempDir, "invalid-storage.ttf")
		const invalidStorageBuffer = createNameTableFont("Invalid")
		invalidStorageBuffer.writeUInt16BE(1000, 34 + 10) // offset very large, record offset + 10
		await fs.writeFile(invalidStoragePath, invalidStorageBuffer)
		await expect(extractFontMetadata(invalidStoragePath)).resolves.toEqual({
			familyName: "invalid-storage",
			extension: ".ttf",
		})
	})

	it("rejects obviously invalid font files", async () => {
		const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "truyenazz-font-"))
		tempDirs.push(tempDir)

		const fontPath = path.join(tempDir, "broken.ttf")
		await fs.writeFile(fontPath, Buffer.alloc(4))

		await expect(extractFontMetadata(fontPath)).rejects.toThrow(
			`Invalid font file: ${path.resolve(fontPath)}`,
		)
	})
})
