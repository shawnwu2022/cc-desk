import fs from 'node:fs'
import path from 'node:path'
const root = process.env.CC_DESK_TEST_ROOT
if (!root || !path.isAbsolute(root)) throw new Error('test root required')
// The isolated test child self-expires even if its parent is forcibly stopped.
fs.writeFileSync(path.join(root, `${process.pid}.tmp`), JSON.stringify({
  argv: process.argv.slice(2), marker: process.env.CC_DESK_SERVICE_MARKER,
  stdin: !!process.stdin.isTTY, stdout: !!process.stdout.isTTY,
}))
fs.renameSync(path.join(root, `${process.pid}.tmp`), path.join(root, `${process.pid}.json`))
process.stdout.write('D11_REAL_READY\r\n')
setTimeout(() => process.exit(0), 45000)
