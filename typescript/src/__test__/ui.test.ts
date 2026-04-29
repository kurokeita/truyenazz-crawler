import { beforeEach, describe, expect, it, vi } from "vitest"

const intro = vi.fn()
const outro = vi.fn()
const cancel = vi.fn()
const note = vi.fn()
const spinner = vi.fn(() => ({ start: vi.fn(), stop: vi.fn() }))
const text = vi.fn()
const pathPrompt = vi.fn()
const confirm = vi.fn()
const select = vi.fn()
const isCancel = vi.fn()

vi.mock("@clack/prompts", () => ({
	intro,
	outro,
	cancel,
	note,
	spinner,
	text,
	path: pathPrompt,
	confirm,
	select,
	isCancel,
}))

import {
	createSpinner,
	isPromptCancel,
	promptConfirm,
	promptExistingChapterAction,
	promptPath,
	promptSelect,
	promptText,
	showCancel,
	showDone,
	showNote,
	showWelcome,
} from "../ui.js"

beforeEach(() => {
	vi.clearAllMocks()
})

describe("ui wrappers", () => {
	it("delegates display helpers to clack", async () => {
		await showWelcome()
		await showDone("done")
		await showCancel("cancel")
		await showNote("body", "title")
		await createSpinner()

		expect(intro).toHaveBeenCalledWith(" truyenazz-crawl ")
		expect(outro).toHaveBeenCalledWith("done")
		expect(cancel).toHaveBeenCalledWith("cancel")
		expect(note).toHaveBeenCalledWith("body", "title")
		expect(spinner).toHaveBeenCalled()
	})

	it("passes prompt parameters through to clack", async () => {
		text.mockResolvedValue("text value")
		pathPrompt.mockResolvedValue("/tmp/font.ttf")
		confirm.mockResolvedValue(true)
		select.mockResolvedValue("custom")
		isCancel.mockResolvedValue(false)

		await expect(promptText({ message: "Text" })).resolves.toBe("text value")
		await expect(promptPath({ message: "Path", root: "/tmp" })).resolves.toBe(
			"/tmp/font.ttf",
		)
		await expect(promptConfirm({ message: "Confirm" })).resolves.toBe(true)
		await expect(
			promptSelect({
				message: "Select",
				options: [{ label: "Custom", value: "custom" as const }],
			}),
		).resolves.toBe("custom")
		await expect(isPromptCancel(Symbol("cancel"))).resolves.toBe(false)
	})

	it("maps existing chapter selection cancellation to skip", async () => {
		select.mockResolvedValue(Symbol("cancel"))
		isCancel.mockResolvedValue(true)

		await expect(
			promptExistingChapterAction("/tmp/chapter.html"),
		).resolves.toBe("skip")
	})

	it("returns the chosen existing chapter action", async () => {
		select.mockResolvedValue("skip_all")
		isCancel.mockResolvedValue(false)

		await expect(
			promptExistingChapterAction("/tmp/chapter.html"),
		).resolves.toBe("skip_all")
	})
})
