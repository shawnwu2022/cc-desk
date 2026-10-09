import assert from 'node:assert/strict'
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync, copyFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createHash, generateKeyPairSync, sign } from 'node:crypto'
import { deflateRawSync } from 'node:zlib'
import test from 'node:test'
import * as policy from '../../scripts/release-policy.mjs'
import { VALIDATION_POLICY, REPORT_FILENAME, validateNativeCoverage } from '../../scripts/windows-native-validation.mjs'
import { crc32, readCoverageZip, fetchCoverageArchive, MAX_ARCHIVE_BYTES } from '../../scripts/release-coverage-archive.mjs'
import { OLD_ASSETS, OLD_SOURCE, OLD_DRAFT_ID, PRESERVED_LABEL, preservedAssetName, stageMarker, validationNotes, stagedValidationNotes } from '../../scripts/release-draft-recovery.mjs'
import { buildUpdaterManifest } from '../../scripts/generate-updater-manifest.js'

const sha = '1'.repeat(40)
const ci = { id: 7, run_attempt: 2, head_sha: sha, head_branch: 'main' }
const name = `windows-native-coverage-${sha}-7-2`
function artifact() {
  return { id: 90, name, expired: false, expires_at: '2099-01-01T00:00:00Z',
    workflow_run: { id: 7, head_sha: sha, head_branch: 'main' } }
}
const scope = JSON.parse(readFileSync(new URL('../../scripts/windows-native-scope.json', import.meta.url)))
const zero = { exitCode: 0, passed: 0, failed: 0, ignored: 0, measured: 0, filteredOut: 0 }
function report() {
  return { schema: 1, policy: VALIDATION_POLICY, completed: true, sourceSha: sha, runId: '7', runAttempt: 2,
    host: { jobQuerySucceeded: true, inJob: true },
    harnesses: scope.harnesses.map((identity, index) => {
      const full = index === 0 ? [...scope.jobFreeTests, ...scope.requiredSelectedTests, 'ordinary_new', 'worker']
        : index === 1 ? [] : [`${identity.name}_worker`]
      const ignored = index === 1 ? [] : [full.at(-1)]
      const excluded = index === 0 ? [...scope.jobFreeTests] : []
      const selected = full.filter(name => !excluded.includes(name))
      return { identity, full: full.map(name => ({ name, type: 'test' })), ignored, excluded, selected,
        defaultIgnored: ignored.map(name => ({ name, reason: null, classification: 'original-default-ignore' })),
        logs: Object.fromEntries(['full', 'ignored', 'selected', 'execution'].map(phase => [phase, `logs/${identity.name}-${phase}.log`])),
        result: { ...zero, passed: selected.length - ignored.length, ignored: ignored.length, filteredOut: excluded.length } }
    }),
    doctests: { command: 'cargo test --locked --doc', exitCode: 0, result: { ...zero }, logs: ['logs/doctests.log'] },
    nativeJobSuite: { status: 'unverified', reason: 'external_job', unverifiedNames: [...scope.jobFreeTests] },
    nativeAll: { status: 'unverified', reason: 'original_all_not_run' }, nativeAcceptanceProven: false }
}
function binding() { return { sourceSha: sha, runId: '7', runAttempt: 2, artifactId: 90, artifactName: name } }
function context() {
  return { event: 'workflow_dispatch', ref: 'refs/heads/main', sha, main: { sha, protected: true },
    versions: ['1.2.3', '1.2.3', '1.2.3'], tag: 'v1.2.3', tagExists: false, releaseExists: false,
    ci: { ...ci, event: 'push', status: 'completed', conclusion: 'success', path: '.github/workflows/ci.yml' },
    jobs: policy.REQUIRED_CI_JOBS.map(name => ({ name, status: 'completed', conclusion: 'success' })),
    coverage: { artifact: artifact(), binding: binding(), report: report() } }
}
function writeFixture(directory, value) {
  mkdirSync(join(directory, 'logs'))
  const listing = records => records.map(record => `${record.name}: ${record.type}`).join('\n') +
    `\n${records.filter(r => r.type === 'test').length} tests, ${records.filter(r => r.type === 'benchmark').length} benchmarks\n`
  const execution = r => `test result: ${r.failed ? 'FAILED' : 'ok'}. ${r.passed} passed; ${r.failed} failed; ${r.ignored} ignored; ${r.measured} measured; ${r.filteredOut} filtered out; finished in 0.00s\n`
  for (const h of value.harnesses) {
    writeFileSync(join(directory, h.logs.full), listing(h.full))
    writeFileSync(join(directory, h.logs.ignored), listing(h.full.filter(r => h.ignored.includes(r.name))))
    writeFileSync(join(directory, h.logs.selected), listing(h.full.filter(r => h.selected.includes(r.name))))
    writeFileSync(join(directory, h.logs.execution), h.defaultIgnored.map(i => `test ${i.name} ... ignored${i.reason ? `, ${i.reason}` : ''}\n`).join('') + execution(h.result))
  }
  writeFileSync(join(directory, value.doctests.logs[0]), execution(value.doctests.result))
  writeFileSync(join(directory, REPORT_FILENAME), JSON.stringify(value))
}
function zip(entries, { method = 8, descriptor = true } = {}) {
  const chunks = [], directory = []
  let offset = 0
  for (const [name, bytes] of entries) {
    const filename = Buffer.from(name), packed = method === 8 ? deflateRawSync(bytes) : bytes, crc = crc32(bytes)
    const local = Buffer.alloc(30), central = Buffer.alloc(46), tail = descriptor ? Buffer.alloc(16) : Buffer.alloc(0)
    local.writeUInt32LE(0x04034b50); local.writeUInt16LE(20, 4); local.writeUInt16LE(descriptor ? 8 : 0, 6); local.writeUInt16LE(method, 8)
    local.writeUInt16LE(filename.length, 26)
    if (descriptor) { tail.writeUInt32LE(0x08074b50); tail.writeUInt32LE(crc, 4); tail.writeUInt32LE(packed.length, 8); tail.writeUInt32LE(bytes.length, 12) }
    else { local.writeUInt32LE(crc, 14); local.writeUInt32LE(packed.length, 18); local.writeUInt32LE(bytes.length, 22) }
    central.writeUInt32LE(0x02014b50); central.writeUInt16LE(20, 6); central.writeUInt16LE(descriptor ? 8 : 0, 8); central.writeUInt16LE(method, 10)
    central.writeUInt32LE(crc, 16); central.writeUInt32LE(packed.length, 20); central.writeUInt32LE(bytes.length, 24)
    central.writeUInt16LE(filename.length, 28); central.writeUInt32LE(offset, 42)
    const block = Buffer.concat([local, filename, packed, tail]); chunks.push(block); offset += block.length
    directory.push(Buffer.concat([central, filename]))
  }
  const central = Buffer.concat(directory), end = Buffer.alloc(22)
  end.writeUInt32LE(0x06054b50); end.writeUInt16LE(entries.length, 8); end.writeUInt16LE(entries.length, 10)
  end.writeUInt32LE(central.length, 12); end.writeUInt32LE(offset, 16)
  return Buffer.concat([...chunks, central, end])
}
function preflight({ mutate = () => {}, mutateReport = () => {}, mutateLocal = () => {}, args, extraArgs = [], missingLog = false, stage = false, inPlace = true, preparedPhase = false, omitNotes = false } = {}) {
  const directory = mkdtempSync(join(tmpdir(), 'ccdesk-release-coverage-'))
  try {
    const c = context(), value = report()
    const api = { ci: c.ci, jobs: c.jobs, artifacts: [artifact()], main: c.main, releases: [], tagExists: false,
      releaseRun: { head_sha: sha, head_branch: 'main', path: '.github/workflows/release.yml' },
      platformArtifacts: ['windows', 'macos', 'linux'].map(platform => ({ name: `cc-desk-candidate-${sha}-${platform}`,
        expired: false, workflow_run: { id: 42, head_sha: sha, head_branch: 'main' } })) }
    mutateReport(value)
    writeFixture(directory, value)
    const entries = [REPORT_FILENAME, ...value.harnesses.flatMap(h => Object.values(h.logs)), ...value.doctests.logs]
      .map(name => [name, readFileSync(join(directory, name))])
    const archive = zip(entries)
    api.artifacts[0].digest = `sha256:${createHash('sha256').update(archive).digest('hex')}`
    if (stage) {
      const { publicKey, privateKey } = generateKeyPairSync('ed25519'), keyId = Buffer.alloc(8, 1)
      const packet = Buffer.concat([Buffer.from('Ed'), keyId, publicKey.export({ type: 'spki', format: 'der' }).subarray(-32)])
      const pubkey = Buffer.from(`untrusted comment: fixture\n${packet.toString('base64')}\n`).toString('base64')
      const platformNames = ['CC.Desk_0.18.1_x64-setup.exe', 'CC.Desk.app.tar.gz', 'CC.Desk_0.18.1_amd64.AppImage']
      const signed = platformNames.map(name => {
        const data = Buffer.from(`new ${name}`), signature = sign(null, createHash('blake2b512').update(data).digest(), privateKey)
        const signedPacket = Buffer.concat([Buffer.from('ED'), keyId, signature]), comment = `timestamp:1\tfile:${name}`
        const global = sign(null, Buffer.concat([signature, Buffer.from(comment)]), privateKey)
        return { name, data, signature: Buffer.from(`untrusted comment: fixture\n${signedPacket.toString('base64')}\ntrusted comment: ${comment}\n${global.toString('base64')}\n`).toString('base64') }
      })
      mkdirSync(join(directory, 'scripts')); mkdirSync(join(directory, 'src-tauri')); mkdirSync(join(directory, 'artifacts'))
      for (const file of ['release-preflight.mjs', 'release-policy.mjs', 'windows-native-validation.mjs', 'windows-native-scope.json',
        'release-coverage-archive.mjs', 'release-draft-recovery.mjs', 'verify-updater-manifest.js', 'updater-signature.js']) {
        copyFileSync(new URL(`../../scripts/${file}`, import.meta.url), join(directory, 'scripts', file))
      }
      writeFileSync(join(directory, 'package.json'), JSON.stringify({ version: '0.18.1' }))
      writeFileSync(join(directory, 'src-tauri/Cargo.toml'), '[package]\nversion = "0.18.1"\n')
      writeFileSync(join(directory, 'src-tauri/tauri.conf.json'), JSON.stringify({ version: '0.18.1', plugins: { updater: { pubkey } } }))
      const stagedFiles = new Map()
      for (const asset of signed) {
        stagedFiles.set(asset.name, asset.data); stagedFiles.set(`${asset.name}.sig`, Buffer.from(asset.signature))
      }
      stagedFiles.set('CC.Desk_0.18.1_aarch64.dmg', Buffer.from('new fixture dmg'))
      for (const [name, bytes] of stagedFiles) writeFileSync(join(directory, 'artifacts', name), bytes)
      const manifest = buildUpdaterManifest({ repository: 'shawnwu2022/cc-desk', tag: 'v0.18.1', assets: signed, pubkey })
      const manifestBytes = Buffer.from(JSON.stringify(manifest)); writeFileSync(join(directory, 'latest.json'), manifestBytes)
      stagedFiles.set('latest.json', manifestBytes); stagedFiles.set(REPORT_FILENAME, readFileSync(join(directory, REPORT_FILENAME)))
      const inventory = [...stagedFiles].map(([name, bytes]) => ({ name, size: bytes.length,
        sha256: createHash('sha256').update(bytes).digest('hex') })).sort((a, b) => a.name.localeCompare(b.name, 'en'))
      api.staged = { id: OLD_DRAFT_ID, name: 'CC Desk 0.18.1', draft: true, published_at: null, prerelease: false, tag_name: 'v0.18.1', target_commitish: sha,
        body: stagedValidationNotes(validationNotes(validateNativeCoverage(value, { sourceSha: sha, runId: '7', runAttempt: 2 }),
          { sourceSha: sha, runId: '7', runAttempt: 2, artifactId: 90, artifactName: name },
          createHash('sha256').update(readFileSync(join(directory, REPORT_FILENAME))).digest('hex')),
          { sha, runId: 42, attempt: 1, tag: 'v0.18.1', inventory }),
        assets: inventory.map((entry, index) => ({ id: 100 + index, name: entry.name, size: entry.size,
          digest: `sha256:${entry.sha256}`, state: 'uploaded', fixtureBytes: stagedFiles.get(entry.name).toString('base64') })) }
      if (inPlace) {
        api.staged.id = OLD_DRAFT_ID
        const originals = OLD_ASSETS.map(a => ({ id: a.id, name: preservedAssetName(a.id), label: PRESERVED_LABEL,
          size: a.size, digest: `sha256:${a.sha256}`, state: 'uploaded' }))
        api.staged.assets = [...originals, ...(preparedPhase ? [] : api.staged.assets)]
      }
      api.releases = [api.staged]
    }
    mutate(api); mutateLocal(directory)
    if (missingLog) rmSync(join(directory, value.harnesses[0].logs.execution))
    const bootstrap = `
      const fixture = JSON.parse(process.env.FIXTURE_API);
      let assetDownloads=0;
      globalThis.fetch = async url => {
        if (url.includes('/releases/assets/')) {
          assetDownloads++;
          const id=Number(url.split('/').at(-1)),asset=fixture.staged.assets.find(a=>a.id===id);
          return new Response(Buffer.from(asset.fixtureBytes,'base64'),{status:200});
        }
        if(url.includes('/actions/artifacts/90/zip')) return new Response(Buffer.from(process.env.FIXTURE_ARCHIVE,'base64'),{status:200});
        if (fixture.queryError && url.includes('/artifacts')) return {ok:false,status:503};
        let value;
        if(url.includes('/branches/main')) value={protected:fixture.main.protected,commit:{sha:fixture.main.sha}};
        else if(url.includes('/actions/workflows/ci.yml/runs')) value={workflow_runs:fixture.runs ?? [fixture.ci]};
        else if(url.includes('/jobs')) value={jobs:fixture.jobs};
        else if(url.includes('/actions/runs/42/artifacts')) value={artifacts:fixture.platformArtifacts};
        else if(url.endsWith('/actions/runs/42')) value=fixture.releaseRun;
        else if(url.includes('/artifacts')) value={artifacts:assetDownloads && fixture.expiredCoverageAfterAssets ? fixture.artifacts.map(a=>({...a,expired:true})) : assetDownloads && fixture.deletedCoverageAfterAssets ? [] : fixture.artifacts};
        else if(url.includes('/git/ref/tags/')) return assetDownloads && fixture.badTagAfterAssets
          ? {ok:true,json:async()=>({object:{type:'commit',sha:'2'.repeat(40)}})} : fixture.tagExists ? {ok:true,json:async()=>({ref:'existing'})} : {status:404};
        else if(url.includes('/releases?')) value=fixture.releases;
        else throw new Error('unexpected fixture API route');
        return {ok:true,json:async()=>value};
      };`
    const selectedArgs = args ?? ['--coverage', join(directory, REPORT_FILENAME), '--ci-run', '7', '--ci-attempt', '2',
      '--coverage-artifact-id', '90', '--coverage-artifact-name', name, ...(omitNotes ? [] : ['--notes', join(directory, 'notes.md')])]
    const result = spawnSync(process.execPath, ['--import', `data:text/javascript;base64,${Buffer.from(bootstrap).toString('base64')}`,
      'scripts/release-preflight.mjs', ...selectedArgs, ...extraArgs], { encoding: 'utf8', timeout: 5000,
      cwd: stage ? directory : fileURLToPath(new URL('../..', import.meta.url)), env: { ...process.env, GITHUB_OUTPUT: join(directory, 'outputs'),
        GITHUB_REPOSITORY: 'shawnwu2022/cc-desk', GITHUB_SHA: sha, GITHUB_REF: 'refs/heads/main',
        GITHUB_EVENT_NAME: 'workflow_dispatch', GITHUB_RUN_ID: '42', GITHUB_RUN_ATTEMPT: '1', GITHUB_TOKEN: 'synthetic-fixture-token',
        FIXTURE_API: JSON.stringify(api), FIXTURE_ARCHIVE: archive.toString('base64') } })
    assert.ifError(result.error)
    return { ...result, notes: result.status === 0 && !args && !omitNotes ? readFileSync(join(directory, 'notes.md'), 'utf8') : null,
      outputs: result.status === 0 ? readFileSync(join(directory, 'outputs'), 'utf8') : null }
  } finally { rmSync(directory, { recursive: true, force: true }) }
}

test('ReleaseCoverage_ExactArtifact_001', () => {
  assert.deepEqual(policy.resolveCoverageArtifact([artifact()], ci, sha), artifact())
})

test('ReleaseCoverage_ReviewedDraftPreparationAuthenticatesCoverage_020', () => {
  const draft = { id: 406663556, tag_name: 'v0.18.1', target_commitish: OLD_SOURCE, draft: true,
    prerelease: false, published_at: null, body: 'old notes', name: 'old candidate',
    assets: OLD_ASSETS.map(a => ({ ...a, digest: `sha256:${a.sha256}`, state: 'uploaded' })) }
  const result = preflight({ extraArgs: ['--prepare-draft-recovery'], mutate: api => { api.releases = [draft] } })
  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stdout, /Read-only recovery candidate preflight passed/)
  for (const mutate of [api => api.releases[0].assets.pop(), api => api.ci.conclusion = 'failure',
    api => api.tagExists = true, api => api.artifacts[0].workflow_run.head_sha = '2'.repeat(40)]) {
    const result = preflight({ extraArgs: ['--prepare-draft-recovery'], mutate: api => { api.releases = [structuredClone(draft)]; mutate(api) } })
    assert.equal(result.status, 1, result.stdout)
  }
  const forged = preflight({ extraArgs: ['--prepare-draft-recovery'], mutate: api => { api.releases = [draft] },
    mutateLocal: directory => { const path = join(directory, REPORT_FILENAME); writeFileSync(path, `${readFileSync(path)}\n`) } })
  assert.equal(forged.status, 1)
  assert.match(forged.stderr, /differs from authenticated archive|path\/size mismatch/)
})
test('ReleaseCoverage_CompleteStagedDraftVerifiesRealSignatures_021', () => {
  const result = preflight({ stage: true, extraArgs: ['--artifacts', '--staged-release-id', String(OLD_DRAFT_ID)],
    args: undefined })
  // --notes is intentionally rejected for staged publication; it cannot rewrite the bound body.
  assert.equal(result.status, 1)
  assert.match(result.stderr, /invalid staged publication preflight/)
  const args = ['--artifacts', '--staged-release-id', String(OLD_DRAFT_ID)]
  // Preserve full authenticated coverage argument generation while omitting only notes.
  const success = preflight({ stage: true, extraArgs: args, omitNotes: true })
  assert.equal(success.status, 0, success.stderr)
  for (const mutate of [api => api.staged.assets.pop(), api => api.staged.target_commitish = OLD_SOURCE,
    api => api.staged.assets.find(asset => asset.name.endsWith('-setup.exe')).fixtureBytes = Buffer.from('tampered payload').toString('base64'),
    api => api.staged.body = api.staged.body.replace(':42:1:', ':43:1:'),
    api => api.staged.body = api.staged.body.replace(/Unverified tests[\s\S]*?Attached /, 'Attached '),
    api => api.staged.name = 'Fully verified 0.18.1',
    api => api.main.sha = OLD_SOURCE, api => api.ci.run_attempt = 3]) {
    const result = preflight({ stage: true, extraArgs: args, omitNotes: true, mutate })
    assert.equal(result.status, 1, result.stdout)
  }
})
test('ReleaseCoverage_StagedFinalStateRechecksCoverageAndTag_022', () => {
  for (const field of ['expiredCoverageAfterAssets', 'deletedCoverageAfterAssets', 'badTagAfterAssets']) {
    const result = preflight({ stage: true, extraArgs: ['--artifacts', '--staged-release-id', String(OLD_DRAFT_ID)], omitNotes: true,
      mutate: api => { api[field] = true } })
    assert.equal(result.status, 1, `${field}: ${result.stdout}`)
    assert.match(result.stderr, /coverage artifact|tag source changed/)
  }
})

test('ReleaseCoverage_InPlaceSameDraftHasExactNewSetAndPreservedOriginals_023', () => {
  const input = { stage: true, inPlace: true, omitNotes: true,
    extraArgs: ['--artifacts', '--staged-release-id', String(OLD_DRAFT_ID)] }
  const good = preflight(input)
  assert.equal(good.status, 0, good.stderr)
  for (const mutate of [f => f.staged.assets[0].name = OLD_ASSETS[0].name,
    f => f.staged.assets[0].digest = `sha256:${'f'.repeat(64)}`,
    f => f.staged.assets.pop(), f => f.staged.target_commitish = OLD_SOURCE]) {
    const bad = preflight({ ...input, mutate })
    assert.equal(bad.status, 1, bad.stdout)
  }
})

test('ReleaseCoverage_PreparedDraftBindsSameRunAndRefusesPartialUploads_024', () => {
  const input = { stage: true, inPlace: true, preparedPhase: true, omitNotes: true,
    extraArgs: ['--artifacts', '--prepared-release-id', String(OLD_DRAFT_ID)] }
  const good = preflight(input)
  assert.equal(good.status, 0, good.stderr)
  for (const mutate of [f => f.staged.body = 'unknown transaction',
    f => f.staged.body = f.staged.body.replace(/Unverified tests[\s\S]*?Attached /, 'Attached '),
    f => f.staged.name = 'Fully verified 0.18.1',
    f => f.staged.assets.push({ id: 999, name: 'unknown' }), f => f.staged.draft = false]) {
    const bad = preflight({ ...input, mutate })
    assert.equal(bad.status, 1, bad.stdout)
  }
})

test('ReleaseCoverage_RecoveryFlagRetainsOrdinaryUnusedVersionPromotion_025', () => {
  const ordinary = { stage: true, extraArgs: ['--prepare-draft-recovery'], mutate: fixture => { fixture.releases = [] },
    mutateLocal: directory => {
      for (const path of ['package.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml']) {
        const file = join(directory, path)
        writeFileSync(file, readFileSync(file, 'utf8').replaceAll('0.18.1', '0.18.2'))
      }
    } }
  const result = preflight(ordinary)
  assert.equal(result.status, 0, result.stderr)
  assert.match(result.outputs, /version=0\.18\.2\n/)
  assert.match(result.outputs, /recovery_draft=\n/)
  const conflict = preflight({ ...ordinary, mutate: fixture => {
    fixture.releases = [{ id: 999, tag_name: 'v0.18.2', draft: true }]
  } })
  assert.equal(conflict.status, 1)
})
test('ReleaseCoverage_MissingDuplicateExpired_002', () => {
  for (const artifacts of [[], [artifact(), artifact()], [{ ...artifact(), expired: true }],
    [{ ...artifact(), expires_at: '2020-01-01T00:00:00Z' }]]) {
    assert.throws(() => policy.resolveCoverageArtifact(artifacts, ci, sha), /coverage artifact/)
  }
})
test('ReleaseCoverage_RunAttemptAndSource_003', () => {
  for (const mutate of [a => { a.name = name.replace('-7-2', '-7-1') },
    a => { a.workflow_run.id = 8 }, a => { a.workflow_run.head_sha = '2'.repeat(40) },
    a => { a.workflow_run.head_branch = 'dev' }, a => { a.id = 0 }]) {
    const value = artifact(); mutate(value)
    assert.throws(() => policy.resolveCoverageArtifact([value], ci, sha), /coverage artifact/)
  }
})
test('ReleaseCoverage_LocalMetadataCannotReplaceResolved_004', () => {
  const expected = { sourceSha: sha, runId: '7', runAttempt: 2, artifactId: 90, artifactName: name }
  assert.doesNotThrow(() => policy.validateCoverageBinding(expected, artifact(), ci, sha))
  for (const mutate of [v => { delete v.artifactId }, v => { v.artifactId = 91 },
    v => { v.runId = '8' }, v => { v.runAttempt = 1 }, v => { v.sourceSha = '2'.repeat(40) },
    v => { v.artifactName = 'arbitrary.json' }]) {
    const value = { ...expected }; mutate(value)
    assert.throws(() => policy.validateCoverageBinding(value, artifact(), ci, sha), /coverage binding/)
  }
})
test('ReleaseCoverage_WorkflowDownloadAndPublication_005', () => {
  const workflow = readFileSync(new URL('../../.github/workflows/release.yml', import.meta.url), 'utf8')
  assert.match(workflow, /--resolve-coverage/)
  assert.match(workflow, /artifact-ids: \$\{\{ steps\.coverage\.outputs\.coverage_artifact_id \}\}/)
  assert.match(workflow, /run-id: \$\{\{ steps\.coverage\.outputs\.ci_run \}\}/)
  assert.match(workflow, /--coverage coverage\/windows-native-coverage\.json/)
  assert.match(workflow, /node scripts\/release-draft-transaction\.mjs prepare/)
  const transaction = readFileSync(new URL('../../scripts/release-draft-transaction.mjs', import.meta.url), 'utf8')
  assert.match(transaction, /readFileSync\('release-validation-notes\.md', 'utf8'\)/)
  assert.match(workflow, /coverage\/windows-native-coverage\.json/)
})
test('ReleaseCoverage_ValidatedDisclosureRequired_006', () => {
  assert.equal(policy.mayPublish(context()), true)
  for (const mutate of [c => { delete c.coverage }, c => { c.coverage.report.policy = 'all-green' },
    c => { c.coverage.report.completed = false }, c => { c.coverage.report.nativeAll.status = 'passed' },
    c => { c.coverage.report.host.jobQuerySucceeded = false }, c => { c.coverage.report.harnesses[0].excluded.push('ordinary_new') },
    c => { c.coverage.report.harnesses[0].result.failed = 1; c.coverage.report.harnesses[0].result.exitCode = 101 },
    c => { c.coverage.report.harnesses[0].full.shift() }, c => { c.coverage.binding.runAttempt = 1 },
    c => { c.main.protected = false }, c => { c.main.sha = '2'.repeat(40) }, c => { c.tagExists = true },
    c => { c.releaseExists = true }, c => { c.versions[2] = '1.2.4' }, c => { c.jobs[1].conclusion = 'failure' },
    c => { c.jobs.pop() }]) {
    const c = context(); mutate(c); assert.equal(policy.mayPublish(c), false)
  }
})
test('ReleaseCoverage_PreflightValidAndHonestNotes_007', () => {
  const result = preflight()
  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stdout, /preflight passed/)
  assert.match(result.notes, /8 executed \(8 passed, 0 failed, 0 measured\); 3 original ignored tests; 18/)
  assert.match(result.notes, /native All: unverified/)
  assert.match(result.notes, /roundtrip acceptance is not proven/)
  assert.match(result.notes, /roundtrip was not executed/)
  assert.match(result.notes, /FreshSettings/)
  assert.match(result.notes, /project-file changes are not rolled back/)
  assert.match(result.notes, /Future packages require their own reviewed/)
  for (const name of scope.jobFreeTests) assert.ok(result.notes.includes(name))
  assert.match(result.notes, /SHA256: [a-f0-9]{64}/)
})
test('ReleaseCoverage_ResolverDoesNotPromote_008', () => {
  const result = preflight({ args: ['--resolve-coverage'] })
  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stdout, /Resolved native coverage artifact/)
  assert.doesNotMatch(result.stdout, /preflight passed|promotion passed/i)
  assert.equal(result.outputs, `ci_run=7\nci_attempt=2\ncoverage_artifact_id=90\ncoverage_artifact_name=${name}\n`)
})
test('ReleaseCoverage_PreflightRechecksLatestAndArtifact_009', () => {
  for (const mutate of [a => { a.ci.run_attempt = 3; a.artifacts[0].name = name.replace('-7-2', '-7-3') },
    a => { a.runs = [a.ci, { ...a.ci, id: 8 }]; a.artifacts[0].name = name.replace('-7-2', '-8-2'); a.artifacts[0].workflow_run.id = 8 },
    a => { a.artifacts.push(artifact()) }, a => { a.artifacts[0].expired = true }, a => { a.queryError = true },
    a => { a.artifacts = [] }, a => { a.main.sha = '2'.repeat(40) }, a => { a.tagExists = true },
    a => { a.releases = [{tag_name:`v${JSON.parse(readFileSync(new URL('../../package.json', import.meta.url))).version}`,draft:true}] }]) {
    const result = preflight({ mutate }); assert.equal(result.status, 1, result.stdout)
    assert.doesNotMatch(result.stdout, /preflight passed/)
  }
})
test('ReleaseCoverage_PreflightRejectsInvalidScopeOrLogs_010', () => {
  for (const mutateReport of [r => { r.host.jobQuerySucceeded = false }, r => { r.harnesses[0].excluded.push('ordinary_new') },
    r => { r.harnesses[0].result.failed = 1; r.harnesses[0].result.exitCode = 101 },
    r => { r.harnesses[0].full.shift() }, r => { r.nativeAll.status = 'passed' }, r => { r.nativeAcceptanceProven = true }]) {
    assert.equal(preflight({ mutateReport }).status, 1)
  }
  assert.equal(preflight({ missingLog: true }).status, 1)
})
test('ReleaseCoverage_PreflightCannotUseLocalJSONAloneOrWaivers_011', () => {
  for (const args of [[], ['--skip-ci'], ['--resolve-coverage', '--artifacts'], ['--coverage', 'arbitrary.json']]) {
    const result = preflight({ args }); assert.equal(result.status, 1); assert.doesNotMatch(result.stdout, /preflight passed/)
  }
})
test('ReleaseCoverage_CIReportAndReleasePlatformRunsStayDistinct_012', () => {
  assert.equal(preflight({ extraArgs: ['--artifacts'] }).status, 0)
  for (const mutate of [a => { a.platformArtifacts[0].workflow_run.id = 7 },
    a => { a.platformArtifacts.pop() }, a => { a.platformArtifacts.push(artifact()) },
    a => { a.releaseRun.path = '.github/workflows/ci.yml' }, a => { a.releaseRun.head_sha = '2'.repeat(40) }]) {
    assert.equal(preflight({ mutate, extraArgs: ['--artifacts'] }).status, 1)
  }
})
test('ReleaseCoverage_CallerForgeryCannotSubstituteArtifactBytes_013', () => {
  for (const mutateLocal of [directory => {
    const filename = join(directory, REPORT_FILENAME), value = JSON.parse(readFileSync(filename))
    value.host.additionalLocalClaim = 'caller-created'; writeFileSync(filename, JSON.stringify(value))
  }, directory => {
    const filename = join(directory, 'logs/cc_desk-execution.log')
    writeFileSync(filename, readFileSync(filename, 'utf8') + 'caller-created extra log bytes\n')
  }]) {
    const result = preflight({ mutateLocal })
    assert.equal(result.status, 1)
    assert.match(result.stderr, /local publication copy.*(?:mismatch|differs)/)
  }
})
test('ReleaseCoverage_ArchiveDigestIsAuthenticated_014', () => {
  for (const digest of ['sha256:' + '0'.repeat(64), 'sha1:' + '0'.repeat(40)]) {
    const result = preflight({ mutate: a => { a.artifacts[0].digest = digest } })
    assert.equal(result.status, 1); assert.match(result.stderr, /digest/)
  }
})
test('ReleaseCoverage_ArchiveFormatsAndCRC_015', () => {
  assert.equal(crc32(Buffer.from('123456789')), 0xcbf43926)
  for (const options of [{ method: 8, descriptor: true }, { method: 8, descriptor: false }, { method: 0, descriptor: false }]) {
    const entries = [[REPORT_FILENAME, Buffer.from('{"fixture":true}')], ['logs/result.log', Buffer.from('test output\n')]]
    const files = readCoverageZip(zip(entries, options))
    for (const [name, bytes] of entries) assert.deepEqual(files.get(name), bytes)
  }
  const bytes = zip([[REPORT_FILENAME, Buffer.from('{"fixture":true}')]], { method: 0, descriptor: false })
  bytes[30 + REPORT_FILENAME.length] ^= 1
  assert.throws(() => readCoverageZip(bytes), /CRC/)
})
test('ReleaseCoverage_IndependentDotNetArchive_019', () => {
  const bytes = readFileSync(new URL('../fixtures/release-coverage-archive/dotnet-deflate.zip', import.meta.url))
  const files = readCoverageZip(bytes)
  assert.equal(files.size, 2)
  assert.equal(files.get(REPORT_FILENAME).toString(), '{"fixture":"independent-dotnet-ziparchive"}')
  assert.equal(files.get('logs/result.log').toString(), 'independent fixture output\n')
})
test('ReleaseCoverage_UnsafeZipEntriesAndBounds_016', () => {
  for (const name of ['../windows-native-coverage.json', '/windows-native-coverage.json', 'C:/data.json',
    'logs/../secret.log', 'logs/subdir/result.log', 'unrelated.txt']) {
    assert.throws(() => readCoverageZip(zip([[name, Buffer.from('fixture')]])), /path/)
  }
  assert.throws(() => readCoverageZip(zip([[REPORT_FILENAME, Buffer.from('{}')], [REPORT_FILENAME, Buffer.from('{}')]])), /duplicate/)
  const base = zip([[REPORT_FILENAME, Buffer.from('{}')]], { descriptor: false })
  const central = base.indexOf(Buffer.from('504b0102', 'hex'))
  for (const mutate of [b => { b.writeUInt32LE((0xa000 << 16) >>> 0, central + 38) },
    b => { b.writeUInt16LE(1, central + 8) }, b => { b.writeUInt32LE(0xffffffff, central + 24) },
    b => { b.writeUInt32LE(5 * 1024 * 1024, central + 24) }, b => { b.writeUInt32LE(1, central + 42) },
    b => { b[30] ^= 1 }, b => { b.writeUInt16LE(1, b.length - 18) }, b => { b.writeUInt16LE(65, b.length - 12) }]) {
    const value = Buffer.from(base); mutate(value); assert.throws(() => readCoverageZip(value))
  }
})
test('ReleaseCoverage_ArchiveRedirectNeverForwardsToken_017', async () => {
  const bytes = zip([[REPORT_FILENAME, Buffer.from('{}')]]), calls = []
  const request = async (url, options) => {
    calls.push({ url, options })
    return calls.length === 1 ? new Response(null, { status: 302, headers: { location: 'https://productionresultssa0.blob.core.windows.net/fixture?signed=synthetic' } })
      : new Response(bytes, { status: 200 })
  }
  await fetchCoverageArchive({ repository: 'shawnwu2022/cc-desk', token: 'synthetic', artifact: artifact(), request })
  assert.equal(calls[0].url, 'https://api.github.com/repos/shawnwu2022/cc-desk/actions/artifacts/90/zip')
  assert.equal(calls[0].options.headers.Authorization, 'Bearer synthetic')
  assert.equal(calls[0].options.redirect, 'manual')
  assert.deepEqual(calls[1].options.headers, {})
})
test('ReleaseCoverage_ArchiveUnsafeRedirectAndStreamingBound_018', async () => {
  for (const location of ['http://productionresultssa0.blob.core.windows.net/a', 'https://evil.example/a',
    'https://user:password@productionresultssa0.blob.core.windows.net/a', 'https://localhost/a']) {
    await assert.rejects(fetchCoverageArchive({ repository: 'shawnwu2022/cc-desk', token: 'synthetic', artifact: artifact(),
      request: async () => new Response(null, { status: 302, headers: { location } }) }), /redirect/)
  }
  await assert.rejects(fetchCoverageArchive({ repository: 'shawnwu2022/cc-desk', token: 'synthetic', artifact: artifact(),
    request: async () => new Response(Buffer.from('fixture'), { status: 200, headers: { 'content-length': String(MAX_ARCHIVE_BYTES + 1) } }) }), /size/)
  await assert.rejects(fetchCoverageArchive({ repository: 'shawnwu2022/cc-desk', token: 'synthetic', artifact: artifact(),
    request: async () => new Response(new ReadableStream({ start(controller) { controller.enqueue(Buffer.alloc(MAX_ARCHIVE_BYTES + 1)); controller.close() } })) }), /stream exceeds/)
  await assert.rejects(fetchCoverageArchive({ repository: 'shawnwu2022/cc-desk', token: 'synthetic', artifact: artifact(),
    request: async () => new Response(null, { status: 302, headers: { location: 'https://productionresultssa0.blob.core.windows.net/fixture' } }) }), /redirect limit/)
  await assert.rejects(fetchCoverageArchive({ repository: 'shawnwu2022/cc-desk', token: 'synthetic', artifact: artifact(),
    request: async () => { throw new Error('signed-url-or-token-must-not-escape') } }), /^Error: Coverage archive: download request failed$/)
})
