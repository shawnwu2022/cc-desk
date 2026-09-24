import { randomUUID } from 'node:crypto'
import { writeFileSync, realpathSync } from 'node:fs'
import { spawn } from 'node:child_process'
import { join } from 'node:path'

const root = process.env.CC_DESK_TEST_ROOT
if (!root || realpathSync(root) !== realpathSync(process.cwd())) {
  throw new Error('EXPLICIT_DISPOSABLE_TEST_ROOT_REQUIRED')
}
const [mode, ...argv] = process.argv.slice(2)
if (!['exit', 'hold', 'input', 'descendant'].includes(mode)) throw new Error('INVALID_PROBE_MODE')
const report = join(root, `${randomUUID()}.json`)
const value = { argv, stdinIsTTY: !!process.stdin.isTTY, stdoutIsTTY: !!process.stdout.isTTY }
if (mode === 'input') {
  process.stdin.setRawMode(true)
  const chunks = []
  let length = 0
  process.stdin.on('data', (chunk) => {
    chunks.push(chunk)
    length += chunk.length
    if (length >= 6) {
      writeFileSync(report, JSON.stringify({ ...value, input: [...Buffer.concat(chunks)] }))
      process.stdout.write('OWNED_TAIL\n', () => process.exit(17))
    }
  })
  process.stdin.resume()
}
writeFileSync(report, JSON.stringify(value), { flag: 'wx' })
process.stdout.write('OWNED_READY\n')
if (mode === 'exit') process.stdout.write('OWNED_TAIL\n', () => process.exit(17))
if (mode === 'descendant') {
  const child = spawn(process.execPath, [
    '-e',
    "setTimeout(() => process.stdout.write('DESCENDANT_TAIL\\n', () => process.exit(0)), 350)",
  ], {
    cwd: process.cwd(),
    stdio: ['ignore', 'inherit', 'inherit'],
    windowsHide: true,
  })
  child.once('spawn', () => {
    child.unref()
    process.stdout.write('ROOT_EXIT\n', () => process.exit(23))
  })
}
if (mode === 'hold') setInterval(() => {}, 1000)
