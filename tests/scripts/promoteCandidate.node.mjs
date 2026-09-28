import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { buildCandidateManifest } from '../../scripts/native-cli/build-candidate-manifest.mjs'
import { preparePromotion } from '../../scripts/native-cli/promote-candidate.mjs'

const catalog = JSON.parse(readFileSync(
  new URL('../../docs/testing/native-cli-acceptance-catalog.json', import.meta.url),
  'utf8',
))

function sha(value) {
  return createHash('sha256').update(value).digest('hex')
}

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'cc-desk-promotion-'))
  const repoRoot = join(root, 'repo')
  const candidateRoot = join(root, 'candidate')
  const evidenceRoot = join(root, 'evidence')
  const publishDir = join(root, 'publish')
  mkdirSync(repoRoot)
  mkdirSync(candidateRoot)
  mkdirSync(evidenceRoot)
  writeFileSync(join(repoRoot, 'package.json'), JSON.stringify({ version: '0.17.7' }))

  const sourceCommit = 'a'.repeat(40)
  const partitions = [
    [`cc-desk-candidate-${sourceCommit}-macos`, ['CC.Desk_aarch64.dmg', 'CC.Desk.app.tar.gz', 'CC.Desk.app.tar.gz.sig']],
    [`cc-desk-candidate-${sourceCommit}-linux`, ['CC.Desk.AppImage', 'CC.Desk.AppImage.sig']],
    [`cc-desk-candidate-${sourceCommit}-windows`, ['CC.Desk-setup.exe', 'CC.Desk-setup.exe.sig']],
  ]
  for (const [directory, files] of partitions) {
    const path = join(candidateRoot, directory)
    mkdirSync(path)
    for (const file of files) writeFileSync(join(path, file), `${directory}/${file}\n`)
  }

  const candidate = buildCandidateManifest(candidateRoot, sourceCommit)
  const windows = candidate.files.find(file => file.path.endsWith('CC.Desk-setup.exe'))
  const proof = Buffer.from('installed candidate proof\n')
  writeFileSync(join(evidenceRoot, 'proof.txt'), proof)

  const plan = {
    schemaVersion: 1,
    claim: 'installed-native-release',
    candidateId: candidate.candidateId,
    targets: [{
      targetId: 'windows-x64-codex-release',
      os: 'windows',
      osBuild: 'synthetic-build',
      arch: 'x86_64',
      executionDomain: 'local',
      candidateFilePath: windows.path,
      packageSha256: windows.sha256,
      cli: {
        kind: 'codex',
        version: 'synthetic-version',
        binarySha256: 'c'.repeat(64),
      },
      requiredCaseIds: catalog.cases.map(item => item.caseId),
    }],
  }

  const records = []
  for (const item of catalog.cases) {
    for (const subcaseId of item.subcaseIds) {
      records.push({
        schemaVersion: 2,
        specVersion: 2,
        targetId: 'windows-x64-codex-release',
        caseId: item.caseId,
        subcaseId,
        runId: `run-${item.caseId}-${subcaseId}`,
        status: 'PASS',
        evidenceLayer: 'D',
        cliBinarySha256: 'c'.repeat(64),
        deskPackageSha256: windows.sha256,
        evidence: [{ path: 'proof.txt', sha256: sha(proof) }],
        nonApplicabilityReason: null,
      })
    }
  }

  return {
    root,
    repoRoot,
    candidateRoot,
    evidenceRoot,
    publishDir,
    sourceCommit,
    candidate,
    plan,
    records,
    windows,
  }
}

function promote(value) {
  return preparePromotion({
    repoRoot: value.repoRoot,
    candidateRoot: value.candidateRoot,
    candidate: value.candidate,
    catalog,
    plan: value.plan,
    records: value.records,
    evidenceRoot: value.evidenceRoot,
    expectedSourceCommit: value.sourceCommit,
    tag: 'v0.17.7',
    publishDir: value.publishDir,
  })
}

test('D30_Promotion_CopiesOnlyVerifiedCandidateBytes_01', () => {
  const value = fixture()
  const result = promote(value)
  assert.equal(result.promotion.status, 'READY_FOR_PROMOTION')
  assert.equal(result.promotion.candidateId, value.candidate.candidateId)
  assert.equal(result.promotion.files.length, value.candidate.files.length)
  const published = readFileSync(join(value.publishDir, 'CC.Desk-setup.exe'))
  assert.equal(sha(published), value.windows.sha256)
  assert.equal(result.acceptanceSummary.status, 'PASS')
})

test('D30_Promotion_ModifiedCandidateByteFailsBeforePublish_02', () => {
  const value = fixture()
  const file = join(value.candidateRoot, value.windows.path)
  writeFileSync(file, 'tampered\n')
  assert.throws(() => promote(value), /PROMOTION_CANDIDATE_HASH_MISMATCH/)
})

test('D30_Promotion_ExtraCandidateFileFailsClosed_03', () => {
  const value = fixture()
  writeFileSync(join(value.candidateRoot, 'unexpected.txt'), 'unexpected')
  assert.throws(() => promote(value), /PROMOTION_CANDIDATE_FILE_SET_MISMATCH/)
})

test('D30_Promotion_IncompleteAcceptanceCannotPromote_04', () => {
  const value = fixture()
  value.records.pop()
  assert.throws(() => promote(value), /PROMOTION_ACCEPTANCE_REJECTED/)
})

test('D30_Promotion_SourceCommitAndVersionAreImmutable_05', () => {
  const value = fixture()
  assert.throws(() => preparePromotion({
    repoRoot: value.repoRoot,
    candidateRoot: value.candidateRoot,
    candidate: value.candidate,
    catalog,
    plan: value.plan,
    records: value.records,
    evidenceRoot: value.evidenceRoot,
    expectedSourceCommit: 'b'.repeat(40),
    tag: 'v0.17.7',
    publishDir: value.publishDir,
  }), /PROMOTION_SOURCE_COMMIT_MISMATCH/)

  const other = fixture()
  assert.throws(() => preparePromotion({
    repoRoot: other.repoRoot,
    candidateRoot: other.candidateRoot,
    candidate: other.candidate,
    catalog,
    plan: other.plan,
    records: other.records,
    evidenceRoot: other.evidenceRoot,
    expectedSourceCommit: other.sourceCommit,
    tag: 'v0.17.8',
    publishDir: other.publishDir,
  }), /PROMOTION_VERSION_MISMATCH/)
})
