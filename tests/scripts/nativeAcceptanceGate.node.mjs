import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
} from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'
import {
  acceptanceCandidateId,
  verifyAcceptance,
} from '../../scripts/native-cli/verify-acceptance.mjs'

const sha = value => createHash('sha256').update(value).digest('hex')
const caseId = index => `NATIVE-${String(index).padStart(2, '0')}`

function catalog() {
  return {
    schemaVersion: 1,
    specVersion: 2,
    cases: Array.from({ length: 64 }, (_, index) => ({
      caseId: caseId(index + 1),
      owner: index + 1 === 64 ? 'W9' : 'W0',
      requiredSubcaseIds: index + 1 === 41
        ? ['paste_then_enter', 'paste_failure_barrier']
        : [],
    })),
  }
}

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'ccdesk-d28-'))
  const artifactRoot = join(root, 'artifacts')
  const evidenceRoot = join(root, 'evidence')
  mkdirSync(artifactRoot)
  mkdirSync(evidenceRoot)

  const artifactBytes = Buffer.from('signed-candidate-fixture')
  const artifactSha = sha(artifactBytes)
  writeFileSync(join(artifactRoot, 'desk.bin'), artifactBytes)

  const candidate = {
    schemaVersion: 1,
    candidateId: '',
    deskCommit: 'a'.repeat(40),
    artifacts: [{
      name: 'linux-x64',
      path: 'desk.bin',
      sha256: artifactSha,
    }],
  }
  candidate.candidateId = acceptanceCandidateId(candidate)

  const cliSha = 'b'.repeat(64)
  const plan = {
    schemaVersion: 1,
    candidateId: candidate.candidateId,
    targets: [{
      targetId: 'linux-x64-codex-pinned',
      artifactName: 'linux-x64',
      cli: {
        kind: 'codex',
        version: '0.test',
        binarySha256: cliSha,
      },
      decisions: catalog().cases.map(entry => ({
        caseId: entry.caseId,
        status: 'REQUIRED',
        layers: ['A'],
      })),
    }],
  }

  const records = []
  for (const entry of catalog().cases) {
    const subcases = entry.requiredSubcaseIds.length > 0
      ? entry.requiredSubcaseIds
      : [null]
    for (const subcaseId of subcases) {
      const name = `${entry.caseId}-${subcaseId ?? 'parent'}.txt`
      const bytes = Buffer.from(`evidence:${name}`)
      writeFileSync(join(evidenceRoot, name), bytes)
      records.push({
        schemaVersion: 2,
        targetId: 'linux-x64-codex-pinned',
        candidateId: candidate.candidateId,
        caseId: entry.caseId,
        subcaseId,
        runId: `run-${entry.caseId}-${subcaseId ?? 'parent'}`,
        specVersion: 2,
        status: 'PASS',
        evidenceLayer: 'A',
        cliBinarySha256: cliSha,
        deskPackageSha256: artifactSha,
        evidence: [{
          path: name,
          sha256: sha(bytes),
        }],
      })
    }
  }

  return {
    root,
    artifactRoot,
    evidenceRoot,
    catalog: catalog(),
    candidate,
    plan,
    records,
  }
}

function verify(value) {
  return verifyAcceptance({
    catalog: value.catalog,
    candidate: value.candidate,
    plan: value.plan,
    records: value.records,
    artifactRoot: value.artifactRoot,
    evidenceRoot: value.evidenceRoot,
  })
}

test('D28_Gate_FullFrozenPlanAndEvidencePass_001', () => {
  const value = fixture()
  assert.deepEqual(verify(value), {
    status: 'PASS',
    candidateId: value.candidate.candidateId,
    targetIds: ['linux-x64-codex-pinned'],
  })
})

test('D28_Gate_MissingRequiredParentOrSubcaseFailsClosed_002', () => {
  const value = fixture()
  value.records = value.records.filter(record =>
    !(record.caseId === 'NATIVE-41' && record.subcaseId === 'paste_failure_barrier'))
  assert.equal(verify(value).reason, 'REQUIRED_EVIDENCE_MISSING')
})

test('D28_Gate_DuplicateEvidenceKeyIsRejectedEvenWhenRecordsAgree_003', () => {
  const value = fixture()
  value.records.push(structuredClone(value.records[0]))
  assert.equal(verify(value).reason, 'DUPLICATE_EVIDENCE_RECORD')
})

test('D28_Gate_FakeNaWithoutBasisEvidenceIsRejected_004', () => {
  const value = fixture()
  const decision = value.plan.targets[0].decisions.find(item => item.caseId === 'NATIVE-40')
  decision.status = 'N_A'
  delete decision.layers
  decision.reason = 'not supported here'
  decision.basis = []
  value.records = value.records.filter(record => record.caseId !== 'NATIVE-40')
  assert.equal(verify(value).reason, 'NA_BASIS_EVIDENCE_REQUIRED')
})

test('D28_Gate_WrongCandidateArtifactHashIsRejected_005', () => {
  const value = fixture()
  value.candidate.artifacts[0].sha256 = 'c'.repeat(64)
  value.candidate.candidateId = acceptanceCandidateId(value.candidate)
  value.plan.candidateId = value.candidate.candidateId
  for (const record of value.records) {
    record.candidateId = value.candidate.candidateId
    record.deskPackageSha256 = 'c'.repeat(64)
  }
  assert.equal(verify(value).reason, 'CANDIDATE_ARTIFACT_HASH_MISMATCH')
})

test('D28_Gate_WrongCliBinaryHashInPassRecordIsRejected_006', () => {
  const value = fixture()
  value.records[0].cliBinarySha256 = 'd'.repeat(64)
  assert.equal(verify(value).reason, 'CLI_BINARY_HASH_MISMATCH')
})

test('D28_Gate_EvidenceFileHashMismatchIsRejected_007', () => {
  const value = fixture()
  value.records[0].evidence[0].sha256 = 'e'.repeat(64)
  assert.equal(verify(value).reason, 'EVIDENCE_FILE_HASH_MISMATCH')
})

test('D28_Gate_FailBlockedAndNotRunCanNeverSatisfyRequiredEvidence_008', () => {
  for (const status of ['FAIL', 'BLOCKED', 'NOT_RUN']) {
    const value = fixture()
    value.records[0].status = status
    assert.equal(
      verify(value).reason,
      `REQUIRED_EVIDENCE_${status}`,
      status,
    )
  }
})

test('D28_Gate_PlanMustDecideEveryCatalogCaseExactlyOnce_009', () => {
  const missing = fixture()
  missing.plan.targets[0].decisions.pop()
  assert.equal(verify(missing).reason, 'TARGET_CASE_PLAN_INCOMPLETE')

  const duplicate = fixture()
  duplicate.plan.targets[0].decisions.push(
    structuredClone(duplicate.plan.targets[0].decisions[0]),
  )
  assert.equal(verify(duplicate).reason, 'DUPLICATE_CASE_DECISION')
})

test('D28_Gate_UnplannedEvidenceCannotBeSmuggledIntoCertification_010', () => {
  const value = fixture()
  const extra = structuredClone(value.records[0])
  extra.caseId = 'NATIVE-99'
  extra.runId = 'run-extra'
  value.records.push(extra)
  assert.equal(verify(value).reason, 'UNPLANNED_EVIDENCE_RECORD')
})
