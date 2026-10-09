import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import test from 'node:test'

// Windows API observations are supplied at the boundary; execute the real
// test-only drain policy with a deterministic clock and error observations.
test('OwnedJobDrain_RealPolicyKeepsExactEmptyAndDeadlineRequirements', t => {
  const directory = mkdtempSync(join(tmpdir(), 'owned-job-drain-'))
  t.after(() => rmSync(directory, { recursive: true, force: true }))
  const policy = fileURLToPath(new URL('../../src-tauri/src/tests/version_history_payload/terminal_diagnostics.rs', import.meta.url))
  const fixture = readFileSync(new URL('../fixtures/owned-job-drain.rs', import.meta.url), 'utf8')
  const source = join(directory, 'drain.rs'), binary = join(directory, process.platform === 'win32' ? 'drain.exe' : 'drain')
  writeFileSync(source, `#[path = ${JSON.stringify(policy)}] mod policy;\n${fixture}`)
  const compile = spawnSync('rustc', ['--edition=2021', '--test', source, '-o', binary], { encoding: 'utf8', timeout: 30000 })
  assert.equal(compile.status, 0, compile.stderr || String(compile.error))
  const run = spawnSync(binary, ['--test-threads=1'], { encoding: 'utf8', timeout: 10000 })
  assert.equal(run.status, 0, run.stdout + run.stderr)
  assert.match(run.stdout, /11 passed; 0 failed; 0 ignored/)
})
