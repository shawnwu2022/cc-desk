import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import {
  candidateIdFor,
  targetIdentitySha256,
} from '../../scripts/native-cli/verify-acceptance.mjs'
import { stageEvidenceBundle } from '../../scripts/native-cli/stage-evidence-bundle.mjs'

const catalog = JSON.parse(readFileSync(
  new URL('../../docs/testing/native-cli-acceptance-catalog.json', import.meta.url),
  'utf8',
))

function sha(value) {
  return createHash('sha256').update(value).digest('hex')
}

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'cc-desk-stage-evidence-'))
  const evidenceRoot = join(root, 'source-evidence')
  const outDir = join(root, 'staged')
  mkdirSync(evidenceRoot)
  const proof = Buffer.from('synthetic installed evidence\n')
  writeFileSync(join(evidenceRoot, 'proof.txt'), proof)
  writeFileSync(join(evidenceRoot, 'unreferenced-secret.txt'), 'must-not-copy\n')

  const candidate = {
    schemaVersion: 1,
    sourceCommit: 'a'.repeat(40),
    files: [{
      path: 'windows/cc-desk.exe',
      platform: 'windows',
      arch: 'x86_64',
      sha256: 'b'.repeat(64),
    }],
  }
  candidate.candidateId = candidateIdFor(candidate)

  const target = {
    targetId: 'windows-x64-codex-stage',
    os: 'windows',
    osBuild: 'synthetic-build',
    arch: 'x86_64',
    executionDomain: 'local',
    deskVersion: '0.17.7',
    deskCommit: candidate.sourceCommit,
    candidateFilePath: 'windows/cc-desk.exe',
    packageSha256: 'b'.repeat(64),
    webviewRuntime: 'synthetic-webview',
    xtermVersion: '5.5.0',
    renderer: 'dom',
    ptyBackend: 'bundled-conpty',
    launcherKind: 'native',
    shellVersion: 'none',
    inputPolicyVersion: 'd19-v1',
    terminalProtocolVersion: 'd19-v1',
    fixtureConfigVersion: 'acceptance-v2',
    cli: {
      kind: 'codex',
      version: 'synthetic-version',
      binarySha256: 'c'.repeat(64),
    },
    requiredCaseIds: catalog.cases.map(item => item.caseId),
  }
  target.identitySha256 = targetIdentitySha256(target)

  const plan = {
    schemaVersion: 1,
    claim: 'installed-native-release',
    candidateId: candidate.candidateId,
    targets: [target],
  }

  const records = catalog.cases.flatMap(item =>
    item.subcaseIds.map(subcaseId => ({
      schemaVersion: 2,
      specVersion: 2,
      targetId: target.targetId,
      caseId: item.caseId,
      subcaseId,
      runId: `run-${item.caseId}-${subcaseId}`,
      status: 'PASS',
      evidenceLayer: 'D',
      cliBinarySha256: target.cli.binarySha256,
      deskPackageSha256: target.packageSha256,
      fixtureSha256: null,
      expected: { outcome: 'synthetic-expected' },
      actual: { outcome: 'synthetic-expected' },
      evidence: [{ path: 'proof.txt', sha256: sha(proof) }],
      nonApplicabilityReason: null,
    })),
  )

  return { root, evidenceRoot, outDir, candidate, plan, records }
}

test('D29_EvidenceStage_CopiesOnlyGateReferencedFiles_01', () => {
  const value = fixture()
  const summary = stageEvidenceBundle({
    catalog,
    candidate: value.candidate,
    plan: value.plan,
    records: value.records,
    evidenceRoot: value.evidenceRoot,
    outDir: value.outDir,
  })
  assert.equal(summary.status, 'PASS')
  assert.equal(summary.evidenceFileCount, 1)
  assert.ok(existsSync(join(value.outDir, 'plan.json')))
  assert.ok(existsSync(join(value.outDir, 'records.json')))
  assert.ok(existsSync(join(value.outDir, 'evidence', 'proof.txt')))
  assert.equal(existsSync(join(value.outDir, 'evidence', 'unreferenced-secret.txt')), false)
})

test('D29_EvidenceStage_TamperedEvidenceNeverStages_02', () => {
  const value = fixture()
  writeFileSync(join(value.evidenceRoot, 'proof.txt'), 'tampered\n')
  assert.throws(() => stageEvidenceBundle({
    catalog,
    candidate: value.candidate,
    plan: value.plan,
    records: value.records,
    evidenceRoot: value.evidenceRoot,
    outDir: value.outDir,
  }), /EVIDENCE_STAGE_GATE_REJECTED/)
})

test('D29_EvidenceStage_NeverOverwritesExistingOutput_03', () => {
  const value = fixture()
  mkdirSync(value.outDir)
  assert.throws(() => stageEvidenceBundle({
    catalog,
    candidate: value.candidate,
    plan: value.plan,
    records: value.records,
    evidenceRoot: value.evidenceRoot,
    outDir: value.outDir,
  }), /EVIDENCE_STAGE_OUTPUT_EXISTS/)
})
