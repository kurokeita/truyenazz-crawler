type PromptSelectOption<T extends string> = {
	value: T
	label?: string
	hint?: string
	disabled?: boolean
}

type PromptAction = "redownload" | "skip" | "skip_all"

async function loadPrompts() {
	return import("@clack/prompts")
}

export async function showWelcome(): Promise<void> {
	const prompts = await loadPrompts()
	prompts.intro(" truyenazz-crawl ")
}

export async function showDone(message: string): Promise<void> {
	const prompts = await loadPrompts()
	prompts.outro(message)
}

export async function showCancel(message: string): Promise<void> {
	const prompts = await loadPrompts()
	prompts.cancel(message)
}

export async function showNote(message: string, title?: string): Promise<void> {
	const prompts = await loadPrompts()
	prompts.note(message, title)
}

export async function createSpinner() {
	const prompts = await loadPrompts()
	return prompts.spinner()
}

export async function promptText(params: {
	message: string
	placeholder?: string
	initialValue?: string
	validate?: (value: string | undefined) => string | Error | undefined
}): Promise<string | symbol> {
	const prompts = await loadPrompts()
	return prompts.text({
		message: params.message,
		placeholder: params.placeholder,
		initialValue: params.initialValue,
		validate: params.validate,
	})
}

export async function promptPath(params: {
	message: string
	initialValue?: string
	root?: string
	directory?: boolean
	validate?: (value: string | undefined) => string | Error | undefined
}): Promise<string | symbol> {
	const prompts = await loadPrompts()
	return prompts.path({
		message: params.message,
		initialValue: params.initialValue,
		root: params.root,
		directory: params.directory,
		validate: params.validate,
	})
}

export async function promptConfirm(params: {
	message: string
	initialValue?: boolean
}): Promise<boolean | symbol> {
	const prompts = await loadPrompts()
	return prompts.confirm(params)
}

export async function promptSelect<T extends string>(params: {
	message: string
	options: Array<PromptSelectOption<T>>
	initialValue?: T
}): Promise<T | symbol> {
	const prompts = await loadPrompts()
	const selectOptions = {
		message: params.message,
		options: params.options,
		initialValue: params.initialValue,
	} as unknown as Parameters<typeof prompts.select<T>>[0]
	return prompts.select<T>(selectOptions)
}

export async function isPromptCancel(value: unknown): Promise<boolean> {
	const prompts = await loadPrompts()
	return prompts.isCancel(value)
}

export async function promptExistingChapterAction(
	chapterPath: string,
): Promise<PromptAction> {
	const selected = await promptSelect<PromptAction>({
		message: `[EXISTS] ${chapterPath}`,
		options: [
			{
				label: "Redownload and overwrite this chapter",
				value: "redownload",
			},
			{
				label: "Skip this chapter",
				value: "skip",
			},
			{
				label: "Skip this and all later existing chapters",
				value: "skip_all",
			},
		],
		initialValue: "skip",
	})

	if (await isPromptCancel(selected)) {
		return "skip"
	}

	return selected as PromptAction
}
