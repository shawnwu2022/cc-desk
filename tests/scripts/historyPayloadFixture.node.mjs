import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createHash } from 'node:crypto'

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
