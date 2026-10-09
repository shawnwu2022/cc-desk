import { appendFileSync, readFileSync, writeFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { basename, dirname } from 'node:path'
import { mayPublish, sourceChecksPassed, coverageChecksPassed, requiredChecksPassed, resolveCoverageArtifact, validateArtifacts, validateCoverageBinding } from './release-policy.mjs'
import { OLD_DRAFT_ID, backupArtifactName, recoveryCandidateChecks, assertReviewedOldDraft, assertReviewedPreparedDraft, preparedRecoveryBackup, validatePreparedRecoveryBackup, assertPreparedDraft, assertRepairablePreparedDraft, assertStagedRelease, collectStageFiles, verifyStagedReleaseBytes, validationNotes, stagedValidationNotes } from './release-draft-recovery.mjs'
import { REPORT_FILENAME, VALIDATION_POLICY } from './windows-native-validation.mjs'
import { fetchCoverageArchive, validateFetchedCoverage } from './release-coverage-archive.mjs'

const { GITHUB_REPOSITORY: repository, GITHUB_SHA: sha, GITHUB_REF: ref, GITHUB_EVENT_NAME: event, GITHUB_TOKEN: token } = process.env
if (!/^[-\w.]+\/[-\w.]+$/.test(repository ?? '') || !token) throw new Error('workflow repository/token required')
const flags = new Set(['--resolve-coverage', '--artifacts', '--prepare-draft-recovery'])
const values = new Set(['--coverage', '--ci-run', '--ci-attempt', '--coverage-artifact-id', '--coverage-artifact-name', '--notes', '--staged-release-id', '--prepared-release-id', '--repair-prepared-release-id'])
const args = new Map()
for (let index = 2; index < process.argv.length; index++) {
  const name = process.argv[index]
  if (args.has(name) || (!flags.has(name) && !values.has(name))) throw new Error('unsupported or duplicate release preflight argument')
  const value = flags.has(name) ? true : process.argv[++index]
  if (!value || (typeof value === 'string' && value.startsWith('--'))) throw new Error('missing release preflight argument')
  args.set(name, value)
}
if (args.has('--resolve-coverage') && args.size !== (args.has('--prepare-draft-recovery') ? 2 : 1)) throw new Error('coverage resolver does not perform promotion')
const repairPhase = args.has('--repair-prepared-release-id')
const phases = ['--staged-release-id', '--prepared-release-id', '--repair-prepared-release-id'].filter(name => args.has(name))
const inPlacePhase = phases.length > 0
if (inPlacePhase && (!args.has('--artifacts') || !args.has('--coverage') || args.has('--prepare-draft-recovery')
  || (args.has('--notes') && !repairPhase) || phases.length !== 1)) throw new Error('invalid staged publication preflight')

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
  releaseExists: releases.some(release => release.tag_name === tag || (tag === 'v0.18.1' && release.id === OLD_DRAFT_ID)), ci, jobs }
let staged, prepared
if (inPlacePhase) {
  if (!sourceChecksPassed(context)) throw new Error('release gate blocked: staged source/current CI changed')
  const id = positiveInteger(args.get(phases[0]))
  if (id !== OLD_DRAFT_ID) throw new Error('invalid recovery draft ID')
  const matches = releases.filter(release => release.tag_name === tag || release.id === id)
  if (matches.length !== 1 || matches[0].id !== id || (tagRef !== null
    && (repairPhase || tagRef.object?.type !== 'commit' || tagRef.object.sha !== sha))) throw new Error('release gate blocked: staged version conflict')
  const { files, inventory } = collectStageFiles('artifacts', packageVersion, args.get('--coverage'), 'latest.json')
  const phase = { release: matches[0], files, binding: { sha, tag, inventory,
    runId: positiveInteger(process.env.GITHUB_RUN_ID), attempt: positiveInteger(process.env.GITHUB_RUN_ATTEMPT) } }
  if (args.has('--prepared-release-id') || repairPhase) prepared = phase
  else staged = phase
} else if (!(requiredChecksPassed(context) || (args.has('--prepare-draft-recovery') && recoveryCandidateChecks(context, releases, tagRef)))) {
  throw new Error('release gate blocked: require current protected main, matching versions, successful required CI and unused tag/release')
}
let recoveryMode = '', recoveryBackup
if (args.has('--prepare-draft-recovery') && context.releaseExists) {
  const original = releases.find(release => release.id === OLD_DRAFT_ID)
  try { assertReviewedOldDraft(original); recoveryMode = 'original' }
  catch {
    assertReviewedPreparedDraft(original)
    recoveryMode = 'reprepare'
    recoveryBackup = preparedRecoveryBackup()
    validatePreparedRecoveryBackup(await api(`actions/artifacts/${recoveryBackup.id}`),
      await api(`actions/runs/${recoveryBackup.runId}`))
  }
}
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
  const expectedNotes = validationNotes(summary, binding, reportHash)
  if (prepared || staged) {
    const phase = prepared ?? staged
    phase.binding.body = stagedValidationNotes(expectedNotes, phase.binding)
    if (repairPhase) assertRepairablePreparedDraft(prepared.release, prepared.binding)
    else if (prepared) assertPreparedDraft(prepared.release, prepared.binding)
    else assertStagedRelease(staged.release, staged.binding, staged.release.id)
  }
  context.coverage = { binding: resolved, artifact, report }
  if (!(inPlacePhase || args.has('--prepare-draft-recovery') ? coverageChecksPassed(context) : mayPublish(context))) throw new Error('release gate blocked: invalid native coverage')
  if (args.has('--artifacts')) {
    const runId = positiveInteger(process.env.GITHUB_RUN_ID)
    const run = await api(`actions/runs/${runId}`)
    if (run.head_sha !== sha || run.head_branch !== 'main' || run.path !== '.github/workflows/release.yml') throw new Error('release workflow source binding failed')
    const allArtifacts = await pages(`actions/runs/${runId}/artifacts`, 'artifacts')
    const backupName = backupArtifactName(sha, runId, positiveInteger(process.env.GITHUB_RUN_ATTEMPT ?? '1'))
    const backups = allArtifacts.filter(artifact => artifact.name === backupName)
    if (backups.length > 1 || backups.some(artifact => artifact.expired !== false || artifact.workflow_run?.id !== runId
      || artifact.workflow_run?.head_sha !== sha || artifact.workflow_run?.head_branch !== 'main')) throw new Error('draft backup artifact binding failed')
    validateArtifacts(allArtifacts.filter(artifact => artifact.name !== backupName), sha, runId)
  }
  if (staged) {
    await verifyStagedReleaseBytes(staged.release, staged.binding, staged.files, repository, token,
      JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8')).plugins.updater.pubkey)
    const latestBranch = await api('branches/main')
    const latestRuns = await pages(`actions/workflows/ci.yml/runs?head_sha=${sha}&branch=main&event=push`, 'workflow_runs')
    const latestCi = latestRuns.sort((a, b) => b.id - a.id)[0]
    const latestJobs = latestCi ? await pages(`actions/runs/${latestCi.id}/attempts/${latestCi.run_attempt}/jobs`, 'jobs') : []
    if (latestCi?.id !== ci.id || latestCi.run_attempt !== ci.run_attempt || !sourceChecksPassed({ ...context,
      main: { sha: latestBranch.commit?.sha, protected: latestBranch.protected }, ci: latestCi, jobs: latestJobs })) {
      throw new Error('release gate blocked: source/CI changed during staged asset verification')
    }
    const latestArtifact = resolveCoverageArtifact(await pages(`actions/runs/${latestCi.id}/artifacts`, 'artifacts'), latestCi, sha)
    validateCoverageBinding(resolved, latestArtifact, latestCi, sha)
    if (latestArtifact.digest !== artifact.digest) throw new Error('release gate blocked: coverage archive digest changed during staged verification')
    const latestTag = await api(`git/ref/tags/${encodeURIComponent(tag)}`, true)
    if (latestTag !== null && (latestTag.object?.type !== 'commit' || latestTag.object.sha !== sha)) {
      throw new Error('release gate blocked: tag source changed during staged verification')
    }
  }
  if (args.has('--notes')) writeFileSync(args.get('--notes'), repairPhase ? prepared.binding.body : expectedNotes)
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT,
    `version=${packageVersion}\ntag=${tag}\nci_run=${ci.id}\nrecovery_draft=${args.has('--prepare-draft-recovery') && context.releaseExists ? OLD_DRAFT_ID : ''}\n` +
    `recovery_mode=${recoveryMode}\nrecovery_backup_id=${recoveryBackup?.id ?? ''}\nrecovery_backup_run=${recoveryBackup?.runId ?? ''}\n`)
  console.log(`${args.has('--prepare-draft-recovery') ? 'Read-only recovery candidate' : 'Release'} preflight passed for ${sha}, CI run ${ci.id}, attempt ${ci.run_attempt}, ${tag}; ${summary.counts.executed} native tests executed, ${summary.counts.ignored} original ignored, ${summary.unverifiedNames.length} unverified; native All ${summary.nativeAllStatus}`)
}
