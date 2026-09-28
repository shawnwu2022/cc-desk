import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { buildCandidateManifest } from '../../scripts/native-cli/build-candidate-manifest.mjs'

function candidateFixture() {
  const root = mkdtempSync(join(tmpdir(), 'cc-desk-candidate-'))
  const sha = 'a'.repeat(40)
  for (const [name, files] of [
    [`cc-desk-candidate-${sha}-macos`, ['CC.Desk_aarch64.dmg', 'CC.Desk.app.tar.gz', 'CC.Desk.app.tar.gz.sig']],
    [`cc-desk-candidate-${sha}-linux`, ['CC.Desk.AppImage', 'CC.Desk.AppImage.sig']],
    [`cc-desk-candidate-${sha}-windows`, ['CC.Desk-setup.exe', 'CC.Desk-setup.exe.sig']],
  ]) {
    const dir = join(root, name)
    mkdirSync(dir)
    for (const file of files) writeFileSync(join(dir, file), `${name}/${file}\n`)
  }
  return { root, sha }
}

test('D30_CandidateManifest_BindsAllCandidateBytesAndSourceCommit_01', () => {
  const value = candidateFixture()
  const manifest = buildCandidateManifest(value.root, value.sha)
  assert.match(manifest.candidateId, /^[0-9a-f]{64}$/)
  assert.equal(manifest.sourceCommit, value.sha)
  assert.equal(manifest.files.length, 7)
  assert.deepEqual(
    [...new Set(manifest.files.map(file => `${file.platform}:${file.arch}`))].sort(),
    ['linux:x86_64', 'macos:aarch64', 'windows:x86_64'],
  )
  assert.ok(manifest.files.every(file => /^[0-9a-f]{64}$/.test(file.sha256)))
})

test('D30_CandidateManifest_AnyByteChangeCreatesNewCandidateId_02', () => {
  const value = candidateFixture()
  const before = buildCandidateManifest(value.root, value.sha)
  const windows = join(
    value.root,
    `cc-desk-candidate-${value.sha}-windows`,
    'CC.Desk-setup.exe',
  )
  writeFileSync(windows, 'changed bytes\n')
  const after = buildCandidateManifest(value.root, value.sha)
  assert.notEqual(after.candidateId, before.candidateId)
})

test('D30_CandidateManifest_RejectsUnknownArtifactPartition_03', () => {
  const value = candidateFixture()
  const bad = join(value.root, 'unclassified')
  mkdirSync(bad)
  writeFileSync(join(bad, 'payload.bin'), 'x')
  assert.throws(
    () => buildCandidateManifest(value.root, value.sha),
    /CANDIDATE_PLATFORM_UNKNOWN/,
  )
})


test('D30_CandidateManifest_RejectsSpoofedPartitionName_04', () => {
  const value = candidateFixture()
  renameSync(
    join(value.root, `cc-desk-candidate-${value.sha}-macos`),
    join(value.root, 'spoof-macos'),
  )
  assert.throws(
    () => buildCandidateManifest(value.root, value.sha),
    /CANDIDATE_PLATFORM_(?:UNKNOWN|INCOMPLETE)/,
  )
})

test('D30_CandidateManifest_RejectsMissingRequiredPlatformAsset_05', () => {
  const value = candidateFixture()
  rmSync(join(
    value.root,
    `cc-desk-candidate-${value.sha}-windows`,
    'CC.Desk-setup.exe.sig',
  ))
  assert.throws(
    () => buildCandidateManifest(value.root, value.sha),
    /CANDIDATE_PLATFORM_INCOMPLETE/,
  )
})
