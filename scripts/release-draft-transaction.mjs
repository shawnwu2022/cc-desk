import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { execFileSync } from 'node:child_process'
import { OLD_ASSETS, OLD_DRAFT_ID, assertReviewedOldDraft, hash, verifyInventoryFiles,
  readRegularFiles, backupArtifactName, stageMarker, collectStageFiles, recoverSourcePreservingAssets, preserveOriginalAssets, replacePreservedDraftMetadata, repairPreparedDraftMetadata, readDraftApiResponse, stagedValidationNotes, stageNewAssets, publishVerifiedDraft } from './release-draft-recovery.mjs'

const { GITHUB_REPOSITORY: repository, GITHUB_TOKEN: token, GITHUB_SHA: sha } = process.env
const runId = Number(process.env.GITHUB_RUN_ID), attempt = Number(process.env.GITHUB_RUN_ATTEMPT)
const backupName = backupArtifactName(sha, runId, attempt)
if (repository !== 'shawnwu2022/cc-desk' || !token) throw new Error('Draft transaction requires existing workflow authentication')
const [mode, ...args] = process.argv.slice(2)
if (!['backup', 'prepare', 'repair-metadata', 'stage', 'publish'].includes(mode)) throw new Error('Unsupported draft transaction mode')
const allowed = new Set(['--directory', '--backup-artifact-id', '--release-id'])
const options = new Map()
for (let i = 0; i < args.length; i += 2) {
  if (!allowed.has(args[i]) || options.has(args[i]) || !args[i + 1] || args[i + 1].startsWith('--')) throw new Error('Invalid draft transaction argument')
  options.set(args[i], args[i + 1])
}
function required(value, message) { if (!value) throw new Error(`Draft transaction: ${message}`) }
function id(value) { required(/^[1-9]\d*$/.test(value ?? '') && Number.isSafeInteger(Number(value)), 'invalid ID'); return Number(value) }
async function api(path, optional = false, method = 'GET', body) {
  const response = await fetch(`https://api.github.com/repos/${repository}/${path}`, {
    method, redirect: 'error', signal: AbortSignal.timeout(30000),
    headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28', ...(body ? { 'Content-Type': 'application/json' } : {}) },
    ...(body ? { body: JSON.stringify(body) } : {}),
  })
  if (optional && response.status === 404) return null
  if (method !== 'GET') console.log(`Draft transaction: ${method} API HTTP ${response.status}`)
  return readDraftApiResponse(response, method)
}
async function pages(path, key) {
  const items = []
  for (let page = 1; page <= 100; page++) {
    const result = await api(`${path}${path.includes('?') ? '&' : '?'}per_page=100&page=${page}`)
    const batch = key ? result[key] : result
    required(Array.isArray(batch), 'invalid API collection'); items.push(...batch)
    if (batch.length < 100) return items
  }
  throw new Error('Draft transaction: pagination exceeded bound')
}
async function gate(extra = []) {
  const report = JSON.parse(readFileSync('coverage/windows-native-coverage.json', 'utf8'))
  required(report.sourceSha === sha && /^[1-9]\d*$/.test(report.runId) && Number.isSafeInteger(report.runAttempt), 'invalid downloaded coverage binding')
  const name = `windows-native-coverage-${sha}-${report.runId}-${report.runAttempt}`
  const artifacts = await pages(`actions/runs/${report.runId}/artifacts`, 'artifacts')
  const matches = artifacts.filter(artifact => artifact.name === name)
  required(matches.length === 1, 'missing or duplicate coverage artifact')
  execFileSync(process.execPath, ['scripts/release-preflight.mjs', '--artifacts', '--coverage', 'coverage/windows-native-coverage.json',
    '--ci-run', report.runId, '--ci-attempt', String(report.runAttempt), '--coverage-artifact-id', String(matches[0].id),
    '--coverage-artifact-name', name, ...extra], { stdio: 'inherit', env: { ...process.env, GITHUB_OUTPUT: '' } })
}
function verifyBackup(directory) {
  const files = readRegularFiles(directory)
  required(files.size === OLD_ASSETS.length + 2 && files.has('release.json') && files.has('backup.json'), 'backup file inventory changed')
  const metadata = files.get('release.json'), snapshot = JSON.parse(metadata), manifest = JSON.parse(files.get('backup.json'))
  assertReviewedOldDraft(snapshot)
  required(manifest.schema === 1 && manifest.sourceSha === sha && manifest.runId === runId && manifest.attempt === attempt
    && manifest.artifactName === backupName && manifest.metadataSha256 === hash(metadata)
    && JSON.stringify(manifest.inventory) === JSON.stringify(OLD_ASSETS), 'backup manifest binding changed')
  files.delete('release.json'); files.delete('backup.json'); verifyInventoryFiles(OLD_ASSETS, files)
  return snapshot
}
if (mode === 'backup') {
  required(options.size === 1 && options.has('--directory'), 'backup requires only a new directory')
  await gate(['--prepare-draft-recovery'])
  const snapshot = await api(`releases/${OLD_DRAFT_ID}`)
  assertReviewedOldDraft(snapshot)
  required(await api('git/ref/tags/v0.18.1', true) === null, 'backup requires unused version tag')
  const directory = options.get('--directory'); mkdirSync(directory)
  const metadata = Buffer.from(`${JSON.stringify(snapshot, null, 2)}\n`)
  writeFileSync(join(directory, 'release.json'), metadata, { flag: 'wx' })
  const { downloadAsset } = await import('./release-draft-recovery.mjs')
  for (const entry of OLD_ASSETS) {
    const bytes = await downloadAsset(repository, token, entry.id, entry.size)
    required(bytes.length === entry.size && hash(bytes) === entry.sha256, 'original asset hash differs')
    writeFileSync(join(directory, entry.name), bytes, { flag: 'wx' })
  }
  writeFileSync(join(directory, 'backup.json'), `${JSON.stringify({ schema: 1, sourceSha: sha, runId, attempt,
    artifactName: backupName, metadataSha256: hash(metadata), inventory: OLD_ASSETS }, null, 2)}\n`, { flag: 'wx' })
  verifyBackup(directory)
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `backup_name=${backupName}\n`)
  console.log(`Verified complete original draft backup ${OLD_DRAFT_ID}; no release or tag changed`)
} else if (mode === 'prepare' || mode === 'repair-metadata') {
  required(options.size === 2 && options.has('--directory') && options.has('--backup-artifact-id'), 'prepare requires downloaded immutable backup')
  const artifact = await api(`actions/artifacts/${id(options.get('--backup-artifact-id'))}`)
  required(artifact.name === backupName && artifact.expired === false && artifact.workflow_run?.id === runId
    && artifact.workflow_run?.head_sha === sha && artifact.workflow_run?.head_branch === 'main', 'durable backup artifact binding changed')
  const snapshot = verifyBackup(options.get('--directory'))
  await gate([...(mode === 'prepare' ? ['--prepare-draft-recovery'] : ['--repair-prepared-release-id', String(OLD_DRAFT_ID)]),
    '--notes', 'release-validation-notes.md'])
  const { inventory } = collectStageFiles('artifacts', '0.18.1', 'coverage/windows-native-coverage.json', 'latest.json')
  const binding = { sha, runId, attempt, tag: 'v0.18.1', inventory }
  const notes = readFileSync('release-validation-notes.md', 'utf8')
  binding.body = mode === 'prepare' ? stagedValidationNotes(notes, binding) : notes
  writeFileSync('release-validation-notes.md', binding.body)
  if (mode === 'prepare') {
    await recoverSourcePreservingAssets(api, snapshot, sha)
    await preserveOriginalAssets(api, snapshot, sha)
    await replacePreservedDraftMetadata(api, snapshot, binding)
  } else await repairPreparedDraftMetadata(api, snapshot, binding)
  console.log(`Prepared original draft ${OLD_DRAFT_ID} on current main ${sha}; five original IDs and bytes preserved, no tag or asset deletion`)
} else if (mode === 'stage') {
  required(options.size === 0, 'stage takes no arguments')
  await gate(['--prepared-release-id', String(OLD_DRAFT_ID)])
  const { files, inventory } = collectStageFiles('artifacts', '0.18.1', 'coverage/windows-native-coverage.json', 'latest.json')
  const binding = { sha, runId, attempt, tag: 'v0.18.1', inventory, body: readFileSync('release-validation-notes.md', 'utf8') }
  await stageNewAssets(api, async (entry, bytes) => {
    const response = await fetch(`https://uploads.github.com/repos/${repository}/releases/${OLD_DRAFT_ID}/assets?name=${encodeURIComponent(entry.name)}`, {
      method: 'POST', redirect: 'error', signal: AbortSignal.timeout(120000),
      headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28', 'Content-Type': 'application/octet-stream' }, body: bytes,
    })
    if (!response.ok) { const error = new Error(`Upload HTTP ${response.status}`); error.status = response.status; throw error }
    await response.json()
  }, binding, files)
  console.log(`Staged nine current assets on original draft ${OLD_DRAFT_ID}; five original asset IDs preserved separately`)
} else {
  required(options.size === 1 && options.has('--release-id'), 'publish requires exact staged release ID')
  const releaseId = id(options.get('--release-id'))
  required(releaseId === OLD_DRAFT_ID, 'publication requires the exact original draft ID')
  await gate(['--staged-release-id', String(releaseId)])
  const { inventory } = collectStageFiles('artifacts', '0.18.1', 'coverage/windows-native-coverage.json', 'latest.json')
  await publishVerifiedDraft(api, { sha, runId, attempt, tag: 'v0.18.1', inventory, body: readFileSync('release-validation-notes.md', 'utf8') }, releaseId)
  console.log(`Published exact complete release ${releaseId} from main ${sha}; original draft/assets remain preserved`)
}
