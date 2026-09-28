import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import {
  candidateIdFor,
  targetIdentitySha256,
  verifyAcceptance,
} from '../../scripts/native-cli/verify-acceptance.mjs'

const catalog = JSON.parse(readFileSync(
  new URL('../../docs/testing/native-cli-acceptance-catalog.json', import.meta.url),
  'utf8',
))

function sha(value) {
  return createHash('sha256').update(value).digest('hex')
}

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'cc-desk-d28-'))
  const proof = Buffer.from('synthetic acceptance proof\n')
  writeFileSync(join(root, 'proof.txt'), proof)

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

  const requiredCaseIds = catalog.cases.map(item => item.caseId)
  const target = {
    targetId: 'windows-x64-codex-1',
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
    requiredCaseIds,
  }
  target.identitySha256 = targetIdentitySha256(target)
  const plan = {
    schemaVersion: 1,
    claim: 'installed-native-release',
    candidateId: candidate.candidateId,
    targets: [target],
  }

  const records = []
  for (const item of catalog.cases) {
    for (const subcaseId of item.subcaseIds) {
      records.push({
        schemaVersion: 2,
        specVersion: 2,
        targetId: 'windows-x64-codex-1',
        caseId: item.caseId,
        subcaseId,
        runId: `run-${item.caseId}-${subcaseId}`,
        status: 'PASS',
        evidenceLayer: 'D',
        cliBinarySha256: 'c'.repeat(64),
        deskPackageSha256: 'b'.repeat(64),
        fixtureSha256: null,
        oracleKind: null,
        oracleSchemaVersion: null,
        expectedTransformId: null,
        expected: { outcome: 'synthetic-expected' },
        actual: { outcome: 'synthetic-expected' },
        evidence: [{
          path: 'proof.txt',
          sha256: sha(proof),
        }],
        nonApplicabilityReason: null,
      })
    }
  }
  return { root, candidate, plan, records }
}

function verify(value) {
  return verifyAcceptance({
    catalog,
    candidate: value.candidate,
    plan: value.plan,
    records: value.records,
    evidenceRoot: value.root,
  })
}

test('D28_Gate_CompleteTargetAndSubcasesPass_01', () => {
  const value = fixture()
  const result = verify(value)
  assert.equal(result.status, 'PASS')
  assert.equal(result.candidateId, value.candidate.candidateId)
  assert.deepEqual(result.targetIds, ['windows-x64-codex-1'])
  assert.equal(result.recordCount, value.records.length)
})

test('D28_Gate_MissingRequiredCaseFailsClosed_02', () => {
  const value = fixture()
  value.plan.targets[0].requiredCaseIds.pop()
  assert.deepEqual(verify(value), {
    status: 'FAIL',
    reason: 'REQUIRED_CASE_SET_INCOMPLETE',
    detail: 'windows-x64-codex-1',
  })
})

test('D28_Gate_MissingSubcaseFailsClosed_03', () => {
  const value = fixture()
  value.records.pop()
  assert.equal(verify(value).reason, 'REQUIRED_EVIDENCE_MISSING')
})

test('D28_Gate_DuplicateRecordFailsEvenWhenIdentical_04', () => {
  const value = fixture()
  value.records.push(structuredClone(value.records[0]))
  assert.equal(verify(value).reason, 'DUPLICATE_EVIDENCE_RECORD')
})

test('D28_Gate_FailBlockedAndNotRunCanNeverCertify_05', () => {
  for (const status of ['FAIL', 'BLOCKED', 'NOT_RUN']) {
    const value = fixture()
    value.records[0].status = status
    assert.equal(
      verify(value).reason,
      'REQUIRED_EVIDENCE_NOT_PASSING',
      status,
    )
  }
})

test('D28_Gate_NARequiresConcreteBasisAndEvidence_06', () => {
  const value = fixture()
  value.records[0].status = 'N_A'
  value.records[0].nonApplicabilityReason = 'no'
  assert.equal(verify(value).reason, 'N_A_BASIS_REQUIRED')

  value.records[0].nonApplicabilityReason = 'version does not expose this native capability'
  assert.equal(verify(value).status, 'PASS')

  value.records[0].evidence = []
  assert.equal(verify(value).reason, 'EVIDENCE_REQUIRED')
})

test('D28_Gate_WrongCandidateOrCliHashFailsIdentity_07', () => {
  const value = fixture()
  value.records[0].deskPackageSha256 = 'd'.repeat(64)
  assert.equal(verify(value).reason, 'EVIDENCE_IDENTITY_MISMATCH')

  const other = fixture()
  other.records[0].cliBinarySha256 = 'e'.repeat(64)
  assert.equal(verify(other).reason, 'EVIDENCE_IDENTITY_MISMATCH')
})

test('D28_Gate_WrongEvidenceHashFails_08', () => {
  const value = fixture()
  value.records[0].evidence[0].sha256 = 'f'.repeat(64)
  assert.equal(verify(value).reason, 'EVIDENCE_HASH_MISMATCH')
})

test('D28_Gate_WrongLayerCannotSubstituteForInstalledPackageEvidence_09', () => {
  for (const evidenceLayer of ['A', 'B', 'C']) {
    const value = fixture()
    value.records[0].evidenceLayer = evidenceLayer
    assert.equal(verify(value).reason, 'EVIDENCE_IDENTITY_MISMATCH')
  }
})

test('D28_Gate_UnplannedOrFakeTargetCannotBeSmuggledIn_10', () => {
  const value = fixture()
  value.records[0].targetId = 'other-target'
  assert.equal(verify(value).reason, 'UNPLANNED_EVIDENCE_RECORD')
})

test('D28_Gate_TargetIdentityHashBindsHostRuntime_11', () => {
  const value = fixture()
  value.plan.targets[0].xtermVersion = 'different-xterm'
  assert.deepEqual(verify(value), {
    status: 'FAIL',
    reason: 'TARGET_IDENTITY_HASH_MISMATCH',
    detail: 'windows-x64-codex-1',
  })
})

test('D28_Gate_PassRequiresExpectedAndActualResult_12', () => {
  const value = fixture()
  value.records[0].actual = null
  assert.equal(verify(value).reason, 'PASS_RESULT_REQUIRED')
})

test('D28_Gate_CandidateIdBindsSourceAndAllFileHashes_13', () => {
  const value = fixture()
  value.candidate.files[0].sha256 = '9'.repeat(64)
  assert.equal(verify(value).reason, 'CANDIDATE_ID_MISMATCH')
})

test('D28_Gate_All64CasesArePresentAndUnique_14', () => {
  assert.equal(catalog.caseCount, 64)
  assert.equal(catalog.cases.length, 64)
  assert.equal(new Set(catalog.cases.map(item => item.caseId)).size, 64)
  for (let index = 1; index <= 64; index += 1) {
    assert.ok(catalog.cases.some(item => item.caseId === `NATIVE-${String(index).padStart(2, '0')}`))
  }
})
