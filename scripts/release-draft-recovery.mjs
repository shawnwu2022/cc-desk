import { createHash } from 'node:crypto'
import { lstatSync, readdirSync, readFileSync } from 'node:fs'
import { join, basename } from 'node:path'
import { sourceChecksPassed } from './release-policy.mjs'
import { verifyUpdaterManifest } from './verify-updater-manifest.js'
import { REPORT_FILENAME, VALIDATION_POLICY } from './windows-native-validation.mjs'

export const OLD_DRAFT_ID = 406663556
export const OLD_SOURCE = '5ed35db9a560e093a91a5ef32a1eb6dd171f27fd'
export const UNTAGGED_DRAFT_TAG = 'untagged-dce9f75805136bcd2e47'
export const API_HTTP_STATUS = Symbol('draft API HTTP status')
export const PRESERVED_LABEL = 'Preserved previous unpublished candidate (source 5ed35db9); not a current installer'
export const OLD_ASSETS = Object.freeze([
  [621380125, 'CC.Desk_0.18.1_aarch64.dmg', 8127924, 'd28b5b177aed7848b4cccc7de852b114cbfb38f0a80c9566781d9520597db6ba'],
  [621381531, 'CC.Desk.app.tar.gz', 7625049, '2917784d637be33c02ce2071dae33ca60eb824aef1caec5f89047ba087de12d0'],
  [621382673, 'CC.Desk.app.tar.gz.sig', 404, 'c1cbae7b3c254a20d5cea64d8bc2fa02c60c51eb02160e357cb9f96066daeca1'],
  [621382810, 'CC.Desk_0.18.1_x64-setup.exe', 7722804, '72915dbe6aa8c6d73e4eeb99f05af74d8bbac3c12640d4b8d9b5ec9b8ffa7195'],
  [621384505, 'CC.Desk_0.18.1_x64-setup.exe.sig', 420, '913ea1ea2efd9db4fc4323ba288dfac699f995a49aaf469bfa70ca0c86b605d5'],
].map(([id, name, size, sha256]) => Object.freeze({ id, name, size, sha256 })))

export function preservedAssetName(id) {
  requireThat(OLD_ASSETS.some(asset => asset.id === id), 'unreviewed original asset ID')
  return `preserved-5ed35db9-${id}.bin`
}

function preservedAssets(release, allowOriginal = false) {
  for (const expected of OLD_ASSETS) {
    const matches = release.assets?.filter(asset => asset.id === expected.id)
    const asset = matches?.[0]
    requireThat(matches?.length === 1 && asset.size === expected.size && asset.digest === `sha256:${expected.sha256}`
      && asset.state === 'uploaded' && ((asset.name === preservedAssetName(expected.id) && asset.label === PRESERVED_LABEL)
        || (allowOriginal && asset.name === expected.name)), 'preserved original asset changed')
  }
}

export function assertPreparedDraft(release, binding) {
  requireThat(release?.id === OLD_DRAFT_ID && release.tag_name === binding.tag && release.target_commitish === binding.sha
    && release.draft === true && release.prerelease === false && release.published_at === null, 'prepared draft identity changed')
  requireThat(typeof binding.body === 'string' && release.body === binding.body && release.name === 'CC Desk 0.18.1', 'prepared draft disclosure or title changed')
  const markers = release.body?.match(/<!-- cc-desk-stage:[^\n]*? -->/g)
  requireThat(markers?.length === 1 && markers[0] === stageMarker(binding), 'prepared source/run/inventory differs')
  requireThat(Array.isArray(release.assets) && release.assets.length === OLD_ASSETS.length, 'prepared draft already contains new or unknown assets')
  preservedAssets(release)
}

export function assertRepairablePreparedDraft(release, binding) {
  requireThat(release?.tag_name === binding.tag || release?.tag_name === UNTAGGED_DRAFT_TAG,
    'unreviewed prepared draft tag')
  assertPreparedDraft({ ...release, tag_name: binding.tag }, binding)
}

export async function readDraftApiResponse(response, method) {
  if (!response.ok) {
    const error = new Error(`Draft transaction: ${method} API HTTP ${response.status}`)
    error.status = response.status
    throw error
  }
  const result = await response.json()
  Object.defineProperty(result, API_HTTP_STATUS, { value: response.status })
  return result
}

function requireThat(value, message) { if (!value) throw new Error(`Draft recovery: ${message}`) }
export function hash(bytes) { return createHash('sha256').update(bytes).digest('hex') }
export function assertReviewedOldDraft(release) {
  requireThat(release?.id === OLD_DRAFT_ID && release.target_commitish === OLD_SOURCE
    && release.draft === true && release.prerelease === false && release.published_at === null
    && release.tag_name === 'v0.18.1', 'old draft identity changed')
  requireThat(typeof release.body === 'string' && typeof release.name === 'string'
    && Array.isArray(release.assets) && release.assets.length === OLD_ASSETS.length, 'old draft metadata/inventory changed')
  for (const expected of OLD_ASSETS) {
    const matches = release.assets.filter(asset => asset.id === expected.id)
    requireThat(matches.length === 1 && matches[0].name === expected.name && matches[0].size === expected.size
      && matches[0].digest === `sha256:${expected.sha256}` && matches[0].state === 'uploaded', 'old asset identity changed')
  }
}
// This admits only read-only candidate preparation. Ordinary promotion still
// requires the version to be unused and never consumes this result as a waiver.
export function recoveryCandidateChecks(context, releases, tagRef) {
  if (!sourceChecksPassed(context) || context.tag !== 'v0.18.1' || context.tagExists !== false || tagRef !== null) return false
  if (!Array.isArray(releases)) return false
  const conflicts = releases.filter(release => release.tag_name === context.tag || release.id === OLD_DRAFT_ID)
  if (!conflicts.length) return context.releaseExists === false
  if (conflicts.length !== 1 || context.releaseExists !== true) return false
  try { assertReviewedOldDraft(conflicts[0]); return true } catch { return false }
}
export function stableRelease(release) {
  return { id: release.id, tag_name: release.tag_name, target_commitish: release.target_commitish,
    name: release.name, body: release.body, draft: release.draft, prerelease: release.prerelease,
    published_at: release.published_at, created_at: release.created_at,
    assets: release.assets.map(asset => ({ id: asset.id, name: asset.name, size: asset.size,
      digest: asset.digest, state: asset.state, label: asset.label, content_type: asset.content_type }))
      .sort((a, b) => a.id - b.id) }
}
export function verifyInventoryFiles(inventory, files) {
  requireThat(Array.isArray(inventory) && inventory.length > 0 && files instanceof Map
    && files.size === inventory.length, 'backup or staging inventory incomplete')
  const seen = new Set()
  for (const entry of inventory) {
    requireThat(typeof entry.name === 'string' && /^[A-Za-z0-9_.-]+$/.test(entry.name)
      && !entry.name.includes('..') && !seen.has(entry.name) && Number.isSafeInteger(entry.size) && entry.size > 0
      && /^[a-f0-9]{64}$/.test(entry.sha256), 'invalid or duplicate inventory entry')
    seen.add(entry.name)
    const bytes = files.get(entry.name)
    requireThat(Buffer.isBuffer(bytes) && bytes.length === entry.size && hash(bytes) === entry.sha256, 'asset byte/hash mismatch')
  }
}
export function readRegularFiles(root) {
  requireThat(lstatSync(root).isDirectory() && !lstatSync(root).isSymbolicLink(), 'expected regular directory')
  const result = new Map()
  for (const entry of readdirSync(root)) {
    const path = join(root, entry), stat = lstatSync(path)
    requireThat(stat.isFile() && !stat.isSymbolicLink(), 'expected only regular backup files')
    const maximum = OLD_ASSETS.find(asset => asset.name === entry)?.size
      ?? (entry === 'release.json' ? 2 * 1024 * 1024 : entry === 'backup.json' ? 128 * 1024 : 0)
    requireThat(stat.size > 0 && stat.size <= maximum, 'backup filename or byte bound changed')
    result.set(entry, readFileSync(path))
  }
  return result
}
export function backupArtifactName(sha, runId, attempt) {
  requireThat(/^[a-f0-9]{40}$/.test(sha) && Number.isSafeInteger(runId) && runId > 0
    && Number.isSafeInteger(attempt) && attempt > 0, 'invalid backup source/run')
  return `cc-desk-old-draft-backup-${sha}-${runId}-${attempt}`
}
export function stageMarker(binding) {
  requireThat(/^[a-f0-9]{40}$/.test(binding.sha) && Number.isSafeInteger(binding.runId) && binding.runId > 0
    && Number.isSafeInteger(binding.attempt) && binding.attempt > 0 && binding.tag === 'v0.18.1', 'invalid staging source/run')
  return `<!-- cc-desk-stage:${binding.sha}:${binding.runId}:${binding.attempt}:${hash(Buffer.from(JSON.stringify(binding.inventory)))} -->`
}
export function assertStagedRelease(release, binding, id) {
  requireThat(id === OLD_DRAFT_ID && release?.id === id
    && release.tag_name === binding.tag && release.target_commitish === binding.sha && release.draft === true
    && release.prerelease === false && release.published_at === null, 'staged release identity changed')
  requireThat(typeof binding.body === 'string' && release.body === binding.body && release.name === 'CC Desk 0.18.1', 'staged draft disclosure or title changed')
  const markers = release.body?.match(/<!-- cc-desk-stage:[^\n]*? -->/g)
  requireThat(markers?.length === 1 && markers[0] === stageMarker(binding), 'staging source/run/inventory differs')
  const preservedCount = OLD_ASSETS.length
  requireThat(Array.isArray(release.assets) && release.assets.length === binding.inventory.length + preservedCount, 'staged asset set incomplete')
  if (preservedCount) preservedAssets(release)
  const ids = new Set()
  for (const expected of binding.inventory) {
    const matches = release.assets.filter(asset => asset.name === expected.name), asset = matches[0]
    requireThat(matches.length === 1 && Number.isSafeInteger(asset.id) && asset.id > 0 && !ids.has(asset.id)
      && !OLD_ASSETS.some(old => old.id === asset.id) && asset.state === 'uploaded'
      && asset.size === expected.size && asset.digest === `sha256:${expected.sha256}`, 'staged asset inventory differs')
    ids.add(asset.id)
  }
}
export async function downloadAsset(repository, token, id, maximum, request = fetch) {
  requireThat(repository === 'shawnwu2022/cc-desk' && Number.isSafeInteger(id) && id > 0
    && Number.isSafeInteger(maximum) && maximum > 0 && maximum <= 512 * 1024 * 1024, 'invalid asset download')
  let url = `https://api.github.com/repos/${repository}/releases/assets/${id}`
  for (let redirect = 0; redirect < 5; redirect++) {
    const response = await request(url, { redirect: 'manual', signal: AbortSignal.timeout(120000),
      headers: redirect === 0 ? { Authorization: `Bearer ${token}`, Accept: 'application/octet-stream', 'X-GitHub-Api-Version': '2022-11-28' } : {} })
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      const next = new URL(response.headers.get('location'))
      requireThat(next.protocol === 'https:' && !next.username && !next.password && (!next.port || next.port === '443')
        && (next.hostname === 'release-assets.githubusercontent.com' || next.hostname === 'objects.githubusercontent.com'), 'unsafe asset redirect')
      await response.body?.cancel(); url = next.href; continue
    }
    requireThat(response.ok && response.body, 'asset download failed')
    const length = response.headers.get('content-length')
    requireThat(length === null || (/^\d+$/.test(length) && Number(length) <= maximum), 'asset download size exceeds bound')
    const reader = response.body.getReader(), parts = []; let size = 0
    try {
      for (;;) {
        const { value, done } = await reader.read(); if (done) break
        size += value.length; requireThat(size <= maximum, 'asset download exceeds bound'); parts.push(Buffer.from(value))
      }
    } finally { await reader.cancel().catch(() => {}); reader.releaseLock() }
    return Buffer.concat(parts, size)
  }
  throw new Error('Draft recovery: asset redirect bound exceeded')
}

export function collectStageFiles(root, version, coveragePath, manifestPath) {
  requireThat(version === '0.18.1', 'recovery transaction applies only to 0.18.1')
  const files = new Map()
  function walk(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name), stat = lstatSync(path)
      requireThat(!stat.isSymbolicLink(), 'staging symlink forbidden')
      if (stat.isDirectory()) walk(path)
      else {
        requireThat(stat.isFile(), 'staging nonregular file forbidden')
        const name = basename(path).replaceAll(' ', '.')
        requireThat(!files.has(name), 'duplicate staging name'); files.set(name, readFileSync(path))
      }
    }
  }
  walk(root)
  const expected = ['CC.Desk_0.18.1_x64-setup.exe', 'CC.Desk_0.18.1_x64-setup.exe.sig',
    'CC.Desk_0.18.1_aarch64.dmg', 'CC.Desk.app.tar.gz', 'CC.Desk.app.tar.gz.sig',
    'CC.Desk_0.18.1_amd64.AppImage', 'CC.Desk_0.18.1_amd64.AppImage.sig']
  requireThat(files.size === expected.length && expected.every(name => files.has(name)), 'expected exact new seven platform files')
  files.set('latest.json', readFileSync(manifestPath))
  files.set('windows-native-coverage.json', readFileSync(coveragePath))
  const inventory = [...files].map(([name, bytes]) => ({ name, size: bytes.length, sha256: hash(bytes) })).sort((a, b) => a.name.localeCompare(b.name, 'en'))
  verifyInventoryFiles(inventory, files)
  return { files, inventory }
}

export async function verifyStagedReleaseBytes(release, binding, files, repository, token, pubkey, request = fetch) {
  assertStagedRelease(release, binding, release.id)
  verifyInventoryFiles(binding.inventory, files)
  const downloaded = new Map()
  for (const entry of binding.inventory) {
    const asset = release.assets.find(asset => asset.name === entry.name)
    downloaded.set(entry.name, await downloadAsset(repository, token, asset.id, entry.size, request))
  }
  verifyInventoryFiles(binding.inventory, downloaded)
  const manifest = JSON.parse(downloaded.get('latest.json'))
  for (const entry of Object.values(manifest.platforms ?? {})) {
    const name = decodeURIComponent(new URL(entry.url).pathname.split('/').at(-1))
    requireThat(downloaded.get(`${name}.sig`)?.toString('utf8').trim() === entry.signature, 'manifest/signature file mismatch')
  }
  await verifyUpdaterManifest(manifest, '0.18.1', pubkey, repository, async url => {
    const name = decodeURIComponent(new URL(url).pathname.split('/').at(-1)), bytes = downloaded.get(name)
    requireThat(bytes, 'manifest references missing staged payload')
    return { ok: true, arrayBuffer: async () => bytes }
  })
}

// Updating to the actual current default-branch commit uses the release API's
// ordinary Contents(write) permission. Preserve the original snapshot/assets;
// never use an invented target or move/create a tag to alter authorization.
export async function recoverSourcePreservingAssets(api, snapshot, sha) {
  assertReviewedOldDraft(snapshot)
  requireThat(/^[a-f0-9]{40}$/.test(sha) && sha !== OLD_SOURCE, 'invalid replacement source')
  const current = await api(`releases/${OLD_DRAFT_ID}`)
  assertReviewedOldDraft(current)
  requireThat(JSON.stringify(stableRelease(current)) === JSON.stringify(stableRelease(snapshot)), 'old metadata changed after backup')
  requireThat(await api('git/ref/tags/v0.18.1', true) === null, 'existing tag cannot be moved')
  const main = await api('branches/main')
  requireThat(main.protected === true && main.commit?.sha === sha, 'replacement source must be current protected main')
  const patch = { target_commitish: sha, tag_name: 'v0.18.1', draft: true }
  let failure
  try { await api(`releases/${OLD_DRAFT_ID}`, false, 'PATCH', patch) }
  catch (error) { failure = error }
  const observed = await api(`releases/${OLD_DRAFT_ID}`)
  const status = Number.isSafeInteger(failure?.status) ? ` (PATCH HTTP ${failure.status})` : failure ? ' (PATCH receipt unknown)' : ''
  requireThat(JSON.stringify(stableRelease(observed)) === JSON.stringify(stableRelease({ ...snapshot, ...patch })),
    `source update unresolved or changed metadata${status}`)
  return observed
}

export async function preserveOriginalAssets(api, snapshot, sha) {
  assertReviewedOldDraft(snapshot)
  for (const entry of OLD_ASSETS) {
    const release = await api(`releases/${OLD_DRAFT_ID}`)
    requireThat(release.id === OLD_DRAFT_ID && release.target_commitish === sha && release.tag_name === 'v0.18.1'
      && release.draft === true && release.published_at === null && release.prerelease === false
      && release.assets.length === OLD_ASSETS.length && release.body === snapshot.body && release.name === snapshot.name,
      'draft changed during original asset preservation')
    preservedAssets(release, true)
    const before = release.assets.find(asset => asset.id === entry.id)
    if (before.name === preservedAssetName(entry.id)) continue
    const originalAsset = stableRelease(snapshot).assets.find(asset => asset.id === entry.id)
    const currentAsset = stableRelease(release).assets.find(asset => asset.id === entry.id)
    requireThat(JSON.stringify(currentAsset) === JSON.stringify(originalAsset), 'original asset metadata changed after backup')
    requireThat(await api('git/ref/tags/v0.18.1', true) === null, 'existing tag cannot be moved')
    const main = await api('branches/main')
    requireThat(main.protected === true && main.commit?.sha === sha, 'asset replacement requires current protected main')
    const patch = { name: preservedAssetName(entry.id), label: PRESERVED_LABEL }
    let failure
    try { await api(`releases/assets/${entry.id}`, false, 'PATCH', patch) }
    catch (error) { failure = error }
    const observed = await api(`releases/assets/${entry.id}`)
    const status = Number.isSafeInteger(failure?.status) ? ` (PATCH HTTP ${failure.status})` : failure ? ' (PATCH receipt unknown)' : ''
    requireThat(observed.id === entry.id && observed.name === patch.name && observed.label === patch.label
      && observed.size === entry.size && observed.digest === `sha256:${entry.sha256}` && observed.state === 'uploaded',
      `asset rename unresolved or changed bytes${status}`)
  }
  const observed = await api(`releases/${OLD_DRAFT_ID}`)
  requireThat(JSON.stringify(stableRelease(observed)) === JSON.stringify(stableRelease(preservedSnapshot(snapshot, sha))),
    'draft metadata changed during original asset preservation')
}

function preservedSnapshot(snapshot, sha) {
  return { ...snapshot, target_commitish: sha,
    assets: snapshot.assets.map(asset => ({ ...asset, name: preservedAssetName(asset.id), label: PRESERVED_LABEL })) }
}

export async function replacePreservedDraftMetadata(api, snapshot, binding) {
  assertReviewedOldDraft(snapshot)
  const before = await api(`releases/${OLD_DRAFT_ID}`)
  const expected = preservedSnapshot(snapshot, binding.sha)
  requireThat(JSON.stringify(stableRelease(before)) === JSON.stringify(stableRelease(expected)), 'draft metadata changed after preservation')
  await writePreparedMetadata(api, expected, binding)
}

// An explicit repair accepts only the exact same prepared source/run/inventory.
// The immutable original backup still pins every retained snapshot/asset field.
export async function repairPreparedDraftMetadata(api, snapshot, binding) {
  assertReviewedOldDraft(snapshot)
  const before = await api(`releases/${OLD_DRAFT_ID}`)
  assertRepairablePreparedDraft(before, binding)
  const expected = { ...preservedSnapshot(snapshot, binding.sha), tag_name: binding.tag,
    name: 'CC Desk 0.18.1', body: binding.body, draft: true }
  requireThat(JSON.stringify(stableRelease({ ...before, tag_name: binding.tag })) === JSON.stringify(stableRelease(expected)),
    'prepared metadata differs from the immutable original backup')
  await writePreparedMetadata(api, expected, binding, before.tag_name === binding.tag)
}

async function writePreparedMetadata(api, expected, binding, alreadyPrepared = false) {
  assertPreparedDraft({ ...expected, tag_name: binding.tag, name: 'CC Desk 0.18.1', body: binding.body }, binding)
  const main = await api('branches/main')
  requireThat(main.protected === true && main.commit?.sha === binding.sha, 'main changed before metadata update')
  requireThat(await api(`git/ref/tags/${binding.tag}`, true) === null, 'tag appeared before metadata update')
  if (alreadyPrepared) return // A completed unknown acknowledgement is never replayed.
  const patch = { tag_name: binding.tag, target_commitish: binding.sha, name: 'CC Desk 0.18.1', body: binding.body, draft: true }
  let failure, receipt
  try { receipt = await api(`releases/${OLD_DRAFT_ID}`, false, 'PATCH', patch) }
  catch (error) { failure = error }
  const observed = await api(`releases/${OLD_DRAFT_ID}`)
  const httpStatus = failure?.status ?? receipt?.[API_HTTP_STATUS]
  const status = Number.isSafeInteger(httpStatus) ? ` (PATCH HTTP ${httpStatus})` : ' (PATCH receipt unknown)'
  const wanted = stableRelease({ ...expected, ...patch }), actual = stableRelease(observed)
  const differences = Object.keys(wanted).filter(field => JSON.stringify(wanted[field]) !== JSON.stringify(actual[field]))
  const safeFields = ['id', 'tag_name', 'target_commitish', 'draft', 'prerelease', 'published_at', 'created_at']
  const detail = differences.map(field => safeFields.includes(field)
    ? { field, expected: wanted[field], observed: actual[field] }
    : { field, expectedSha256: hash(Buffer.from(JSON.stringify(wanted[field]) ?? 'null')),
      observedSha256: hash(Buffer.from(JSON.stringify(actual[field]) ?? 'null')) })
  requireThat(differences.length === 0,
    `metadata update unresolved or concurrent changes${status}; changed fields: ${JSON.stringify(detail)}`)
  assertPreparedDraft(observed, binding)
}

export async function publishVerifiedDraft(api, binding, id) {
  const before = await api(`releases/${id}`)
  assertStagedRelease(before, binding, id)
  const beforeTag = await api(`git/ref/tags/${binding.tag}`, true)
  requireThat(beforeTag === null || (beforeTag.object?.type === 'commit' && beforeTag.object.sha === binding.sha), 'version tag changed before publication')
  let failure
  try { await api(`releases/${id}`, false, 'PATCH', { draft: false, make_latest: 'true' }) }
  catch (error) { failure = error } // Resolve by a read, never retry an unknown write.
  const observed = await api(`releases/${id}`)
  const status = Number.isSafeInteger(failure?.status) ? ` (PATCH HTTP ${failure.status})` : failure ? ' (PATCH receipt unknown)' : ''
  requireThat(observed.draft === false && typeof observed.published_at === 'string' && observed.published_at.length > 0, `publication response unresolved${status}`)
  assertStagedRelease({ ...observed, draft: true, published_at: null }, binding, id)
  const latest = await api('releases/latest')
  requireThat(latest.id === id && latest.tag_name === binding.tag && latest.draft === false, 'published release is not Latest')
  const tag = await api(`git/ref/tags/${binding.tag}`)
  requireThat(tag.object?.type === 'commit' && tag.object.sha === binding.sha, 'published tag source mismatch')
  return observed
}

export async function stageNewAssets(api, upload, binding, files) {
  verifyInventoryFiles(binding.inventory, files)
  let expected = await api(`releases/${OLD_DRAFT_ID}`)
  assertPreparedDraft(expected, binding)
  for (const entry of binding.inventory) {
    const before = await api(`releases/${OLD_DRAFT_ID}`)
    requireThat(JSON.stringify(stableRelease(before)) === JSON.stringify(stableRelease(expected)), 'draft changed during staging')
    requireThat(!before.assets.some(asset => asset.name === entry.name), 'new asset name exists; never overwrite')
    const main = await api('branches/main')
    requireThat(main.protected === true && main.commit?.sha === binding.sha, 'main changed during staging')
    requireThat(await api(`git/ref/tags/${binding.tag}`, true) === null, 'tag appeared during staging')
    let failure
    try { await upload(entry, files.get(entry.name)) }
    catch (error) { failure = error }
    const observed = await api(`releases/${OLD_DRAFT_ID}`)
    const matches = observed.assets.filter(asset => asset.name === entry.name), asset = matches[0]
    const status = Number.isSafeInteger(failure?.status) ? ` (POST HTTP ${failure.status})` : failure ? ' (POST receipt unknown)' : ''
    requireThat(matches.length === 1 && Number.isSafeInteger(asset.id) && asset.id > 0 && !before.assets.some(old => old.id === asset.id)
      && asset.state === 'uploaded' && asset.size === entry.size && asset.digest === `sha256:${entry.sha256}`,
      `asset upload unresolved${status}`)
    requireThat(JSON.stringify(stableRelease({ ...observed, assets: observed.assets.filter(other => other.id !== asset.id) }))
      === JSON.stringify(stableRelease(before)), 'other draft metadata or assets changed during upload')
    expected = structuredClone(observed)
  }
  assertStagedRelease(expected, binding, OLD_DRAFT_ID)
}

export function validationNotes(summary, binding, reportHash) {
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

export function stagedValidationNotes(notes, binding) {
  return `${notes}\nPrevious unpublished candidate assets are preserved as five labelled .bin backups (source 5ed35db9), separately from this run's nine current assets.\n${stageMarker(binding)}\n`
}
