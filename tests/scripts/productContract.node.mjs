import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

function read(relative) {
  return readFileSync(fileURLToPath(new URL(`../../${relative}`, import.meta.url)), 'utf8')
}

test('D31_ProductMetadata_DeclaresBothNativeCLIs_01', () => {
  const pkg = JSON.parse(read('package.json'))
  const tauri = JSON.parse(read('src-tauri/tauri.conf.json'))

  assert.match(pkg.description, /Claude Code/)
  assert.match(pkg.description, /Codex CLI/)
  assert.ok(pkg.keywords.includes('codex'))
  assert.ok(pkg.keywords.includes('openai-codex'))
  assert.equal(pkg.scripts['release:oss'], undefined)

  assert.match(tauri.bundle.shortDescription, /Claude Code/)
  assert.match(tauri.bundle.shortDescription, /Codex CLI/)
  assert.match(tauri.bundle.longDescription, /Claude Code/)
  assert.match(tauri.bundle.longDescription, /Codex CLI/)
})

test('D31_Readmes_NoLongerDescribeProductAsClaudeOnly_02', () => {
  const english = read('README.md')
  const chinese = read('README_CN.md')
  for (const source of [english, chinese]) {
    assert.match(source, /Claude Code/)
    assert.match(source, /Codex CLI/)
    assert.match(source, /Native CLI/)
  }
})

test('D31_HandoffContract_DistinguishesCodeFromCertification_03', () => {
  const contract = read('docs/native-cli-product-contract.md')
  assert.match(contract, /not a certification result/i)
  assert.match(contract, /certificationStatus: NOT_RUN/)
  assert.match(contract, /native-release-promotion/)
  assert.match(contract, /Do not rebuild/)
  assert.match(contract, /BLOCKED/)
})
