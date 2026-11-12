// Only creates synthetic SDK data under this checkout; never accepts a database path.
import fs from 'node:fs'
import path from 'node:path'
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { createHash } from 'node:crypto'
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const modules = fs.realpathSync(process.argv[2])
const run = spawnSync(process.execPath, [path.join(root, 'tests/scripts/opencodeRecoverySdk.real.mjs'), modules], { cwd: root, windowsHide: true, encoding: 'utf8', timeout: 40000, maxBuffer: 1048576 })
assert.equal(run.status, 0, run.stderr)
const report = JSON.parse(run.stdout), seed = path.relative(path.join(root, '.audit'), report.root)
assert.match(seed, /^opencode-sdk-recovery-[A-Za-z0-9]+$/)
const hash = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex')
const node = fs.realpathSync(process.execPath)
const manifest = { seed, seedSha256: hash(path.join(report.root, 'fixture.db')), node, nodeSha256: hash(node), nodeVersion: process.version }
fs.writeFileSync(path.join(root, '.audit/opencode-webview-seed.json'), JSON.stringify(manifest, null, 2))
console.log(JSON.stringify(manifest))
