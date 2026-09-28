import assert from 'node:assert/strict'
import test from 'node:test'
import { verifyAcceptance } from '../../scripts/native-cli/verify-acceptance.mjs'
import { candidateIdFor } from '../../scripts/native-cli/candidate-manifest.mjs'

const shaA = 'a'.repeat(64)
const shaB = 'b'.repeat(64)

function valid() {
  const commitSha = '1'.repeat(40)
  const files = [
    { path: 'windows/app.exe', kind: 'windows-package', sha256: shaA, size: 10 },
    { path: 'linux/app.AppImage', kind: 'linux-package', sha256: shaB, size: 20 },
  ]
  const candidateId = candidateIdFor(commitSha, files)
  const targets = [{
    targetId: 'windows-codex',
    required: [
      { caseId: 'NATIVE-01', evidenceLayers: ['D'] },
      { caseId: 'NATIVE-63', evidenceLayers: ['C'], subcaseIds: ['observer-off', 'observer-on'] },
    ],
  }]
  return {
    manifest: {
      schemaVersion: 1,
      candidate: {
        schemaVersion: 1,
        candidateId,
        commitSha,
        files,
      },
      targets: structuredClone(targets),
      records: [
        {
          targetId: 'windows-codex',
          caseId: 'NATIVE-01',
          subcaseId: null,
          candidateId,
          status: 'PASS',
          evidenceLayer: 'D',
          packageSha256: shaA,
          evidence: [{ kind: 'installed-package', path: 'evidence/native-01.json', sha256: shaB }],
          nonApplicabilityReason: null,
        },
        {
          targetId: 'windows-codex',
          caseId: 'NATIVE-63',
          subcaseId: 'observer-off',
          candidateId,
          status: 'PASS',
          evidenceLayer: 'C',
          evidence: [{ kind: 'raw-envelope', path: 'evidence/off.json', sha256: shaA }],
          nonApplicabilityReason: null,
        },
        {
          targetId: 'windows-codex',
          caseId: 'NATIVE-63',
          subcaseId: 'observer-on',
          candidateId,
          status: 'N_A',
          evidenceLayer: 'C',
          evidence: [{ kind: 'capability-proof', path: 'evidence/na.json', sha256: shaB }],
          nonApplicabilityReason: 'Pinned CLI version documents this capability as unavailable.',
        },
      ],
    },
    plan: {
      schemaVersion: 1,
      status: 'READY',
      targets: structuredClone(targets),
    },
  }
}

function verify(value) {
  return verifyAcceptance(value.manifest, value.manifest.candidate, value.plan)
}

test('D28_Gate_AcceptsCompleteDeclaredTarget_01', () => {
  const value = valid()
  assert.deepEqual(verify(value), {
    ok: true,
    schemaVersion: 1,
    candidateId: value.manifest.candidate.candidateId,
    targetCount: 1,
    recordCount: 3,
  })
})

test('D28_Gate_RejectsMissingSubcase_02', () => {
  const value = valid()
  value.manifest.records.pop()
  assert.throws(() => verify(value), /MISSING_REQUIRED_ACCEPTANCE/)
})

test('D28_Gate_RejectsDuplicateRecord_03', () => {
  const value = valid()
  value.manifest.records.push(structuredClone(value.manifest.records[0]))
  assert.throws(() => verify(value), /DUPLICATE_ACCEPTANCE_RECORD/)
})

test('D28_Gate_RejectsFailBlockedAndNotRun_04', () => {
  for (const status of ['FAIL', 'BLOCKED', 'NOT_RUN']) {
    const value = valid()
    value.manifest.records[0].status = status
    assert.throws(() => verify(value), /UNCERTIFIED_STATUS/)
  }
})

test('D28_Gate_RejectsFakeNAWithoutReasonOrEvidence_05', () => {
  const missingReason = valid()
  missingReason.manifest.records[2].nonApplicabilityReason = ''
  assert.throws(() => verify(missingReason), /N_A_REASON_REQUIRED/)

  const missingEvidence = valid()
  missingEvidence.manifest.records[2].evidence = []
  assert.throws(() => verify(missingEvidence), /EVIDENCE_REQUIRED/)
})

test('D28_Gate_RejectsWrongCandidatePackageHash_06', () => {
  const value = valid()
  value.manifest.records[0].packageSha256 = 'c'.repeat(64)
  assert.throws(() => verify(value), /CANDIDATE_PACKAGE_HASH_MISMATCH/)
})

test('D28_Gate_RejectsCandidateIdentityMismatchAndUndeclaredRecord_07', () => {
  const mismatch = valid()
  mismatch.manifest.records[0].candidateId = 'other'
  assert.throws(() => verify(mismatch), /CANDIDATE_ID_MISMATCH/)

  const extra = valid()
  extra.manifest.records.push({
    ...structuredClone(extra.manifest.records[0]),
    caseId: 'NATIVE-02',
  })
  assert.throws(() => verify(extra), /UNDECLARED_ACCEPTANCE_RECORD/)
})

test('D28_Gate_RecomputesCandidateIdentity_08', () => {
  const value = valid()
  value.manifest.candidate.files[0].sha256 = 'c'.repeat(64)
  assert.throws(() => verify(value), /INVALID_CANDIDATE_IDENTITY/)
})

test('D28_Gate_RejectsEvidenceLayerDowngrade_09', () => {
  const value = valid()
  value.manifest.records[0].evidenceLayer = 'A'
  delete value.manifest.records[0].packageSha256
  assert.throws(() => verify(value), /EVIDENCE_LAYER_TOO_WEAK/)
})

test('D28_Gate_RejectsTargetPlanShrinkOrBlockedPlan_10', () => {
  const shrink = valid()
  shrink.manifest.targets[0].required.pop()
  assert.throws(() => verify(shrink), /TARGET_PLAN_MISMATCH/)

  const blocked = valid()
  blocked.plan.status = 'BLOCKED'
  assert.throws(() => verify(blocked), /RELEASE_TARGETS_NOT_READY/)
})
