import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, existsSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'

const root = new URL('../fixtures/version-history-payload/', import.meta.url)
const read = (name) => readFileSync(new URL(name, root))
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex')

// 检查固定官方安装包与签名元数据不包含推断或已批准的 installed payload。
test('pinned v0.17.7 selection requires both exact official asset identities', () => {
  const input = JSON.parse(read('v0.17.7-selection.json'))
  assert.equal(input.id, 392398817)
  assert.equal(input.tag_name, 'v0.17.7')
  assert.equal(input.draft, false)
  assert.equal(input.prerelease, false)
  assert.equal(input.assets.length, 2)
  assert.deepEqual(input.assets.map(({ id, size, digest }) => ({ id, size, digest })), [
    { id: 576637999, size: 4966193, digest: 'sha256:e9ffbc5ba627f0c133a4385db404342a7344729339185e6f9b8ee6b5969086ac' },
    { id: 576637991, size: 420, digest: 'sha256:30b26f21c76d8c30bf4ca042ff699f1dd5d181af54f0e8956a3bff10650b80e2' },
  ])
  for (const asset of input.assets) {
    assert.equal(asset.state, 'uploaded')
    assert.equal(asset.browser_download_url, `https://github.com/shawnwu2022/cc-desk/releases/download/v0.17.7/${asset.name}`)
    assert.ok(asset.created_at && asset.updated_at)
  }
})

// 检查源码快照哈希，避免审查依据无声漂移；源码不等于编译安装包证明。
test('source snapshots match their recorded digests and declared v0.17.7 configuration', () => {
  const provenance = JSON.parse(read('provenance.json'))
  assert.equal(provenance.targetCommit, '77707e3b03187aa2ed96f5ab780f62f14c1e4ffc')
  for (const source of provenance.sources) assert.equal(digest(read(source.file)), source.sha256, source.file)
  const config = JSON.parse(read('v0.17.7-tauri.conf.json'))
  assert.equal(config.productName, 'CC Desk')
  assert.equal(config.version, '0.17.7')
  assert.equal(config.bundle.publisher, 'shawnwu2022')
  assert.equal(config.bundle.windows.nsis.installerHooks, './installer.nsh')
  assert.equal(config.bundle.windows.nsis.installMode, undefined)
  assert.equal(config.bundle.windows.minimumWebview2Version, undefined)
  assert.equal(config.bundle.fileAssociations, undefined)
  assert.equal(config.plugins['deep-link'], undefined)
  const current = JSON.parse(readFileSync(new URL('../../src-tauri/tauri.conf.json', import.meta.url)))
  assert.equal(config.plugins.updater.pubkey, current.plugins.updater.pubkey)
})

const versions = ['0.14.0', '0.15.0', '0.16.0', '0.17.0', '0.17.1', '0.17.2', '0.17.5', '0.17.6', '0.17.7']
const releases = JSON.parse(readFileSync(new URL('../fixtures/version-history-releases.json', import.meta.url)))
const loadCatalog = () => {
  assert.ok(existsSync(new URL('catalog.json', root)), 'the compiled nine-version evidence catalog must exist')
  return JSON.parse(read('catalog.json'))
}

// Every version is an exact selected tuple from the committed observed catalog.
test('all nine compiled fixtures retain both exact release asset tuples', () => {
  const catalog = loadCatalog()
  assert.equal(catalog.schema, 1)
  assert.equal(catalog.releaseCatalogSha256, digest(readFileSync(new URL('../fixtures/version-history-releases.json', import.meta.url))))
  assert.deepEqual(catalog.fixtures.map(f => f.version), versions)
  const ids = new Set()
  for (const fixture of catalog.fixtures) {
    const release = releases.find(r => r.tag_name === `v${fixture.version}`)
    const installerName = `CC.Desk_${fixture.version}_x64-setup.exe`
    const assets = [installerName, `${installerName}.sig`].map(name => release.assets.find(a => a.name === name))
    assert.deepEqual(fixture.selection, { ...release, assets })
    for (const asset of assets) {
      assert.equal(asset.state, 'uploaded')
      assert.match(asset.digest, /^sha256:[0-9a-f]{64}$/)
      assert.ok(asset.size > 0 && asset.size <= 256 * 1024 * 1024)
      assert.equal(asset.browser_download_url, `https://github.com/shawnwu2022/cc-desk/releases/download/v${fixture.version}/${asset.name}`)
      assert.ok(!ids.has(asset.id), 'no asset may be borrowed by another version')
      ids.add(asset.id)
    }
    assert.equal(assets[1].size, 420)
  }
})

// Tagged config bytes and absent overrides are source comparison, never measured output.
test('nine immutable tagged config snapshots bind source provenance and resource shape', () => {
  const catalog = loadCatalog()
  const key = JSON.parse(read('v0.17.7-tauri.conf.json')).plugins.updater.pubkey
  for (const fixture of catalog.fixtures) {
    const provenance = fixture.provenance
    assert.match(provenance.sourceCommit, /^[0-9a-f]{40}$/)
    assert.equal(provenance.tauriCli.version, '2.10.1')
    assert.match(provenance.tauriCli.packageLockSha256, /^[0-9a-f]{64}$/)
    assert.equal(provenance.tauriCli.source, `https://github.com/shawnwu2022/cc-desk/blob/${provenance.sourceCommit}/package-lock.json`)
    for (const source of provenance.sources) {
      assert.equal(digest(read(source.file)), source.sha256)
      assert.equal(source.source, `https://github.com/shawnwu2022/cc-desk/blob/${provenance.sourceCommit}/src-tauri/${source.path}`)
    }
    const config = JSON.parse(read(`v${fixture.version}-tauri.conf.json`))
    assert.equal(config.productName, 'CC Desk')
    assert.equal(config.version, fixture.version)
    assert.equal(config.plugins.updater.pubkey, key)
    assert.equal(config.bundle.publisher, 'shawnwu2022')
    assert.equal(config.bundle.windows.nsis.installerHooks, './installer.nsh')
    assert.equal(config.bundle.windows.nsis.installMode, undefined)
    assert.equal(config.bundle.windows.minimumWebview2Version, undefined)
    assert.equal(config.bundle.fileAssociations, undefined)
    assert.equal(config.plugins['deep-link'], undefined)
    if (fixture.version === '0.17.7') {
      assert.equal(provenance.windowsOverrideAbsent, false)
      assert.equal(provenance.sources.length, 3)
      assert.deepEqual(JSON.parse(read('v0.17.7-tauri.windows.conf.json')).bundle.resources, {
        'conpty/runtime/conpty.dll': 'conpty.dll',
        'conpty/runtime/OpenConsole.exe': 'OpenConsole.exe',
        'conpty/runtime/LICENSE-Microsoft-ConPTY.txt': 'LICENSE-Microsoft-ConPTY.txt',
      })
    } else {
      assert.equal(provenance.windowsOverrideAbsent, true)
      assert.equal(provenance.sources.length, 2)
      assert.equal(config.bundle.resources, undefined)
      assert.equal(provenance.windowsOverrideSource, `https://github.com/shawnwu2022/cc-desk/tree/${provenance.sourceCommit}/src-tauri`)
    }
  }
})

// A portable source contract is not a Windows Rust execution claim.
test('the collector freezes the compiled tuple and rejects cross-case worker bindings before effects', () => {
  const source = readFileSync(new URL('../../src-tauri/src/tests/version_history_payload.rs', import.meta.url), 'utf8')
  assert.ok(source.includes('mod fixture;'), 'collector must consume the compiled fixture catalog')
  assert.ok(source.indexOf('let binding = fixture::Binding::from_environment()') < source.indexOf('let user = CurrentUser::capture()'))
  assert.ok(source.includes('binding.verify_record(root)?'))
  assert.ok(source.includes('"selection-before.json"'))
  assert.ok(source.includes('"selection-after.json"'))
  assert.ok(source.includes('fixture.check_package(package)'))
  assert.ok(source.includes('inventory::check_observations(fixture, case'))
  assert.ok(!source.includes('Some("576637999")'))
  const binding = readFileSync(new URL('../../src-tauri/src/tests/version_history_payload/fixture.rs', import.meta.url), 'utf8')
  assert.ok(binding.includes('env!("CC_DESK_BUILD_SHA")'))
  for (const version of versions) assert.ok(binding.includes(`"${version}"`))
  for (const test of ['HistoryPayload_FixtureCatalog_020', 'HistoryPayload_FixtureMutation_021', 'HistoryPayload_Binding_022', 'HistoryPayload_BindingRecord_023']) assert.ok(binding.includes(`fn ${test}()`))
})

test('payload workflow executes an exact nine by two matrix on independently fresh hosted jobs', () => {
  const workflow = readFileSync(new URL('../../.github/workflows/history-payload-evidence.yml', import.meta.url), 'utf8')
  assert.match(workflow, /fixture_version: \[0\.14\.0, 0\.15\.0, 0\.16\.0, 0\.17\.0, 0\.17\.1, 0\.17\.2, 0\.17\.5, 0\.17\.6, 0\.17\.7\]/)
  assert.match(workflow, /fixture_case: \[clean, seeded-existing\]/)
  assert.match(workflow, /fail-fast: false/)
  assert.match(workflow, /runs-on: windows-2022/)
  assert.match(workflow, /CC_DESK_PAYLOAD_VERSION: \$\{\{ matrix.fixture_version \}\}/)
  assert.match(workflow, /CC_DESK_BUILD_SHA: \$\{\{.*inputs.expected_sha.*\}\}/)
  assert.match(workflow, /cargo test --locked --lib --no-run/)
  assert.match(workflow, /if: \$\{\{ always\(\) \}\}/)
  assert.match(workflow, /name: historical-v\$\{\{ matrix.fixture_version \}\}-\$\{\{ matrix.fixture_case \}\}/)
  assert.ok(!workflow.includes('download-artifact'))
  assert.ok(!workflow.includes('actions/cache'))
  assert.ok(!workflow.includes('SUPPORTED_ROUNDTRIP_ENABLED'))
})

test('the compiled catalog retains the shared reviewed Tauri template bytes', () => {
  const catalog = loadCatalog()
  const template = JSON.parse(read('provenance.json')).sources.find(source => source.file === 'tauri-2.10.1-installer.nsi')
  assert.deepEqual(catalog.nsisTemplate, template)
  assert.equal(digest(read(template.file)), catalog.nsisTemplate.sha256)
})


// Exact-byte provenance must survive Windows checkout line-ending defaults.
test('all hashed catalog and snapshot inputs disable checkout text conversion', () => {
  const paths = [
    'tests/fixtures/version-history-releases.json',
    'tests/fixtures/version-history-payload/catalog.json',
    ...versions.map(version => `tests/fixtures/version-history-payload/v${version}-tauri.conf.json`),
  ]
  for (const path of paths) {
    const attribute = execFileSync('git', ['check-attr', 'text', '--', path], {
      cwd: new URL('../../', import.meta.url), encoding: 'utf8',
    }).trim()
    assert.equal(attribute, `${path}: text: unset`, `preserve exact source bytes for ${path}`)
  }
})
