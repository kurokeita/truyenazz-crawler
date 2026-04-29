import { promises as fs } from "node:fs"
import path from "node:path"

interface NameRecord {
	platformId: number
	languageId: number
	nameId: number
	value: string
}

function decodeUtf16Be(buffer: Buffer): string {
	if (buffer.length < 2) {
		return ""
	}

	const codeUnits: number[] = []
	for (let index = 0; index + 1 < buffer.length; index += 2) {
		codeUnits.push(buffer.readUInt16BE(index))
	}

	return String.fromCharCode(...codeUnits)
		.replace(/\0/g, "")
		.trim()
}

function decodeAscii(buffer: Buffer): string {
	return buffer.toString("latin1").replace(/\0/g, "").trim()
}

function pickBestName(records: NameRecord[], nameId: number): string | null {
	const candidates = records.filter(
		(record) => record.nameId === nameId && record.value,
	)
	if (candidates.length === 0) {
		return null
	}

	const preferred =
		candidates.find(
			(record) => record.platformId === 3 && record.languageId === 0x0409,
		) ??
		candidates.find((record) => record.platformId === 0) ??
		candidates.find((record) => record.platformId === 3) ??
		candidates[0]

	return preferred.value || null
}

export async function extractFontMetadata(fontPath: string): Promise<{
	familyName: string
	extension: string
}> {
	const resolvedPath = path.resolve(fontPath)
	const buffer = await fs.readFile(resolvedPath)

	if (buffer.length < 12) {
		throw new Error(`Invalid font file: ${resolvedPath}`)
	}

	const numTables = buffer.readUInt16BE(4)
	let nameTableOffset = -1
	let nameTableLength = 0

	for (let index = 0; index < numTables; index += 1) {
		const recordOffset = 12 + index * 16
		if (recordOffset + 16 > buffer.length) {
			break
		}

		const tag = buffer.toString("ascii", recordOffset, recordOffset + 4)
		if (tag !== "name") {
			continue
		}

		nameTableOffset = buffer.readUInt32BE(recordOffset + 8)
		nameTableLength = buffer.readUInt32BE(recordOffset + 12)
		break
	}

	if (
		nameTableOffset < 0 ||
		nameTableOffset + nameTableLength > buffer.length
	) {
		return {
			familyName: path.parse(resolvedPath).name,
			extension: path.extname(resolvedPath).toLowerCase() || ".ttf",
		}
	}

	const format = buffer.readUInt16BE(nameTableOffset)
	if (format !== 0 && format !== 1) {
		return {
			familyName: path.parse(resolvedPath).name,
			extension: path.extname(resolvedPath).toLowerCase() || ".ttf",
		}
	}

	const count = buffer.readUInt16BE(nameTableOffset + 2)
	const stringOffset = buffer.readUInt16BE(nameTableOffset + 4)
	const storageBase = nameTableOffset + stringOffset
	const records: NameRecord[] = []

	for (let index = 0; index < count; index += 1) {
		const recordOffset = nameTableOffset + 6 + index * 12
		if (recordOffset + 12 > buffer.length) {
			break
		}

		const platformId = buffer.readUInt16BE(recordOffset)
		const languageId = buffer.readUInt16BE(recordOffset + 4)
		const nameId = buffer.readUInt16BE(recordOffset + 6)
		const length = buffer.readUInt16BE(recordOffset + 8)
		const offset = buffer.readUInt16BE(recordOffset + 10)
		const start = storageBase + offset
		const end = start + length

		if (start < 0) {
			continue
		}
		if (end > buffer.length) {
			continue
		}
		if (length <= 0) {
			continue
		}

		const rawValue = buffer.subarray(start, end)
		const value =
			platformId === 0 || platformId === 3
				? decodeUtf16Be(rawValue)
				: decodeAscii(rawValue)

		if (!value) {
			continue
		}

		records.push({
			platformId,
			languageId,
			nameId,
			value,
		})
	}

	const familyName =
		pickBestName(records, 1) ??
		pickBestName(records, 4) ??
		path.parse(resolvedPath).name

	return {
		familyName,
		extension: path.extname(resolvedPath).toLowerCase() || ".ttf",
	}
}
