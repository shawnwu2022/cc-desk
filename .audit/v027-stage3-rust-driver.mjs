// Local-only execution of the unchanged project runner; no CI or release claim.
import fs from 'node:fs'
import path from 'node:path'
import { execFileSync, spawnSync } from 'node:child_process'
import { createPlan, verifyBundle, runShard, aggregateResults } from '../scripts/windows-rust-shard-runner.mjs'
const root = process.cwd(), sourceSha = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8', windowsHide: true }).trim()
if (sourceSha !== '09a7d4224c357b5fc0d85830b2844b5b1c12ae3f') throw new Error('Local source changed before compilation verification')
const probe = fs.readFileSync('.audit/v027-stage3-host-probe.log', 'utf8')
const host = /WINDOWS_RUST_CI_PARENT pid=\d+ inJob=(True|False) elevated=(True|False)/.exec(probe)
if (!host) throw new Error('Successful actual host probe required')
const inJob = host[1] === 'True', elevated = host[2] === 'True'
// The runner requires a numeric binding. This is a local namespace, not a hosted run ID.
const environment = { ...process.env, CARGO_NET_OFFLINE: 'true', GITHUB_SHA: sourceSha, GITHUB_RUN_ID: '2026110601', GITHUB_RUN_ATTEMPT: '1' }
delete environment.GITHUB_OUTPUT; delete environment.GITHUB_STEP_SUMMARY
const bundle = path.resolve('.audit/v027-stage3-rust-bundle'), shards = path.resolve('.audit/v027-stage3-rust-shards')
const context = { localOnly: true, notHostedCi: true, syntheticRunBinding: '2026110601', sourceSha, inJob, elevated }
fs.writeFileSync('.audit/v027-stage3-rust-context.json', JSON.stringify(context, null, 2) + '\n')
const options = { root, environment, bundle, inJob, elevated }
let plan
if (fs.existsSync(path.join(bundle, 'plan.json'))) plan = verifyBundle(options).plan
else {
  fs.mkdirSync(path.join(bundle, 'logs'), { recursive: true })
  const docLog = fs.openSync(path.join(bundle, 'logs/doctests.log'), 'wx')
  const docs = spawnSync('cargo', ['test', '--locked', '--offline', '--doc'], { cwd: path.join(root, 'src-tauri'), env: environment,
    windowsHide: true, stdio: ['ignore', docLog, docLog], timeout: 20 * 60 * 1000 })
  fs.closeSync(docLog)
  if (docs.status !== 0 || docs.error) throw new Error('Local unfiltered doctests failed; original log retained')
  plan = createPlan({ ...options, docExitCode: 0 })
}
options.artifactName = plan.artifactName
console.log(JSON.stringify({ phase: 'planned', ...context, planHash: plan.planHash, harnesses: plan.harnesses.map(h => ({ identity: h.identity, full: h.full.length, selected: h.selected.length, ignored: h.ignored.length, excluded: h.excluded.length })) }))
const failures = []
for (let index = 0; index < 16; index++) {
  console.log(JSON.stringify({ phase: 'start', index }))
  try {
    const result = await runShard({ ...options, index, output: path.join(shards, `shard-${index}`),
      onPhase: phase => { if (phase.status === 'heartbeat') console.log(JSON.stringify({ index, ...phase })) } })
    console.log(JSON.stringify({ phase: 'complete', index, completed: result.completed, exitCode: result.exitCode, seconds: result.durationSeconds }))
    if (!result.completed || result.exitCode !== 0) failures.push(index)
  } catch (error) { failures.push(index); console.log(JSON.stringify({ phase: 'failed', index, error: String(error.message) })) }
}
try {
  const report = aggregateResults({ ...options, shards, coverage: path.resolve('.audit/v027-stage3-rust-coverage') })
  console.log(JSON.stringify({ phase: 'aggregate', localOnly: true, harnesses: report.harnesses.map(h => ({ identity: h.identity, result: h.result })), nativeJobSuite: report.nativeJobSuite, nativeUnelevatedSuite: report.nativeUnelevatedSuite }))
} catch (error) { console.log(JSON.stringify({ phase: 'aggregate-failed', error: String(error.message), failures })); process.exitCode = 1 }
if (failures.length) process.exitCode = 1
