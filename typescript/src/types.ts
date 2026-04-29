export interface ChapterContent {
	novelTitle: string
	chapterTitle: string
	paragraphs: string[]
}

export interface CrawlResult {
	novelTitle: string
	outputDir: string
	outputPath: string
	status: "written" | "skipped" | "skip_all"
}

export const ExistingFilePolicy = {
	ASK: "ask",
	SKIP: "skip",
	OVERWRITE: "overwrite",
	SKIP_ALL: "skip_all",
} as const

export type ExistingFilePolicyValues = typeof ExistingFilePolicy
