import fs from 'node:fs'
import path from 'node:path'
import { createHash } from 'node:crypto'
import { pathToFileURL } from 'node:url'

const MAX_FILES = 10000
const MAX_BYTES = 2 * 1024 * 1024 * 1024
function requireThat(condition, reason) {
  if (!condition) throw new Error(`CI evidence readback: ${reason}`)
}
function inventory(directory) {
  requireThat(typeof directory === 'string' && directory.length > 0, 'directory required')
  const root = path.resolve(directory), stat = fs.lstatSync(root)
  requireThat(stat.isDirectory() && !stat.isSymbolicLink(), 'regular evidence directory required')
  const files = new Map()
  let total = 0
  function walk(current, prefix = '') {
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const filename = path.join(current, entry.name), relative = prefix + entry.name
      requireThat(!entry.isSymbolicLink(), 'linked evidence refused')
      if (entry.isDirectory()) walk(filename, relative + '/')
      else {
        requireThat(entry.isFile(), 'nonregular evidence refused')
        const bytes = fs.statSync(filename).size
        total += bytes
        requireThat(Number.isSafeInteger(bytes) && total <= MAX_BYTES && files.size < MAX_FILES, 'evidence bound exceeded')
        files.set(relative, { filename, bytes })
      }
    }
  }
  walk(root)
  requireThat(files.size > 0, 'empty evidence refused')
  return files
}
async function digest(filename) {
  const value = createHash('sha256')
  for await (const chunk of fs.createReadStream(filename)) value.update(chunk)
  return value.digest('hex')
}
export async function verifyEvidenceCopy(original, downloaded) {
  const expected = inventory(original), actual = inventory(downloaded)
  requireThat(expected.size === actual.size && [...expected.keys()].every(name => actual.has(name)), 'file inventory differs')
  for (const [name, source] of expected) {
    const copy = actual.get(name)
    requireThat(source.bytes === copy.bytes && await digest(source.filename) === await digest(copy.filename), 'file bytes differ')
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  // 错误只报告固定原因，不输出原始日志、凭证或宿主路径。
  try {
    await verifyEvidenceCopy(process.env.EVIDENCE_ROOT, process.env.EVIDENCE_COPY)
    console.log('CI evidence upload reconciled by exact file inventory and SHA-256')
  } catch (error) {
    console.error(error.message.startsWith('CI evidence readback:') ? error.message : 'CI evidence readback: unreadable evidence')
    process.exitCode = 1
  }
}
