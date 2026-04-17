import { promises as fs } from "node:fs"
import path from "node:path"
import { load } from "cheerio"
import JSZip from "jszip"
import mime from "mime-types"
import { extractFontMetadata } from "./font.js"
import {
	cleanText,
	downloadBinary,
	fetchHtml,
	fileExists,
	findFontFile,
	slugify,
} from "./utils.js"

function escapeXml(text: string): string {
	return text
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;")
		.replace(/'/g, "&apos;")
}

function extractNovelTitleFromMainPage(htmlSource: string): string {
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

function extractAuthorFromMainPage(htmlSource: string): string | null {
	const $ = load(htmlSource)
	const text = $.root().text()
	const match = /Tác giả:\s*([^\n\r]+)/i.exec(text)
	if (!match) {
		return null
	}

	const author = cleanText(match[1])
		.split("Thể loại:")[0]
		?.trim()
		.replace(/[ ,]+$/g, "")
	return author || null
}

function extractCoverImageUrl(
	novelMainUrl: string,
	htmlSource: string,
): string | null {
	const $ = load(htmlSource)
	const selectors = [
		"img.lazyloaded",
		"img.lazyload",
		".book-img img",
		".detail-info img",
		".info-img img",
		"img",
	]

	for (const selector of selectors) {
		for (const image of $(selector).toArray()) {
			const src =
				$(image).attr("src") ??
				$(image).attr("data-src") ??
				$(image).attr("data-original") ??
				$(image).attr("data-lazy-src")

			if (!src || src.startsWith("data:")) {
				continue
			}

			return new URL(src.trim(), novelMainUrl).toString()
		}
	}

	return null
}

function pickCoverExtension(coverUrl: string, mediaType: string): string {
	const extFromType = mime.extension(mediaType)
	if (extFromType) {
		return `.${extFromType}`
	}

	const ext = path.extname(new URL(coverUrl).pathname).toLowerCase()
	return ext || ".jpg"
}

async function listChapterFiles(chapterDir: string): Promise<string[]> {
	const entries = await fs.readdir(chapterDir)
	const files = entries
		.filter((entry) => /^chapter_\d+\.html$/.test(entry))
		.sort()
		.map((entry) => path.join(chapterDir, entry))

	if (files.length === 0) {
		throw new Error(`No chapter_*.html files found in ${chapterDir}`)
	}

	return files
}

async function extractTitleAndBodyFromSavedChapter(
	chapterPath: string,
): Promise<{ title: string; bodyHtml: string }> {
	const raw = await fs.readFile(chapterPath, "utf8")
	const $ = load(raw)

	const titleEl = $(".chapter-title").first().length
		? $(".chapter-title").first()
		: $("h1").first()
	const bodyEl = $(".chapter-content").first()

	if (titleEl.length === 0 || bodyEl.length === 0) {
		throw new Error(
			`Missing .chapter-title or .chapter-content in ${chapterPath}`,
		)
	}

	const title = cleanText(titleEl.text())
	const bodyHtml = bodyEl.html()?.trim() ?? ""

	if (!bodyHtml) {
		throw new Error(`Empty .chapter-content in ${chapterPath}`)
	}

	return { title, bodyHtml }
}

function chapterXhtml(title: string, bodyHtml: string): string {
	return `<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="vi" lang="vi">
  <head>
    <title>${escapeXml(title)}</title>
    <link href="../styles/main.css" rel="stylesheet" type="text/css"/>
  </head>
  <body>
    <h1>${escapeXml(title)}</h1>
    ${bodyHtml}
  </body>
</html>`
}

function titlePageXhtml(title: string, author: string | null): string {
	const authorHtml = author
		? `<p style="text-indent:0;text-align:center;">${escapeXml(author)}</p>`
		: ""

	return `<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="vi" lang="vi">
  <head>
    <title>${escapeXml(title)}</title>
    <link href="../styles/main.css" rel="stylesheet" type="text/css"/>
  </head>
  <body>
    <h1>${escapeXml(title)}</h1>
    ${authorHtml}
  </body>
</html>`
}

function navXhtml(
	novelTitle: string,
	chapters: Array<{ fileName: string; title: string }>,
): string {
	const items = chapters
		.map(
			(chapter) =>
				`        <li><a href="text/${chapter.fileName}">${escapeXml(chapter.title)}</a></li>`,
		)
		.join("\n")

	return `<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="vi" lang="vi">
  <head>
    <title>${escapeXml(novelTitle)}</title>
    <link href="styles/main.css" rel="stylesheet" type="text/css"/>
  </head>
  <body>
    <nav epub:type="toc" id="toc">
      <h1>Mục lục</h1>
      <ol>
${items}
      </ol>
    </nav>
  </body>
</html>`
}

function ncxXml(
	novelTitle: string,
	identifier: string,
	chapters: Array<{ fileName: string; title: string }>,
): string {
	const navPoints = chapters
		.map(
			(
				chapter,
				index,
			) => `    <navPoint id="navPoint-${index + 1}" playOrder="${index + 1}">
      <navLabel><text>${escapeXml(chapter.title)}</text></navLabel>
      <content src="text/${chapter.fileName}"/>
    </navPoint>`,
		)
		.join("\n")

	return `<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head>
    <meta name="dtb:uid" content="${escapeXml(identifier)}"/>
    <meta name="dtb:depth" content="1"/>
    <meta name="dtb:totalPageCount" content="0"/>
    <meta name="dtb:maxPageNumber" content="0"/>
  </head>
  <docTitle><text>${escapeXml(novelTitle)}</text></docTitle>
  <navMap>
${navPoints}
  </navMap>
</ncx>`
}

function contentOpf(params: {
	identifier: string
	title: string
	author: string | null
	includeCover: boolean
	coverExt: string
	includeFont: boolean
	fontFileName: string
	chapters: Array<{ id: string; fileName: string }>
}): string {
	const authorMetadata = params.author
		? `    <dc:creator>${escapeXml(params.author)}</dc:creator>\n`
		: ""

	const coverManifest = params.includeCover
		? `    <item id="cover-image" href="cover${params.coverExt}" media-type="${
				mime.lookup(params.coverExt) || "image/jpeg"
			}"/>\n`
		: ""

	const fontManifest = params.includeFont
		? `    <item id="epub-font" href="fonts/${params.fontFileName}" media-type="${
				mime.lookup(params.fontFileName) || "font/ttf"
			}"/>\n`
		: ""

	const chapterManifest = params.chapters
		.map(
			(chapter) =>
				`    <item id="${chapter.id}" href="text/${chapter.fileName}" media-type="application/xhtml+xml"/>`,
		)
		.join("\n")

	const spineItems = params.chapters
		.map((chapter) => `    <itemref idref="${chapter.id}"/>`)
		.join("\n")

	const coverMeta = params.includeCover
		? `    <meta name="cover" content="cover-image"/>\n`
		: ""

	return `<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="BookId">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="BookId">${escapeXml(params.identifier)}</dc:identifier>
    <dc:title>${escapeXml(params.title)}</dc:title>
    <dc:language>vi</dc:language>
${authorMetadata}${coverMeta}  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="style" href="styles/main.css" media-type="text/css"/>
    <item id="titlepage" href="text/titlepage.xhtml" media-type="application/xhtml+xml"/>
${coverManifest}${fontManifest}${chapterManifest}
  </manifest>
  <spine toc="ncx">
    <itemref idref="nav"/>
    <itemref idref="titlepage"/>
${spineItems}
  </spine>
</package>`
}

export async function buildEpub(params: {
	novelMainUrl: string
	chapterDir: string
	outputEpub?: string
	fontPath?: string
}): Promise<string> {
	const chapterDir = path.resolve(params.chapterDir)
	if (!(await fileExists(chapterDir))) {
		throw new Error(`Chapter directory not found: ${chapterDir}`)
	}

	const mainHtml = await fetchHtml(params.novelMainUrl)
	const novelTitle = extractNovelTitleFromMainPage(mainHtml)
	const author = extractAuthorFromMainPage(mainHtml)

	const coverUrl = extractCoverImageUrl(params.novelMainUrl, mainHtml)
	let coverBytes: Buffer | null = null
	let coverExt = ".jpg"
	let coverMediaType = "image/jpeg"

	if (coverUrl) {
		try {
			const downloaded = await downloadBinary(coverUrl)
			coverBytes = downloaded.content
			coverMediaType = downloaded.contentType || "image/jpeg"
			coverExt = pickCoverExtension(coverUrl, coverMediaType)
		} catch {
			coverBytes = null
		}
	}

	const outputEpub =
		params.outputEpub ??
		path.join(chapterDir, `${slugify(novelTitle, "book")}.epub`)

	const fontPath = await findFontFile(params.fontPath)
	const fontBytes = fontPath ? await fs.readFile(fontPath) : null
	const fontMetadata = fontPath
		? await extractFontMetadata(fontPath)
		: { familyName: "serif", extension: ".ttf" }
	const embeddedFontFileName = `epub-font${fontMetadata.extension}`

	const zip = new JSZip()
	zip.file("mimetype", "application/epub+zip", { compression: "STORE" })
	zip.folder("META-INF")?.file(
		"container.xml",
		`<?xml version="1.0" encoding="utf-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>`,
	)

	const epubFolder = zip.folder("EPUB")
	if (!epubFolder) {
		throw new Error("Could not initialize EPUB archive.")
	}

	const css = `
@font-face {
  font-family: '${fontMetadata.familyName.replace(/'/g, "\\'")}';
  src: url('../fonts/${embeddedFontFileName}');
}

body {
  font-family: '${fontMetadata.familyName.replace(/'/g, "\\'")}', serif;
  line-height: 1.8;
  margin: 0%;
  padding: 0;
}

h1 {
  text-align: center;
  font-size: 2.2em;
  font-weight: bold;
  margin: 2.5em 0 1.5em 0;
  padding: 0;
}

p {
  margin: 0 0 0.9em 0;
  text-indent: 2em;
  text-align: justify;
}
`.trim()
	epubFolder.folder("styles")?.file("main.css", css)

	if (fontBytes) {
		epubFolder.folder("fonts")?.file(embeddedFontFileName, fontBytes)
	} else {
		console.warn("[WARN] No EPUB font found, fallback to serif")
	}

	if (coverBytes) {
		epubFolder.file(`cover${coverExt}`, coverBytes)
	}

	const chapterFiles = await listChapterFiles(chapterDir)
	const chapters: Array<{ id: string; fileName: string; title: string }> = []
	const textFolder = epubFolder.folder("text")
	if (!textFolder) {
		throw new Error("Could not initialize EPUB text folder.")
	}

	for (const [index, chapterFile] of chapterFiles.entries()) {
		const chapter = await extractTitleAndBodyFromSavedChapter(chapterFile)
		const fileName = `chapter_${String(index + 1).padStart(4, "0")}.xhtml`
		const id = `chapter_${String(index + 1).padStart(4, "0")}`

		textFolder.file(fileName, chapterXhtml(chapter.title, chapter.bodyHtml))
		chapters.push({ id, fileName, title: chapter.title })
	}

	epubFolder.file(
		"nav.xhtml",
		navXhtml(
			novelTitle,
			chapters.map((c) => ({ fileName: c.fileName, title: c.title })),
		),
	)
	epubFolder.file(
		"toc.ncx",
		ncxXml(
			novelTitle,
			params.novelMainUrl,
			chapters.map((c) => ({ fileName: c.fileName, title: c.title })),
		),
	)
	epubFolder.file(
		"content.opf",
		contentOpf({
			identifier: params.novelMainUrl,
			title: novelTitle,
			author,
			includeCover: !!coverBytes,
			coverExt,
			includeFont: !!fontBytes,
			fontFileName: embeddedFontFileName,
			chapters,
		}),
	)

	textFolder.file("titlepage.xhtml", titlePageXhtml(novelTitle, author))

	const content = await zip.generateAsync({ type: "nodebuffer" })
	await fs.writeFile(outputEpub, content)

	return outputEpub
}
