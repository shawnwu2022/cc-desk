import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import test from 'node:test'
import { readFileSync } from 'node:fs'
const reviewedPrepared = JSON.parse(readFileSync(new URL('../fixtures/release-prepared-406663556.json', import.meta.url)))

let recovery
try { recovery = await import('../../scripts/release-draft-recovery.mjs') }
catch (error) { if (error.code !== 'ERR_MODULE_NOT_FOUND') throw error }
const sha = 'a'.repeat(40)
const oldSource = '5ed35db9a560e093a91a5ef32a1eb6dd171f27fd'
const assets = [
  [621380125, 'CC.Desk_0.18.1_aarch64.dmg', 8127924, 'd28b5b177aed7848b4cccc7de852b114cbfb38f0a80c9566781d9520597db6ba'],
  [621381531, 'CC.Desk.app.tar.gz', 7625049, '2917784d637be33c02ce2071dae33ca60eb824aef1caec5f89047ba087de12d0'],
  [621382673, 'CC.Desk.app.tar.gz.sig', 404, 'c1cbae7b3c254a20d5cea64d8bc2fa02c60c51eb02160e357cb9f96066daeca1'],
  [621382810, 'CC.Desk_0.18.1_x64-setup.exe', 7722804, '72915dbe6aa8c6d73e4eeb99f05af74d8bbac3c12640d4b8d9b5ec9b8ffa7195'],
  [621384505, 'CC.Desk_0.18.1_x64-setup.exe.sig', 420, '913ea1ea2efd9db4fc4323ba288dfac699f995a49aaf469bfa70ca0c86b605d5'],
]
function oldDraft() {
  return { id: 406663556, tag_name: 'v0.18.1', target_commitish: oldSource, name: 'CC Desk 0.18.1',
    body: 'Original candidate notes', draft: true, prerelease: false, published_at: null,
    assets: assets.map(([id, name, size, digest]) => ({ id, name, size, digest: `sha256:${digest}`, state: 'uploaded', label: '' })) }
}
function context() {
  return { event: 'push', ref: 'refs/heads/main', sha, main: { sha, protected: true },
    versions: ['0.18.1', '0.18.1', '0.18.1'], tag: 'v0.18.1', tagExists: false, releaseExists: true,
    ci: { id: 7, run_attempt: 1, head_sha: sha, head_branch: 'main', event: 'push',
      path: '.github/workflows/ci.yml', status: 'completed', conclusion: 'success' },
    jobs: ['Frontend checks', 'Rust checks', 'Disposable roundtrip compile-only policy (no native acceptance)']
      .map(name => ({ name, status: 'completed', conclusion: 'success' })) }
}
function api() { assert.ok(recovery, 'exact reviewed draft recovery implementation is required'); return recovery }
function preservedFixtureAssets() {
  const { preservedAssetName, PRESERVED_LABEL } = api()
  return oldDraft().assets.map(asset => ({ ...asset, name: preservedAssetName(asset.id), label: PRESERVED_LABEL }))
}

test('DraftRecovery_ExactOldDraftAllowsCandidateOnly_001', () => {
  const { recoveryCandidateChecks } = api()
  assert.equal(recoveryCandidateChecks(context(), [oldDraft()], null), true)
})
test('DraftRecovery_ChangedIdentityInventoryOrTagRefuses_002', () => {
  const { recoveryCandidateChecks } = api()
  for (const mutate of [d => d.id++, d => d.target_commitish = sha, d => d.draft = false,
    d => d.prerelease = true, d => d.published_at = '2026-10-09', d => d.assets.pop(),
    d => d.assets.push(d.assets[0]), d => d.assets[0].id++, d => d.assets[0].name += '.old',
    d => d.assets[0].size++, d => d.assets[0].digest = `sha256:${sha}`, d => d.assets[0].state = 'starter']) {
    const draft = oldDraft(); mutate(draft)
    assert.equal(recoveryCandidateChecks(context(), [draft], null), false)
  }
  assert.equal(recoveryCandidateChecks(context(), [oldDraft(), oldDraft()], null), false)
  assert.equal(recoveryCandidateChecks(context(), [oldDraft()], { object: { sha: oldSource } }), false)
})
test('DraftRecovery_SourceCIFailuresNeverBecomeWaivers_003', () => {
  const { recoveryCandidateChecks } = api()
  for (const mutate of [c => c.main.sha = oldSource, c => c.main.protected = false,
    c => c.sha = oldSource, c => c.ref = 'refs/heads/feature', c => c.ci.conclusion = 'failure',
    c => c.ci.status = 'in_progress', c => c.jobs.pop(), c => c.jobs[0].conclusion = 'failure',
    c => c.versions[1] = '0.18.0', c => c.tagExists = true]) {
    const value = context(); mutate(value)
    assert.equal(recoveryCandidateChecks(value, [oldDraft()], null), false)
  }
})
test('DraftRecovery_IndependentBackupHashAndInventory_005', () => {
  const { verifyInventoryFiles } = api()
  const bytes = Buffer.from('preserved bytes')
  const inventory = [{ name: 'asset.exe', size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') }]
  assert.doesNotThrow(() => verifyInventoryFiles(inventory, new Map([['asset.exe', bytes]])))
  for (const files of [new Map(), new Map([['asset.exe', Buffer.from('tampered')]]),
    new Map([['asset.exe', bytes], ['extra', bytes]])]) assert.throws(() => verifyInventoryFiles(inventory, files))
  assert.throws(() => verifyInventoryFiles([...inventory, inventory[0]], new Map([['asset.exe', bytes]])))
  assert.throws(() => verifyInventoryFiles([{ ...inventory[0], name: '../asset.exe' }], new Map([['../asset.exe', bytes]])))
})
test('DraftRecovery_StagingBindsCompleteInventoryAndExactRun_006', () => {
  const { stageMarker, assertStagedRelease } = api()
  const inventory = [{ name: 'asset.exe', size: 4, sha256: 'b'.repeat(64) }]
  const binding = { sha, runId: 9, attempt: 1, tag: 'v0.18.1', inventory }
  binding.body = api().stageMarker(binding)
  const release = { id: 406663556, name: 'CC Desk 0.18.1', tag_name: binding.tag, target_commitish: sha, draft: true, prerelease: false,
    published_at: null, body: stageMarker(binding), assets: [...preservedFixtureAssets(), { id: 100, state: 'uploaded',
      name: inventory[0].name, size: 4, digest: `sha256:${inventory[0].sha256}` }] }
  assert.doesNotThrow(() => assertStagedRelease(release, binding, 406663556))
  for (const mutate of [r => r.id++, r => r.target_commitish = oldSource, r => r.draft = false,
    r => r.assets = [], r => r.assets.push(r.assets[0]), r => r.assets[0].size++,
    r => r.assets[0].digest = 'sha256:invalid', r => r.body += stageMarker({ ...binding, runId: 10 })]) {
    const changed = structuredClone(release); mutate(changed)
    assert.throws(() => assertStagedRelease(changed, binding, 406663556))
  }
  assert.throws(() => assertStagedRelease(release, { ...binding, runId: 10 }, 406663556))
  assert.throws(() => assertStagedRelease(release, { ...binding, attempt: 2 }, 406663556))
})
test('DraftRecovery_DownloadRedirectDoesNotForwardCredentials_007', async () => {
  const { downloadAsset } = api()
  const calls = []
  const result = await downloadAsset('shawnwu2022/cc-desk', 'synthetic', 123, 10, async (url, options) => {
    calls.push({ url, options })
    if (calls.length === 1) return new Response(null, { status: 302, headers: { location: 'https://release-assets.githubusercontent.com/file?fixture=1' } })
    return new Response('12345')
  })
  assert.equal(result.toString(), '12345')
  assert.ok(calls[0].options.headers.Authorization)
  assert.equal(calls[1].options.headers?.Authorization, undefined)
  await assert.rejects(downloadAsset('shawnwu2022/cc-desk', 'synthetic', 123, 10,
    async () => new Response(null, { status: 302, headers: { location: 'https://attacker.invalid/file' } })))
  await assert.rejects(downloadAsset('shawnwu2022/cc-desk', 'synthetic', 123, 4, async () => new Response('12345')))
})

test('DraftRecovery_NoDeleteOrCredentialCreationPath_009', () => {
  const transaction = readFileSync(new URL('../../scripts/release-draft-transaction.mjs', import.meta.url), 'utf8')
  assert.doesNotMatch(transaction, /['"]DELETE['"]|gh auth|credential|register.runner|git.*force/)
  assert.match(transaction, /verifyBackup\(options.get\('--directory'\)(?:, provenance)?\)/)
  assert.match(transaction, /artifact.workflow_run\?\.head_sha === sha/)
  const workflow = readFileSync(new URL('../../.github/workflows/release.yml', import.meta.url), 'utf8')
  const helper = readFileSync(new URL('../../scripts/release-draft-recovery.mjs', import.meta.url), 'utf8')
  assert.match(transaction, /replacePreservedDraftMetadata\(api, snapshot, binding\)/)
  assert.match(helper, /draft: true/)
  assert.ok(workflow.indexOf('Independently download the exact durable backup') < workflow.indexOf('prepare --directory'))
  assert.ok(workflow.indexOf('Generate updater manifest') < workflow.indexOf('backup --directory'))
})

test('DraftRecovery_WaitForCurrentMainCINeverSkipsFailure_010', async () => {
  let wait
  try { wait = await import('../../scripts/release-wait-for-ci.mjs') }
  catch (error) { if (error.code !== 'ERR_MODULE_NOT_FOUND') throw error }
  assert.ok(wait, 'same-source CI wait must be implemented')
  const input = { sha, main: { sha, protected: true }, runs: [] }
  assert.equal(wait.ciWaitState(input), 'wait')
  const run = { id: 10, head_sha: sha, head_branch: 'main', event: 'push', path: '.github/workflows/ci.yml', status: 'in_progress', conclusion: null }
  assert.equal(wait.ciWaitState({ ...input, runs: [run] }), 'wait')
  assert.equal(wait.ciWaitState({ ...input, runs: [{ ...run, status: 'completed', conclusion: 'success' }] }), 'ready')
  assert.throws(() => wait.ciWaitState({ ...input, runs: [{ ...run, status: 'completed', conclusion: 'failure' }] }), /failed/)
  assert.throws(() => wait.ciWaitState({ ...input, main: { ...input.main, sha: oldSource } }), /main/)
  assert.throws(() => wait.ciWaitState({ ...input, main: { ...input.main, protected: false } }), /main/)
  assert.equal(wait.ciWaitState({ ...input, runs: [{ ...run, id: 9, status: 'completed', conclusion: 'success' }, run] }), 'wait')
  let waits = 0, reads = 0
  await wait.waitForCurrentCi({ read: async () => { reads++; return reads === 1 ? { ...input, runs: [run] }
    : { ...input, runs: [{ ...run, status: 'completed', conclusion: 'success' }] } },
    pause: async () => { waits++ }, now: () => 0 })
  assert.equal(reads, 2); assert.equal(waits, 1)
  await assert.rejects(wait.waitForCurrentCi({ read: async () => input, now: () => 100,
    started: 0, timeout: 50, pause: async () => assert.fail('timeout must not sleep') }), /timed out/)
})
test('DraftRecovery_FinalTagConflictCannotPublish_011', async () => {
  const { stageMarker, publishVerifiedDraft } = api()
  const inventory = [{ name: 'asset.exe', size: 4, sha256: 'b'.repeat(64) }]
  const binding = { sha, runId: 9, attempt: 1, tag: 'v0.18.1', inventory }
  binding.body = api().stageMarker(binding)
  const staged = { id: 406663556, name: 'CC Desk 0.18.1', draft: true, published_at: null, prerelease: false, tag_name: binding.tag,
    target_commitish: sha, body: stageMarker(binding),
    assets: [...preservedFixtureAssets(), { id: 100, name: 'asset.exe', size: 4, digest: `sha256:${'b'.repeat(64)}`, state: 'uploaded' }] }
  let mutations = 0
  await assert.rejects(publishVerifiedDraft(async (path, optional, method) => {
    if (path.startsWith('git/ref/')) return { object: { type: 'commit', sha: oldSource } }
    if (method === 'PATCH') mutations++
    return staged
  }, binding, 406663556), /tag changed before publication/)
  assert.equal(mutations, 0)
})

test('DraftRecovery_InPlaceSourceUsesRealMainAndPreservesOriginalIDs_012', async () => {
  const { recoverSourcePreservingAssets } = api()
  assert.equal(typeof recoverSourcePreservingAssets, 'function')
  const snapshot = oldDraft()
  let current = structuredClone(snapshot), patches = []
  const request = async (path, optional, method, body) => {
    if (path === 'branches/main') return { protected: true, commit: { sha } }
    if (path.startsWith('git/ref/')) return null
    if (method === 'PATCH') { patches.push({ path, body }); current = { ...current, ...body } }
    return current
  }
  const updated = await recoverSourcePreservingAssets(request, snapshot, sha)
  assert.deepEqual(patches, [{ path: `releases/${snapshot.id}`, body: { target_commitish: sha, tag_name: 'v0.18.1', draft: true } }])
  assert.equal(updated.id, snapshot.id)
  assert.deepEqual(updated.assets, snapshot.assets)
  assert.equal(updated.body, snapshot.body)
  assert.equal(updated.name, snapshot.name)
  current = structuredClone(snapshot); patches = []
  await assert.rejects(recoverSourcePreservingAssets(async (path, optional, method, body) => {
    if (method === 'PATCH') { patches.push(body); const error = new Error('integration rejection'); error.status = 403; throw error }
    return request(path, optional, method, body)
  }, snapshot, sha), /PATCH HTTP 403/)
  assert.equal(patches.length, 1)
  patches = []
  await assert.rejects(recoverSourcePreservingAssets(async (path, optional, method, body) => {
    if (path === 'branches/main') return { protected: true, commit: { sha: oldSource } }
    return request(path, optional, method, body)
  }, snapshot, sha), /current protected main/)
  assert.equal(patches.length, 0)
})

test('DraftRecovery_InPlaceBackupsAreSeparateAndCannotStandInForNewAssets_013', () => {
  const { stageMarker, assertStagedRelease, preservedAssetName, PRESERVED_LABEL } = api()
  assert.equal(typeof preservedAssetName, 'function')
  const inventory = [{ name: 'asset.exe', size: 4, sha256: 'b'.repeat(64) }]
  const binding = { sha, runId: 9, attempt: 1, tag: 'v0.18.1', inventory }
  binding.body = api().stageMarker(binding)
  const release = { ...oldDraft(), target_commitish: sha, body: stageMarker(binding),
    assets: [...oldDraft().assets.map(a => ({ ...a, name: preservedAssetName(a.id), label: PRESERVED_LABEL })),
      { id: 100, name: 'asset.exe', size: 4, digest: `sha256:${'b'.repeat(64)}`, state: 'uploaded' }] }
  assert.doesNotThrow(() => assertStagedRelease(release, binding, release.id))
  for (const mutate of [r => r.assets.pop(), r => r.assets[0].name = oldDraft().assets[0].name,
    r => r.assets[0].digest = `sha256:${'c'.repeat(64)}`, r => r.assets[0].id++,
    r => r.assets[0].label = 'current installer', r => r.assets.push(r.assets[0])]) {
    const changed = structuredClone(release); mutate(changed)
    assert.throws(() => assertStagedRelease(changed, binding, release.id))
  }
})

test('DraftRecovery_InPlaceRenamePreservesBytesAndNeverReplaysUnknownReceipts_014', async () => {
  const { preserveOriginalAssets, preservedAssetName, PRESERVED_LABEL } = api()
  assert.equal(typeof preserveOriginalAssets, 'function')
  const snapshot = oldDraft(); let current = { ...structuredClone(snapshot), target_commitish: sha }, patches = []
  const request = async (path, optional, method, body) => {
    if (path === 'branches/main') return { protected: true, commit: { sha } }
    if (path.startsWith('git/ref/')) return null
    if (path.startsWith('releases/assets/')) {
      const asset = current.assets.find(a => a.id === Number(path.split('/').at(-1)))
      if (method === 'PATCH') { patches.push({ path, body }); Object.assign(asset, body); throw new Error('lost acknowledgement') }
      return asset
    }
    return current
  }
  await preserveOriginalAssets(request, snapshot, sha)
  assert.equal(patches.length, snapshot.assets.length)
  assert.deepEqual(current.assets.map(a => ({ ...a, name: snapshot.assets.find(old => old.id === a.id).name, label: snapshot.assets.find(old => old.id === a.id).label })), snapshot.assets)
  for (const patch of patches) {
    const id = Number(patch.path.split('/').at(-1))
    assert.deepEqual(patch.body, { name: preservedAssetName(id), label: PRESERVED_LABEL })
  }
  patches = []
  await preserveOriginalAssets(request, snapshot, sha)
  assert.equal(patches.length, 0)
  current = { ...structuredClone(snapshot), target_commitish: sha }
  await assert.rejects(preserveOriginalAssets(async (path, optional, method, body) => {
    if (method === 'PATCH') { patches.push(body); const error = new Error('permission rejected'); error.status = 403; throw error }
    return request(path, optional, method, body)
  }, snapshot, sha), /PATCH HTTP 403/)
  assert.equal(patches.length, 1)
})

function stagingFixture() {
  const { stageMarker, preservedAssetName, PRESERVED_LABEL, hash } = api()
  const files = new Map([['one.bin', Buffer.from('first')], ['two.bin', Buffer.from('second')]])
  const inventory = [...files].map(([name, bytes]) => ({ name, size: bytes.length, sha256: hash(bytes) }))
  const binding = { sha, runId: 9, attempt: 1, tag: 'v0.18.1', inventory }
  binding.body = api().stageMarker(binding)
  let release = { ...oldDraft(), target_commitish: sha, body: stageMarker(binding),
    assets: oldDraft().assets.map(asset => ({ ...asset, name: preservedAssetName(asset.id), label: PRESERVED_LABEL })) }
  let reads = 0, uploads = 0
  const fixture = { files, binding, get release() { return release }, get reads() { return reads }, get uploads() { return uploads } }
  fixture.read = async path => {
    if (path === 'branches/main') return { protected: true, commit: { sha } }
    if (path.startsWith('git/ref/')) return null
    reads++
    return structuredClone(release)
  }
  fixture.upload = async (entry, bytes) => {
    uploads++
    assert.deepEqual(bytes, files.get(entry.name))
    release.assets.push({ id: 100 + uploads, state: 'uploaded', name: entry.name, size: entry.size, digest: `sha256:${entry.sha256}` })
  }
  return fixture
}

test('DraftRecovery_StagingLostReceiptsResolveExactBytesWithoutReplay_015', async () => {
  const { stageNewAssets, assertStagedRelease } = api(), fixture = stagingFixture()
  await stageNewAssets(fixture.read, async (entry, bytes) => {
    await fixture.upload(entry, bytes)
    throw new Error('lost POST acknowledgement')
  }, fixture.binding, fixture.files)
  assert.equal(fixture.uploads, 2)
  assertStagedRelease(fixture.release, fixture.binding, fixture.release.id)
  const rejected = stagingFixture()
  let attempts = 0
  await assert.rejects(stageNewAssets(rejected.read, async () => {
    attempts++
    const error = new Error('permission rejected'); error.status = 403; throw error
  }, rejected.binding, rejected.files), /POST HTTP 403/)
  assert.equal(attempts, 1)
  assert.equal(rejected.release.assets.length, 5)
})

test('DraftRecovery_ConcurrentSourceTagMetadataOrAssetChangesStopStaging_016', async () => {
  const { stageNewAssets } = api()
  for (const change of ['main', 'tag', 'metadata', 'asset']) {
    const fixture = stagingFixture()
    const read = async path => {
      if (change === 'main' && path === 'branches/main') return { protected: true, commit: { sha: oldSource } }
      if (change === 'tag' && path.startsWith('git/ref/')) return { object: { type: 'commit', sha } }
      const result = await fixture.read(path)
      if (fixture.reads === 2 && path.startsWith('releases/')) {
        if (change === 'metadata') result.body += 'concurrent metadata'
        if (change === 'asset') result.assets[0].digest = `sha256:${'0'.repeat(64)}`
      }
      return result
    }
    await assert.rejects(stageNewAssets(read, fixture.upload, fixture.binding, fixture.files))
    assert.equal(fixture.uploads, 0, change)
  }
  const fixture = stagingFixture()
  await assert.rejects(stageNewAssets(fixture.read, async (entry, bytes) => {
    await fixture.upload(entry, bytes)
    fixture.release.body += 'concurrent change after POST'
  }, fixture.binding, fixture.files), /other draft metadata/)
  assert.equal(fixture.uploads, 1)
})

test('DraftRecovery_LastRenameCannotHideConcurrentDraftMetadata_017', async () => {
  const { preserveOriginalAssets } = api()
  for (const field of ['body', 'name']) {
    const snapshot = oldDraft()
    const current = { ...structuredClone(snapshot), target_commitish: sha }
    let patches = 0
    const request = async (path, optional, method, body) => {
      if (path === 'branches/main') return { protected: true, commit: { sha } }
      if (path.startsWith('git/ref/')) return null
      if (path.startsWith('releases/assets/')) {
        const asset = current.assets.find(asset => asset.id === Number(path.split('/').at(-1)))
        if (method === 'PATCH') {
          Object.assign(asset, body)
          if (++patches === assets.length) current[field] += ' concurrent change'
        }
        return asset
      }
      return current
    }
    await assert.rejects(preserveOriginalAssets(request, snapshot, sha), /metadata changed/)
    assert.equal(patches, assets.length)
    assert.match(current[field], /concurrent change$/)
  }
})

test('DraftRecovery_MetadataReplacementRequiresUnchangedPreservedSnapshot_018', async () => {
  const { replacePreservedDraftMetadata, stageMarker } = api()
  const binding = { sha, runId: 9, attempt: 1, tag: 'v0.18.1', inventory: [] }
  binding.body = `Fixed disclosure\n${stageMarker(binding)}\n`
  const snapshot = oldDraft()
  for (const field of ['body', 'name', 'main', 'tag', 'unchanged']) {
    let current = { ...structuredClone(snapshot), target_commitish: sha, assets: preservedFixtureAssets() }
    if (field === 'body' || field === 'name') current[field] += ' concurrent change'
    let patches = 0
    const request = async (path, optional, method, body) => {
      if (path === 'branches/main') return { protected: true, commit: { sha: field === 'main' ? oldSource : sha } }
      if (path.startsWith('git/ref/')) return field === 'tag' ? { object: { type: 'commit', sha } } : null
      if (method === 'PATCH') { patches++; current = { ...current, ...body }; throw new Error('lost acknowledgement') }
      return current
    }
    if (field === 'unchanged') {
      await replacePreservedDraftMetadata(request, snapshot, binding)
      assert.equal(patches, 1)
      assert.equal(current.body, binding.body)
    } else {
      await assert.rejects(replacePreservedDraftMetadata(request, snapshot, binding))
      assert.equal(patches, 0, field)
    }
  }
})

test('DraftRecovery_PublicationRefusesChangedDisclosureOrTitle_019', async () => {
  const { publishVerifiedDraft } = api()
  for (const field of ['body', 'name']) {
    const fixture = stagingFixture()
    await fixture.upload(fixture.binding.inventory[0], fixture.files.get('one.bin'))
    await fixture.upload(fixture.binding.inventory[1], fixture.files.get('two.bin'))
    fixture.release[field] += ' concurrent change'
    let patches = 0
    await assert.rejects(publishVerifiedDraft(async (path, optional, method) => {
      if (method === 'PATCH') patches++
      return fixture.release
    }, fixture.binding, 406663556), /disclosure or title changed/)
    assert.equal(patches, 0)
  }
})

test('DraftRecovery_RoutingUsesFreshWriteJobObservation_020', () => {
  const workflow = readFileSync(new URL('../../.github/workflows/release.yml', import.meta.url), 'utf8')
  assert.doesNotMatch(workflow, /needs\.preflight\.outputs\.recovery_draft/)
  assert.match(workflow, /name: Recheck main, required CI, coverage and same-run platform artifacts\n\s+id: promotion/)
  assert.equal((workflow.match(/if: steps\.promotion\.outputs\.recovery_draft/g) ?? []).length, 10)
  assert.match(workflow, /contents: write/)
})

test('DraftRecovery_PublicationReportsActualPermissionRejectionWithoutRetry_021', async () => {
  const { publishVerifiedDraft } = api(), fixture = stagingFixture()
  for (const entry of fixture.binding.inventory) await fixture.upload(entry, fixture.files.get(entry.name))
  let patches = 0
  await assert.rejects(publishVerifiedDraft(async (path, optional, method) => {
    if (path.startsWith('git/ref/')) return null
    if (method === 'PATCH') {
      patches++
      const error = new Error('integration rejected'); error.status = 403; throw error
    }
    return fixture.release
  }, fixture.binding, 406663556), /PATCH HTTP 403/)
  assert.equal(patches, 1)
})

function metadataFixture() {
  const snapshot = oldDraft()
  snapshot.created_at = '2026-10-08T00:00:00Z'
  const binding = { sha, runId: 9, attempt: 1, tag: 'v0.18.1', inventory: [] }
  binding.body = `Exact validation disclosure\n${api().stageMarker(binding)}\n`
  const current = { ...structuredClone(snapshot), target_commitish: sha, assets: preservedFixtureAssets() }
  return { snapshot, binding, current }
}

test('DraftRecovery_MetadataPatchKeepsExplicitVersionAndSource_022', async () => {
  const fixture = metadataFixture()
  let writes = 0
  await api().replacePreservedDraftMetadata(async (path, optional, method, body) => {
    if (path === 'branches/main') return { protected: true, commit: { sha } }
    if (path.startsWith('git/ref/')) return null
    if (method === 'PATCH') {
      writes++
      Object.assign(fixture.current, body)
      // Reproduce the observed API normalization when these fields are absent.
      if (!body.tag_name || !body.target_commitish) fixture.current.tag_name = 'untagged-dce9f75805136bcd2e47'
    }
    return structuredClone(fixture.current)
  }, fixture.snapshot, fixture.binding)
  api().assertPreparedDraft(fixture.current, fixture.binding)
  assert.equal(writes, 1)
})

test('DraftRecovery_MetadataMismatchReportsHTTPAndSafeFieldDifference_023', async () => {
  const fixture = metadataFixture()
  let writes = 0
  await assert.rejects(api().replacePreservedDraftMetadata(async (path, optional, method, body) => {
    if (path === 'branches/main') return { protected: true, commit: { sha } }
    if (path.startsWith('git/ref/')) return null
    if (method === 'PATCH') {
      writes++
      Object.assign(fixture.current, body, { tag_name: 'untagged-dce9f75805136bcd2e47' })
      return api().readDraftApiResponse(new Response(JSON.stringify(fixture.current), { status: 200 }), method)
    }
    return structuredClone(fixture.current)
  }, fixture.snapshot, fixture.binding), error => {
    assert.match(error.message, /PATCH HTTP 200/)
    assert.match(error.message, /tag_name/)
    assert.match(error.message, /untagged-dce9f75805136bcd2e47/)
    assert.doesNotMatch(error.message, /Exact validation disclosure|permission|Bearer/)
    return true
  })
  assert.equal(writes, 1)
})

test('DraftRecovery_PreparedRepairUsesExactBackupAndBindingWithoutReplaying_024', async () => {
  const fixture = metadataFixture()
  Object.assign(fixture.current, { body: fixture.binding.body, tag_name: 'untagged-dce9f75805136bcd2e47' })
  let writes = 0
  const request = async (path, optional, method, body) => {
    if (path === 'branches/main') return { protected: true, commit: { sha } }
    if (path.startsWith('git/ref/')) return null
    if (method === 'PATCH') {
      writes++
      Object.assign(fixture.current, body)
      throw new Error('acknowledgement lost after repair')
    }
    return structuredClone(fixture.current)
  }
  assert.equal(typeof api().repairPreparedDraftMetadata, 'function', 'explicit same-binding repair must be implemented')
  await api().repairPreparedDraftMetadata(request, fixture.snapshot, fixture.binding)
  api().assertPreparedDraft(fixture.current, fixture.binding)
  await api().repairPreparedDraftMetadata(request, fixture.snapshot, fixture.binding)
  assert.equal(writes, 1, 'observed complete state must not repeat the unknown write')
})

test('DraftRecovery_PreparedRepairRejectsSourceRunInventoryOrSnapshotDrift_025', async () => {
  assert.equal(typeof api().repairPreparedDraftMetadata, 'function', 'explicit same-binding repair must be implemented')
  for (const mutate of [
    f => f.current.target_commitish = oldSource,
    f => f.binding.runId++, f => f.binding.attempt++,
    f => f.binding.inventory.push({ name: 'other.bin', size: 1, sha256: 'b'.repeat(64) }),
    f => f.current.body += ' concurrent disclosure', f => f.current.name += ' changed',
    f => f.current.created_at = '2026-10-09T00:00:00Z',
    f => f.current.tag_name = 'v0.18.0', f => f.current.tag_name = 'untagged-invalid',
    f => f.current.assets[0].id++, f => f.current.assets[0].digest = `sha256:${'b'.repeat(64)}`,
    f => f.current.assets[0].label = 'changed', f => f.current.assets[0].content_type = 'changed',
    f => f.current.assets.push({ ...f.current.assets[0], id: 1 }),
    f => f.current.draft = false, f => f.current.published_at = '2026-10-09T00:00:00Z',
    f => f.mainSha = oldSource, f => f.tag = { object: { type: 'commit', sha } },
  ]) {
    const fixture = metadataFixture()
    Object.assign(fixture.current, { body: fixture.binding.body, tag_name: 'untagged-dce9f75805136bcd2e47' })
    mutate(fixture)
    let writes = 0
    await assert.rejects(api().repairPreparedDraftMetadata(async (path, optional, method) => {
      if (path === 'branches/main') return { protected: true, commit: { sha: fixture.mainSha ?? sha } }
      if (path.startsWith('git/ref/')) return fixture.tag ?? null
      if (method === 'PATCH') writes++
      return structuredClone(fixture.current)
    }, fixture.snapshot, fixture.binding))
    assert.equal(writes, 0)
  }
})

test('DraftRecovery_UntaggedOriginalDraftRemainsAnUnusedVersionConflict_026', () => {
  const draft = oldDraft(); draft.tag_name = 'untagged-dce9f75805136bcd2e47'
  assert.equal(api().recoveryCandidateChecks({ ...context(), releaseExists: false }, [draft], null), false)
})

test('DraftRecovery_HTTPReceiptsKeepConcreteSuccessAndFailureStatus_027', async () => {
  assert.equal(typeof api().readDraftApiResponse, 'function', 'HTTP response decoder must preserve receipt status')
  const receipt = await api().readDraftApiResponse(new Response('{"id":406663556}', { status: 200 }), 'PATCH')
  assert.equal(receipt[api().API_HTTP_STATUS], 200)
  assert.equal(JSON.stringify(receipt), '{"id":406663556}', 'receipt metadata must not change snapshot comparison')
  await assert.rejects(api().readDraftApiResponse(new Response('{}', { status: 403 }), 'PATCH'), error => {
    assert.equal(error.status, 403); assert.match(error.message, /PATCH API HTTP 403/); return true
  })
})

function reviewedOriginalSnapshot() {
  const snapshot = structuredClone(reviewedPrepared.release)
  Object.assign(snapshot, { tag_name: 'v0.18.1', target_commitish: oldSource, body: 'Original backed-up disclosure' })
  snapshot.assets = snapshot.assets.map(asset => ({ ...asset, name: assets.find(a => a[0] === asset.id)[1], label: null }))
  return snapshot
}

test('DraftRecovery_ReviewedPartialStateAdmitsFreshCurrentSourceCandidate_028', () => {
  assert.equal(api().recoveryCandidateChecks(context(), [reviewedPrepared.release], null), true)
  for (const mutate of [d => d.body += ' changed', d => d.assets[0].content_type = 'changed',
    d => d.assets[0].size++, d => d.target_commitish = sha, d => d.created_at = 'changed',
    d => d.assets.push({ ...d.assets[0], id: 1 }), d => d.tag_name = 'other']) {
    const changed = structuredClone(reviewedPrepared.release); mutate(changed)
    assert.equal(api().recoveryCandidateChecks(context(), [changed], null), false)
  }
  assert.equal(api().recoveryCandidateChecks({ ...context(), ci: { ...context().ci, conclusion: 'failure' } }, [reviewedPrepared.release], null), false)
})

test('DraftRecovery_ReprepareReviewedStateRebindsOnlyFreshSourceAndInventory_029', async () => {
  assert.equal(typeof api().reprepareReviewedDraft, 'function', 'reviewed partial-state transition must be implemented')
  const snapshot = reviewedOriginalSnapshot(), binding = metadataFixture().binding
  let current = structuredClone(reviewedPrepared.release), writes = 0
  const request = async (path, optional, method, body) => {
    if (path === 'branches/main') return { protected: true, commit: { sha } }
    if (path.startsWith('git/ref/')) return null
    if (method === 'PATCH') {
      writes++; current = { ...current, ...body }; throw new Error('lost new preparation acknowledgement')
    }
    return structuredClone(current)
  }
  await api().reprepareReviewedDraft(request, snapshot, binding)
  api().assertPreparedDraft(current, binding)
  assert.deepEqual(current.assets, reviewedPrepared.release.assets)
  assert.equal(current.created_at, snapshot.created_at)
  assert.equal(current.target_commitish, sha)
  assert.doesNotMatch(current.body, /37899773504|37899773556|db517757/)
  await api().reprepareReviewedDraft(request, snapshot, binding)
  assert.equal(writes, 1)
})

test('DraftRecovery_ReprepareRefusesOldEvidenceOrConcurrentChanges_030', async () => {
  assert.equal(typeof api().reprepareReviewedDraft, 'function', 'reviewed partial-state transition must be implemented')
  for (const mutate of [f => f.current.body += ' changed', f => f.current.assets[0].digest = 'changed',
    f => f.snapshot.created_at = 'changed', f => f.snapshot.assets[0].content_type = 'changed',
    f => f.binding.body = reviewedPrepared.release.body,
    f => f.binding.runId = reviewedPrepared.preparation.runId,
    f => f.mainSha = reviewedPrepared.preparation.sha, f => f.tag = { object: { type: 'commit', sha } },
    f => f.current.assets.push({ ...f.current.assets[0], id: 1 })]) {
    const fixture = { snapshot: reviewedOriginalSnapshot(), binding: metadataFixture().binding,
      current: structuredClone(reviewedPrepared.release) }
    mutate(fixture); let writes = 0
    await assert.rejects(api().reprepareReviewedDraft(async (path, optional, method) => {
      if (path === 'branches/main') return { protected: true, commit: { sha: fixture.mainSha ?? sha } }
      if (path.startsWith('git/ref/')) return fixture.tag ?? null
      if (method === 'PATCH') writes++
      return structuredClone(fixture.current)
    }, fixture.snapshot, fixture.binding))
    assert.equal(writes, 0)
  }
})

test('DraftRecovery_ReviewedOldBackupHasIndependentImmutableProvenance_031', () => {
  assert.equal(typeof api().validatePreparedRecoveryBackup, 'function', 'immutable old-run backup validation must be implemented')
  const b = reviewedPrepared.backup
  const artifact = { id: b.id, name: b.name, digest: b.digest, size_in_bytes: b.size, expired: false,
    expires_at: '2099-01-01T00:00:00Z', workflow_run: { id: b.runId, head_sha: b.sha, head_branch: 'main' } }
  const run = { id: b.runId, head_sha: b.sha, head_branch: 'main', run_attempt: b.attempt,
    path: '.github/workflows/release.yml', event: 'push', status: 'completed', conclusion: 'failure' }
  assert.doesNotThrow(() => api().validatePreparedRecoveryBackup(artifact, run))
  for (const mutate of [a => a.id++, a => a.name += '-other', a => a.digest = 'changed', a => a.size_in_bytes++,
    a => a.expired = true, a => a.expires_at = '2020-01-01T00:00:00Z', a => a.workflow_run.head_sha = sha]) {
    const changed = structuredClone(artifact); mutate(changed)
    assert.throws(() => api().validatePreparedRecoveryBackup(changed, run))
  }
  assert.throws(() => api().validatePreparedRecoveryBackup(artifact, { ...run, run_attempt: 2 }))
  assert.throws(() => api().validatePreparedRecoveryBackup(artifact, { ...run, head_sha: sha }))
})

test('DraftRecovery_WorkflowRepreparesWithOldBackupAndNewSignedRun_032', () => {
  const workflow = readFileSync(new URL('../../.github/workflows/release.yml', import.meta.url), 'utf8')
  const transaction = readFileSync(new URL('../../scripts/release-draft-transaction.mjs', import.meta.url), 'utf8')
  assert.match(workflow, /run-id: \$\{\{ steps\.promotion\.outputs\.recovery_backup_run \}\}/)
  assert.match(workflow, /artifact-ids: \$\{\{ steps\.promotion\.outputs\.recovery_backup_id \}\}/)
  assert.equal((workflow.match(/recovery_mode == 'original'/g) ?? []).length, 4)
  assert.equal((workflow.match(/recovery_mode == 'reprepare'/g) ?? []).length, 2)
  assert.ok(workflow.indexOf('Generate updater manifest and verify all three actual signatures') < workflow.indexOf('reprepare --directory'))
  assert.match(transaction, /validatePreparedRecoveryBackup\(artifact, await api/)
  assert.match(transaction, /verifyBackup\(options.get\('--directory'\), provenance\)/)
  assert.ok(transaction.indexOf('await verifyUpdaterManifest') < transaction.indexOf('await reprepareReviewedDraft'))
})
