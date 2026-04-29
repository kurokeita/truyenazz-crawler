import { copyFile, mkdir } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const packageRoot = path.resolve(__dirname, "..")
const sourceFont = path.join(packageRoot, "Bokerlam.ttf")
const distDir = path.join(packageRoot, "dist")
const targetFont = path.join(distDir, "Bokerlam.ttf")

await mkdir(distDir, { recursive: true })
await copyFile(sourceFont, targetFont)
