declare module "mime-types" {
	export function extension(type: string): string | false
	export function lookup(path: string): string | false

	const mime: {
		extension: typeof extension
		lookup: typeof lookup
	}

	export default mime
}
