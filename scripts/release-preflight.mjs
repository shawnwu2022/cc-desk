import { appendFileSync, readFileSync, writeFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { basename, dirname } from 'node:path'
import { mayPublish, requiredChecksPassed, resolveCoverageArtifact, validateArtifacts, validateCoverageBinding } from './release-policy.mjs'
import { REPORT_FILENAME, VALIDATION_POLICY } from './windows-native-validation.mjs'
import { fetchCoverageArchive, validateFetchedCoverage } from './release-coverage-archive.mjs'

const { GITHUB_REPOSITORY: repository, GITHUB_SHA: sha, GITHUB_REF: ref, GITHUB_EVENT_NAME: event, GITHUB_TOKEN: token } = process.env
if (!/^[-\w.]+\/[-\w.]+$/.test(repository ?? '') || !token) throw new Error('workflow repository/token required')
const flags = new Set(['--resolve-coverage', '--artifacts'])
const values = new Set(['--coverage', '--ci-run', '--ci-attempt', '--coverage-artifact-id', '--coverage-artifact-name', '--notes'])
const args = new Map()
for (let index = 2; index < process.argv.length; index++) {
  const name = process.argv[index]
  if (args.has(name) || (!flags.has(name) && !values.has(name))) throw new Error('unsupported or duplicate release preflight argument')
  const value = flags.has(name) ? true : process.argv[++index]
  if (!value || (typeof value === 'string' && value.startsWith('--'))) throw new Error('missing release preflight argument')
  args.set(name, value)
}
if (args.has('--resolve-coverage') && args.size !== 1) throw new Error('coverage resolver does not perform promotion')

async function api(path, optional = false) {
  const response = await fetch(`https://api.github.com/repos/${repository}/${path}`, {
    headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' },
    signal: AbortSignal.timeout(30000),
  })
  if (optional && response.status === 404) return null
  if (!response.ok) throw new Error(`release gate API failed: HTTP ${response.status}`)
  return response.json()
}
async function pages(path, key) {
  const items = []
  for (let page = 1; page <= 100; page++) {
    const result = await api(`${path}${path.includes('?') ? '&' : '?'}per_page=100&page=${page}`)
    const batch = key ? result[key] : result
    if (!Array.isArray(batch)) throw new Error('invalid paginated release gate response')
    items.push(...batch)
    if (batch.length < 100) return items
  }
  throw new Error('release gate pagination exceeded bound')
}
function positiveInteger(value) {
  if (!/^[1-9]\d*$/.test(value ?? '') || !Number.isSafeInteger(Number(value))) throw new Error('invalid coverage binding integer')
  return Number(value)
}
function notes(summary, binding, reportHash) {
  const c = summary.counts
  return `Validation policy: ${VALIDATION_POLICY}\n\nSource: ${binding.sourceSha}\nCI run: ${binding.runId}, attempt: ${binding.runAttempt}\nCoverage artifact: ${binding.artifactId} (${binding.artifactName})\n\n` +
    `Required frontend, Rust normal-suite/build/lint, and roundtrip compile-only policy jobs succeeded.\n` +
    `Native normal suite: ${c.executed} executed (${c.passed} passed, ${c.failed} failed, ${c.measured} measured); ${c.ignored} original ignored tests; ${summary.unverifiedNames.length} unavailable Job-free manager tests unverified.\n\n` +
    `Original unfiltered native All: ${summary.nativeAllStatus}. Native installation/return roundtrip acceptance is not proven by this coverage.\n\n` +
    `Real native installation/return roundtrip was not executed by this CI validation; the report discloses the observed host limits.\n` +
    `Current available reviewed official signed Windows historical packages support installation and preserved-current return with FreshSettings. Shared Claude/Codex configuration, history and project-file changes are not rolled back. Future packages require their own reviewed version/digest/size/inventory admission.\n\n` +
    (summary.unverifiedNames.length ? `Unverified tests (observed external Windows Job):\n${summary.unverifiedNames.map(name => `- ${name}`).join('\n')}\n\n` : '') +
    `Attached ${REPORT_FILENAME} SHA256: ${reportHash}. This is the report validated for this promotion. Client automatic-install policy and runtime historical-installation guards remain unchanged.\n`
}
const packageVersion = JSON.parse(readFileSync('package.json', 'utf8')).version
const tauriVersion = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8')).version
const cargoPackage = readFileSync('src-tauri/Cargo.toml', 'utf8').match(/\[package\]([\s\S]*?)(?=\n\[|$)/)?.[1]
const cargoVersion = cargoPackage?.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
const tag = `v${packageVersion}`
const branch = await api('branches/main')
const runs = await pages(`actions/workflows/ci.yml/runs?head_sha=${sha}&branch=main&event=push`, 'workflow_runs')
const ci = runs.sort((a, b) => b.id - a.id)[0]
const jobs = ci ? await pages(`actions/runs/${ci.id}/attempts/${ci.run_attempt}/jobs`, 'jobs') : []
const tagRef = await api(`git/ref/tags/${encodeURIComponent(tag)}`, true)
// Authenticated listing includes drafts; they remain conflicts.
const releases = await pages('releases', null)
const context = { event, ref, sha, main: { sha: branch.commit?.sha, protected: branch.protected },
  versions: [packageVersion, cargoVersion, tauriVersion], tag, tagExists: tagRef !== null,
  releaseExists: releases.some(release => release.tag_name === tag), ci, jobs }
if (!requiredChecksPassed(context)) throw new Error('release gate blocked: require current protected main, matching versions, successful required CI and unused tag/release')
const artifact = resolveCoverageArtifact(await pages(`actions/runs/${ci.id}/artifacts`, 'artifacts'), ci, sha)
const binding = { sourceSha: sha, runId: String(ci.id), runAttempt: ci.run_attempt, artifactId: artifact.id, artifactName: artifact.name }
if (args.has('--resolve-coverage')) {
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT,
    `ci_run=${binding.runId}\nci_attempt=${binding.runAttempt}\ncoverage_artifact_id=${binding.artifactId}\ncoverage_artifact_name=${binding.artifactName}\n`)
  console.log(`Resolved native coverage artifact ${binding.artifactId} for CI run ${binding.runId}, attempt ${binding.runAttempt}`)
} else {
  if (!args.has('--coverage')) throw new Error('release gate blocked: downloaded CI coverage report and resolved artifact metadata required')
  const resolved = { sourceSha: sha, runId: String(positiveInteger(args.get('--ci-run'))),
    runAttempt: positiveInteger(args.get('--ci-attempt')), artifactId: positiveInteger(args.get('--coverage-artifact-id')),
    artifactName: args.get('--coverage-artifact-name') }
  validateCoverageBinding(resolved, artifact, ci, sha)
  const coveragePath = args.get('--coverage')
  if (basename(coveragePath) !== REPORT_FILENAME) throw new Error('coverage report filename differs from CI contract')
  const files = await fetchCoverageArchive({ repository, token, artifact })
  const { report, summary, reportBytes } = validateFetchedCoverage(files,
    { sourceSha: sha, runId: binding.runId, runAttempt: binding.runAttempt }, dirname(coveragePath))
  const reportHash = createHash('sha256').update(reportBytes).digest('hex')
  context.coverage = { binding: resolved, artifact, report }
  if (!mayPublish(context)) throw new Error('release gate blocked: invalid native coverage')
  if (args.has('--artifacts')) {
    const runId = positiveInteger(process.env.GITHUB_RUN_ID)
    const run = await api(`actions/runs/${runId}`)
    if (run.head_sha !== sha || run.head_branch !== 'main' || run.path !== '.github/workflows/release.yml') throw new Error('release workflow source binding failed')
    validateArtifacts(await pages(`actions/runs/${runId}/artifacts`, 'artifacts'), sha, runId)
  }
  if (args.has('--notes')) writeFileSync(args.get('--notes'), notes(summary, binding, reportHash))
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `version=${packageVersion}\ntag=${tag}\nci_run=${ci.id}\n`)
  console.log(`Release preflight passed for ${sha}, CI run ${ci.id}, attempt ${ci.run_attempt}, ${tag}; ${summary.counts.executed} native tests executed, ${summary.counts.ignored} original ignored, ${summary.unverifiedNames.length} unverified; native All ${summary.nativeAllStatus}`)
}
