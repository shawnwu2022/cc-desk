const assert = require('node:assert/strict')
const { createHash, generateKeyPairSync, sign } = require('node:crypto')
const { readFileSync, mkdtempSync, mkdirSync, copyFileSync, writeFileSync, rmSync } = require('node:fs')
const { resolve } = require('node:path')
const { tmpdir } = require('node:os')
const { spawnSync } = require('node:child_process')
const test = require('node:test')
const { buildUpdaterManifest } = require('../../scripts/generate-updater-manifest.js')
const { verifyUpdaterManifest } = require('../../scripts/verify-updater-manifest.js')

const sha = '1'.repeat(40)
const names = ['CC.Desk_1.2.3_x64-setup.exe', 'CC.Desk.app.tar.gz', 'CC.Desk_1.2.3_amd64.AppImage']
const { publicKey, privateKey } = generateKeyPairSync('ed25519')
const keyId = Buffer.from('0102030405060708', 'hex')
const publicPacket = Buffer.concat([Buffer.from('Ed'), keyId, publicKey.export({ type: 'spki', format: 'der' }).subarray(-32)])
const pubkey = Buffer.from(`untrusted comment: fixture public key\n${publicPacket.toString('base64')}\n`).toString('base64')
function signedAsset(name, algorithm = 'ED') {
  const data = Buffer.from(`fixture bytes ${name}`)
  const signature = sign(null, algorithm === 'ED' ? createHash('blake2b512').update(data).digest() : data, privateKey)
  const packet = Buffer.concat([Buffer.from(algorithm), keyId, signature])
  const comment = `timestamp:1\tfile:${name}`
  const global = sign(null, Buffer.concat([signature, Buffer.from(comment)]), privateKey)
  const text = `untrusted comment: fixture\n${packet.toString('base64')}\ntrusted comment: ${comment}\n${global.toString('base64')}\n`
  return { name, data, signature: Buffer.from(text).toString('base64') }
}
function input() { return { repository: 'shawnwu2022/cc-desk', tag: 'v1.2.3', assets: names.map(name => signedAsset(name)), pubkey } }

test('ReleaseRecovery_AllThreeSignatures_001', () => {
  const manifest = buildUpdaterManifest(input())
  assert.equal(Object.keys(manifest.platforms).length, 3)
})
test('ReleaseRecovery_DuplicateAndMissingCoverage_002', () => {
  const duplicate = input(); duplicate.assets.push(signedAsset('Other_x64-setup.exe'))
  assert.throws(() => buildUpdaterManifest(duplicate), /exactly one|duplicate/)
  const missing = input(); missing.assets.pop()
  assert.throws(() => buildUpdaterManifest(missing), /missing|exactly one/)
})
for (let index = 0; index < 3; index++) {
  test(`ReleaseRecovery_TamperedPlatform_${index + 3}`, () => {
    const value = input(); value.assets[index].data = Buffer.from('tampered')
    assert.throws(() => buildUpdaterManifest(value), /signature/)
  })
}
test('ReleaseRecovery_KeyAndCommentRefusal_006', () => {
  const value = input()
  const text = Buffer.from(value.assets[0].signature, 'base64').toString().replace('timestamp:1', 'timestamp:2')
  value.assets[0].signature = Buffer.from(text).toString('base64')
  assert.throws(() => buildUpdaterManifest(value), /signature/)
  const wrong = input(); wrong.pubkey = ''
  assert.throws(() => buildUpdaterManifest(wrong), /public key|pubkey/)
})
test('ReleaseRecovery_LegacySignature_007', () => {
  const value = input(); value.assets = names.map(name => signedAsset(name, 'Ed'))
  assert.equal(Object.keys(buildUpdaterManifest(value).platforms).length, 3)
})
async function policy() { return import('../../scripts/release-policy.mjs') }
function context() {
  return { event: 'workflow_dispatch', ref: 'refs/heads/main', sha, main: { sha, protected: true },
    versions: ['1.2.3', '1.2.3', '1.2.3'], tag: 'v1.2.3', tagExists: false, releaseExists: false,
    ci: { id: 7, run_attempt: 2, head_sha: sha, head_branch: 'main', event: 'push', status: 'completed', conclusion: 'success', path: '.github/workflows/ci.yml' },
    jobs: ['Frontend checks', 'Rust checks', 'Disposable roundtrip compile-only policy (no native acceptance)']
      .map(name => ({ name, status: 'completed', conclusion: 'success' })),
  }
}
test('ReleaseRecovery_ExactMainCI_008', async () => {
  const { requiredChecksPassed, mayPublish } = await policy()
  assert.equal(requiredChecksPassed(context()), true)
  assert.equal(mayPublish(context()), false, 'CI success without bound downloaded native coverage cannot promote')
  for (const mutate of [
    v => { v.main.sha = '2'.repeat(40) }, v => { v.main.protected = false },
    v => { v.ref = 'refs/heads/dev' }, v => { v.ci.head_sha = '2'.repeat(40) },
    v => { v.ci.event = 'pull_request' }, v => { v.ci.conclusion = 'failure' },
    v => { v.jobs[1].conclusion = 'failure' }, v => { v.jobs.pop() },
    v => { v.versions[1] = '1.2.4' }, v => { v.tagExists = true },
    v => { v.releaseExists = true }, v => { v.ci.path = '.github/workflows/diagnostic.yml' },
    v => { v.jobs.pop() }, v => { v.ci.run_attempt = 0 },
  ]) { const value = context(); mutate(value); assert.equal(requiredChecksPassed(value), false, JSON.stringify(value)) }
})
test('ReleaseRecovery_ArtifactRunBinding_009', async () => {
  const { validateArtifacts } = await policy()
  const artifacts = ['windows', 'macos', 'linux'].map(platform => ({ name: `cc-desk-candidate-${sha}-${platform}`, expired: false, workflow_run: { id: 42, head_sha: sha, head_branch: 'main' } }))
  assert.doesNotThrow(() => validateArtifacts(artifacts, sha, 42))
  assert.throws(() => validateArtifacts(artifacts.slice(0, 2), sha, 42), /artifact/)
  const mixed = structuredClone(artifacts); mixed[1].workflow_run.id = 43
  assert.throws(() => validateArtifacts(mixed, sha, 42), /artifact/)
})
test('ReleaseRecovery_WorkflowGateWiring_010', () => {
  const workflow = readFileSync(resolve(__dirname, '../../.github/workflows/release.yml'), 'utf8')
  assert.match(workflow, /needs: \[preflight, build\]/)
  assert.match(workflow, /node scripts\/release-preflight\.mjs/)
  assert.match(workflow, /pattern: cc-desk-candidate-\$\{\{ github\.sha \}\}-\*/)
  assert.match(workflow, /node --test tests\/scripts\/releaseRecovery\.node\.cjs/)
})
function releaseJob(name) {
  const workflow = readFileSync(resolve(__dirname, '../../.github/workflows/release.yml'), 'utf8')
  const job = workflow.split(/^  (?=[a-z][a-z0-9-]*:[ \t]*$)/m).find(section => section.startsWith(`${name}:\n`))
  assert.ok(job, `release workflow must contain ${name}`)
  return job
}
test('ReleaseRecovery_PreflightWaitsForSignedBuilds_014', () => {
  const preflight = releaseJob('preflight')
  assert.match(preflight, /^    needs: \[admission, build\]$/m,
    'CI polling must not occupy a runner while the signed builds are still running')
  assert.match(preflight, /^    timeout-minutes: 130$/m)
  assert.match(preflight, /run: node scripts\/release-wait-for-ci\.mjs/)
})
test('ReleaseRecovery_BuildsRemainIndependentOfPreflight_015', () => {
  const build = releaseJob('build')
  assert.match(build, /^    needs: admission$/m,
    'all signed builds must remain parallel with ordinary CI, without a preflight dependency cycle')
  assert.match(build, /^    timeout-minutes: 60$/m)
  assert.match(build, /fail-fast: false/)
})
test('ReleaseRecovery_PublicationRequiresSuccessfulBuildsAndPreflight_016', () => {
  const release = releaseJob('release')
  assert.match(release, /^    needs: \[preflight, build\]$/m)
  assert.match(release, /^    if: github\.ref == 'refs\/heads\/main' && github\.ref_protected$/m)
  for (const name of ['preflight', 'build', 'release']) {
    assert.doesNotMatch(releaseJob(name), /^    (?:continue-on-error:|if:.*(?:always\(|cancelled\(|failure\())/m,
      'failed or cancelled prerequisites must not admit publication')
  }
})
test('ReleaseRecovery_DownloadedSignatures_011', async () => {
  const value = input()
  const manifest = buildUpdaterManifest(value)
  const request = async url => {
    const asset = value.assets.find(asset => url.endsWith(asset.name))
    return { ok: true, arrayBuffer: async () => asset.data }
  }
  await verifyUpdaterManifest(manifest, '1.2.3', pubkey, value.repository, request)
  await assert.rejects(verifyUpdaterManifest(manifest, '1.2.3', pubkey, value.repository,
    async () => ({ ok: true, arrayBuffer: async () => Buffer.from('tampered download') })), /signature/)
  const incomplete = structuredClone(manifest); delete incomplete.platforms['linux-x86_64']
  await assert.rejects(verifyUpdaterManifest(incomplete, '1.2.3', pubkey, value.repository, request), /coverage/)
})
test('ReleaseRecovery_PreflightAPIRefusal_012', () => {
  const version = JSON.parse(readFileSync(resolve(__dirname, '../../package.json'), 'utf8')).version
  const script = `
    const sha = '${sha}';
    const run = { id: 7, run_attempt: 2, head_sha: sha, head_branch: 'main', event: 'push', status: 'completed', conclusion: process.env.FIXTURE_CI, path: '.github/workflows/ci.yml' };
    globalThis.fetch = async url => {
      let value;
      if (url.includes('/branches/main')) value = { protected: true, commit: { sha } };
      else if (url.includes('/actions/workflows/ci.yml/runs')) value = { workflow_runs: [run] };
      else if (url.includes('/actions/runs/7/attempts/2/jobs')) value = { jobs: ['Frontend checks', 'Rust checks', 'Disposable roundtrip compile-only policy (no native acceptance)'].map(name => ({ name, status: 'completed', conclusion: 'success' })) };
      else if (url.includes('/actions/runs/7/artifacts')) value = { artifacts: [{id: 90, name: 'windows-native-coverage-' + sha + '-7-2', expired: false, expires_at: '2099-01-01T00:00:00Z', workflow_run: {id:7, head_sha:sha, head_branch:'main'}}] };
      else if (url.includes('/git/ref/tags/')) return { status: 404 };
      else if (url.endsWith('/releases?per_page=100&page=1')) value = process.env.FIXTURE_DRAFT === 'yes' ? [{ tag_name: 'v${version}', draft: true }] : [];
      else throw new Error('unexpected fixture API route');
      return { ok: true, json: async () => value };
    };
    process.argv = ['node', 'release-preflight.mjs', '--resolve-coverage'];
    await import('./scripts/release-preflight.mjs');
  `
  for (const [conclusion, draft, expected] of [['success', 'no', 0], ['failure', 'no', 1], ['success', 'yes', 1]]) {
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', script], {
      cwd: resolve(__dirname, '../..'), encoding: 'utf8', timeout: 5000,
      env: { ...process.env, GITHUB_REPOSITORY: 'shawnwu2022/cc-desk', GITHUB_SHA: sha, GITHUB_REF: 'refs/heads/main',
        GITHUB_EVENT_NAME: 'workflow_dispatch', GITHUB_TOKEN: 'synthetic-fixture-token', GITHUB_OUTPUT: '', FIXTURE_CI: conclusion, FIXTURE_DRAFT: draft },
    })
    assert.ifError(result.error)
    assert.equal(result.status, expected, result.stderr)
    if (expected === 1) assert.match(result.stderr, /release gate blocked/)
    else { assert.match(result.stdout, /Resolved native coverage artifact/); assert.doesNotMatch(result.stdout, /preflight passed|promotion passed/i) }
  }
})
test('ReleaseRecovery_ActualManifestCLI_013', () => {
  const root = mkdtempSync(resolve(tmpdir(), 'ccdesk-recovery-fixture-'))
  try {
    mkdirSync(resolve(root, 'scripts')); mkdirSync(resolve(root, 'src-tauri')); mkdirSync(resolve(root, 'artifacts'))
    for (const file of ['generate-updater-manifest.js', 'updater-signature.js']) copyFileSync(resolve(__dirname, '../../scripts', file), resolve(root, 'scripts', file))
    writeFileSync(resolve(root, 'src-tauri/tauri.conf.json'), JSON.stringify({ plugins: { updater: { pubkey } } }))
    const value = input()
    for (const asset of value.assets) {
      writeFileSync(resolve(root, 'artifacts', asset.name), asset.data)
      writeFileSync(resolve(root, 'artifacts', `${asset.name}.sig`), asset.signature)
    }
    const result = spawnSync(process.execPath, [resolve(root, 'scripts/generate-updater-manifest.js'), resolve(root, 'artifacts'), resolve(root, 'latest.json'), 'v1.2.3'], {
      encoding: 'utf8', timeout: 5000, env: { ...process.env, GITHUB_REPOSITORY: value.repository },
    })
    assert.ifError(result.error); assert.equal(result.status, 0, result.stderr)
    const manifest = JSON.parse(readFileSync(resolve(root, 'latest.json'), 'utf8'))
    assert.equal(manifest.version, '1.2.3'); assert.equal(Object.keys(manifest.platforms).length, 3)
  } finally { rmSync(root, { recursive: true, force: true }) }
})
