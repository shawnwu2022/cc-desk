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
  return {
    schemaVersion: 1,
    candidate: {
      schemaVersion: 1,
      candidateId,
      commitSha,
      files,
    },
    targets: [{
      targetId: 'windows-codex',
      required: [
        { caseId: 'NATIVE-01' },
        { caseId: 'NATIVE-63', subcaseIds: ['observer-off', 'observer-on'] },
      ],
    }],
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
  }
}

test('D28_Gate_AcceptsCompleteDeclaredTarget_01', () => {
  const result = verifyAcceptance(valid())
  assert.deepEqual(result, {
    ok: true,
    schemaVersion: 1,
    candidateId: valid().candidate.candidateId,
    targetCount: 1,
    recordCount: 3,
  })
})

test('D28_Gate_RejectsMissingSubcase_02', () => {
  const value = valid()
  value.records.pop()
  assert.throws(() => verifyAcceptance(value), /MISSING_REQUIRED_ACCEPTANCE/)
})

test('D28_Gate_RejectsDuplicateRecord_03', () => {
  const value = valid()
  value.records.push(structuredClone(value.records[0]))
  assert.throws(() => verifyAcceptance(value), /DUPLICATE_ACCEPTANCE_RECORD/)
})

test('D28_Gate_RejectsFailBlockedAndNotRun_04', () => {
  for (const status of ['FAIL', 'BLOCKED', 'NOT_RUN']) {
    const value = valid()
    value.records[0].status = status
    assert.throws(() => verifyAcceptance(value), /UNCERTIFIED_STATUS/)
  }
})

test('D28_Gate_RejectsFakeNAWithoutReasonOrEvidence_05', () => {
  const missingReason = valid()
  missingReason.records[2].nonApplicabilityReason = ''
  assert.throws(() => verifyAcceptance(missingReason), /N_A_REASON_REQUIRED/)

  const missingEvidence = valid()
  missingEvidence.records[2].evidence = []
  assert.throws(() => verifyAcceptance(missingEvidence), /EVIDENCE_REQUIRED/)
})

test('D28_Gate_RejectsWrongCandidatePackageHash_06', () => {
  const value = valid()
  value.records[0].packageSha256 = 'c'.repeat(64)
  assert.throws(() => verifyAcceptance(value), /CANDIDATE_PACKAGE_HASH_MISMATCH/)
})

test('D28_Gate_RejectsCandidateIdentityMismatchAndUndeclaredRecord_07', () => {
  const mismatch = valid()
  mismatch.records[0].candidateId = 'other'
  assert.throws(() => verifyAcceptance(mismatch), /CANDIDATE_ID_MISMATCH/)

  const extra = valid()
  extra.records.push({
    ...structuredClone(extra.records[0]),
    caseId: 'NATIVE-02',
  })
  assert.throws(() => verifyAcceptance(extra), /UNDECLARED_ACCEPTANCE_RECORD/)
})


test('D28_Gate_RecomputesCandidateIdentity_08', () => {
  const value = valid()
  value.candidate.files[0].sha256 = 'c'.repeat(64)
  assert.throws(() => verifyAcceptance(value), /INVALID_CANDIDATE_IDENTITY/)
})
