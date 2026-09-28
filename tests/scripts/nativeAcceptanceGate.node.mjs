import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import test from 'node:test'
import { buildCandidateManifest } from '../../scripts/native-cli/candidate-manifest.mjs'
import {
  computeTargetPlanId,
  verifyAcceptance,
} from '../../scripts/native-cli/verify-acceptance.mjs'

const SOURCE_SHA = 'a'.repeat(40)
const CLI_SHA = 'b'.repeat(64)
const IDENTITY_SHA = 'c'.repeat(64)

function hash(value) {
  return createHash('sha256').update(value).digest('hex')
}

function write(root, relative, value) {
  const file = join(root, relative)
  mkdirSync(dirname(file), { recursive: true })
  writeFileSync(file, value)
  return file
}

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'cc-desk-acceptance-'))
  const candidateRoot = join(root, 'candidate')
  const evidenceRoot = join(root, 'evidence')

  write(candidateRoot, 'windows/CC Desk_1.2.3_x64-setup.exe', 'win-package')
  write(candidateRoot, 'windows/CC Desk_1.2.3_x64-setup.exe.sig', 'win-signature')
  write(candidateRoot, 'macos/CC Desk_aarch64.app.tar.gz', 'mac-package')
  write(candidateRoot, 'macos/CC Desk_aarch64.app.tar.gz.sig', 'mac-signature')
  write(candidateRoot, 'macos/CC Desk_aarch64.dmg', 'mac-dmg')
  write(candidateRoot, 'linux/CC Desk_1.2.3_amd64.AppImage', 'linux-package')
  write(candidateRoot, 'linux/CC Desk_1.2.3_amd64.AppImage.sig', 'linux-signature')

  const candidate = buildCandidateManifest({
    root: candidateRoot,
    sourceSha: SOURCE_SHA,
    version: '1.2.3',
  })
  const winPackage = candidate.files.find(file =>
    file.platform === 'windows-x86_64' && file.path.endsWith('.exe'))

  const plan = {
    schemaVersion: 1,
    candidateId: candidate.candidateId,
    targets: [{
      targetId: 'windows-codex-fixture',
      platform: 'windows-x86_64',
      identitySha256: IDENTITY_SHA,
      cli: {
        kind: 'codex',
        version: 'fixture-1.0.0',
        binarySha256: CLI_SHA,
      },
      requirements: [{
        caseId: 'NATIVE-64',
        subcaseId: 'gate-negative-cases',
        requiredLayers: ['A', 'D'],
      }],
    }],
  }
  plan.planId = computeTargetPlanId(plan)

  const aEvidence = Buffer.from('layer-a-evidence')
  const dEvidence = Buffer.from('layer-d-evidence')
  write(evidenceRoot, 'a.json', aEvidence)
  write(evidenceRoot, 'd.json', dEvidence)

  const base = {
    schemaVersion: 2,
    targetId: 'windows-codex-fixture',
    caseId: 'NATIVE-64',
    subcaseId: 'gate-negative-cases',
    candidateId: candidate.candidateId,
    sourceSha: SOURCE_SHA,
    targetIdentitySha256: IDENTITY_SHA,
    cliKind: 'codex',
    cliVersion: 'fixture-1.0.0',
    cliBinarySha256: CLI_SHA,
    status: 'PASS',
    nonApplicabilityReason: null,
    nonApplicabilityEvidenceSha256: null,
  }
  const records = [
    {
      ...base,
      runId: 'run-a',
      evidenceLayer: 'A',
      deskPackageSha256: null,
      evidence: [{ path: 'a.json', sha256: hash(aEvidence) }],
    },
    {
      ...base,
      runId: 'run-d',
      evidenceLayer: 'D',
      deskPackageSha256: winPackage.sha256,
      evidence: [{ path: 'd.json', sha256: hash(dEvidence) }],
    },
  ]

  return {
    root,
    candidateRoot,
    evidenceRoot,
    candidate,
    plan,
    records,
    cleanup: () => rmSync(root, { recursive: true, force: true }),
  }
}

function verify(fx, records = fx.records, plan = fx.plan, candidate = fx.candidate) {
  return verifyAcceptance({
    candidate,
    candidateRoot: fx.candidateRoot,
    plan,
    records,
    evidenceRoot: fx.evidenceRoot,
  })
}

test('D28_Gate_CompleteExactEvidencePasses_01', () => {
  const fx = fixture()
  try {
    const result = verify(fx)
    assert.equal(result.status, 'PASS')
    assert.equal(result.requirementCount, 1)
    assert.equal(result.recordCount, 2)
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_MissingRequiredLayerFails_02', () => {
  const fx = fixture()
  try {
    assert.throws(() => verify(fx, [fx.records[0]]), /ACCEPTANCE_REQUIRED_LAYER_MISSING/)
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_DuplicateFinalResultFails_03', () => {
  const fx = fixture()
  try {
    assert.throws(
      () => verify(fx, [...fx.records, { ...fx.records[0], runId: 'duplicate' }]),
      /ACCEPTANCE_DUPLICATE_RESULT/,
    )
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_FakeNaAndNaConflictFailClosed_04', () => {
  const fx = fixture()
  try {
    const fakeNa = {
      ...fx.records[0],
      status: 'N_A',
      nonApplicabilityReason: 'not supported by fixture version',
      nonApplicabilityEvidenceSha256: 'd'.repeat(64),
    }
    assert.throws(() => verify(fx, [fakeNa]), /ACCEPTANCE_NA_EVIDENCE_MISMATCH/)

    const evidenceSha = fx.records[0].evidence[0].sha256
    const justifiedNa = {
      ...fakeNa,
      nonApplicabilityEvidenceSha256: evidenceSha,
    }
    assert.throws(
      () => verify(fx, [justifiedNa, fx.records[1]]),
      /ACCEPTANCE_NA_CONFLICT/,
    )
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_WrongEvidenceOrPackageHashFails_05', () => {
  const fx = fixture()
  try {
    assert.throws(
      () => verify(fx, [
        { ...fx.records[0], evidence: [{ path: 'a.json', sha256: 'e'.repeat(64) }] },
        fx.records[1],
      ]),
      /ACCEPTANCE_EVIDENCE_HASH_MISMATCH/,
    )
    assert.throws(
      () => verify(fx, [
        fx.records[0],
        { ...fx.records[1], deskPackageSha256: 'f'.repeat(64) },
      ]),
      /ACCEPTANCE_PACKAGE_HASH_MISMATCH/,
    )
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_CandidateMutationOrIdentityMismatchFails_06', () => {
  const fx = fixture()
  try {
    const win = fx.candidate.files.find(file => file.path.endsWith('.exe'))
    writeFileSync(join(fx.candidateRoot, win.path), 'tampered-package')
    assert.throws(() => verify(fx), /CANDIDATE_FILE_HASH_MISMATCH/)
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_UnplannedLayerOrRecordFails_07', () => {
  const fx = fixture()
  try {
    assert.throws(
      () => verify(fx, [...fx.records, { ...fx.records[0], runId: 'run-b', evidenceLayer: 'B' }]),
      /ACCEPTANCE_UNPLANNED_LAYER/,
    )
    assert.throws(
      () => verify(fx, [{ ...fx.records[0], caseId: 'NATIVE-63' }, fx.records[1]]),
      /ACCEPTANCE_UNPLANNED_RECORD/,
    )
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_PlanCannotSilentlyChangeWithoutNewPlanId_08', () => {
  const fx = fixture()
  try {
    const changed = structuredClone(fx.plan)
    changed.targets[0].requirements[0].requiredLayers = ['A']
    assert.throws(() => verify(fx, fx.records, changed), /ACCEPTANCE_PLAN_ID_MISMATCH/)
  } finally {
    fx.cleanup()
  }
})
