import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import test from 'node:test'
import { buildCandidateManifest } from '../../scripts/native-cli/candidate-manifest.mjs'
import {
  computeAcceptanceCatalogId,
  computeTargetPlanId,
  verifyAcceptance,
} from '../../scripts/native-cli/verify-acceptance.mjs'

const SOURCE_SHA = 'a'.repeat(40)
const CLI_SHA = 'b'.repeat(64)
const IDENTITY_SHA = 'c'.repeat(64)

const owners = [
  'W7','W7','W2','W2','W2','W2','W2','W1','W7','W1',
  'W7','W7','W3','W6','W6','W6','W6','W6','W5','W6',
  'W5','W5','W5','W5','W5','W5','W6','W6','W4','W6',
  'W4','W4','W4','W3','W3','W8','W6','W3','W6','W9',
  'W5','W5','W5','W5','W5','W4','W4','W4','W4','W2',
  'W2','W2','W1','W4','W3','W8','W1','W3','W0','W2',
  'W4','W8','W6','W9',
]

const minimumLayers = [
  ['C','D'],['D'],['A'],['A','C'],['B'],['B'],['A','B'],['A','C'],['A','C','D'],['A'],
  ['C'],['A','C'],['A','B'],['C'],['C'],['C'],['C'],['C'],['A','C'],['B','C'],
  ['B'],['A','C'],['A','C'],['A','C'],['A'],['A','B'],['B','C'],['B'],['A','B'],['B','C'],
  ['B'],['B'],['B'],['A','B'],['A'],['A'],['C','D'],['B','C'],['D'],['C'],
  ['A','B'],['A','B'],['A'],['A','B'],['B','C'],['A'],['B'],['A','B'],['A','B'],['A'],
  ['A'],['A','C'],['A','B'],['B'],['B'],['A','B'],['D'],['A'],['A'],['A','B'],
  ['A','C'],['A','D'],['C'],['A'],
]

function hash(value) {
  return createHash('sha256').update(value).digest('hex')
}

function write(root, relative, value) {
  const file = join(root, relative)
  mkdirSync(dirname(file), { recursive: true })
  writeFileSync(file, value)
  return file
}

function buildCatalog() {
  const catalog = {
    schemaVersion: 1,
    catalogVersion: 1,
    policy: {
      allRequiredCasesMustAppearPerTarget: true,
      naRequiresEvidence: true,
      planMayAddLayersButNotRemoveMinimumLayers: true,
      note: 'This catalog defines release-gate minimums, not execution results.',
    },
    cases: Array.from({ length: 64 }, (_, index) => ({
      caseId: `NATIVE-${String(index + 1).padStart(2, '0')}`,
      owner: owners[index],
      required: true,
      allowNa: true,
      subcasePolicy: 'explicit',
      minimumLayers: minimumLayers[index],
    })),
  }
  catalog.catalogId = computeAcceptanceCatalogId(catalog)
  return catalog
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

  const catalog = buildCatalog()
  const plan = {
    schemaVersion: 1,
    candidateId: candidate.candidateId,
    catalogId: catalog.catalogId,
    targets: [{
      targetId: 'windows-codex-fixture',
      platform: 'windows-x86_64',
      identitySha256: IDENTITY_SHA,
      cli: {
        kind: 'codex',
        version: 'fixture-1.0.0',
        binarySha256: CLI_SHA,
      },
      requirements: catalog.cases.map(entry => ({
        caseId: entry.caseId,
        subcaseId: 'baseline',
        requiredLayers: [...entry.minimumLayers],
      })),
    }],
  }
  plan.planId = computeTargetPlanId(plan)

  const records = []
  for (const requirement of plan.targets[0].requirements) {
    for (const layer of requirement.requiredLayers) {
      const body = Buffer.from(`${requirement.caseId}:${layer}:evidence`)
      const relative = `${requirement.caseId}/${layer}.json`
      write(evidenceRoot, relative, body)
      records.push({
        schemaVersion: 2,
        targetId: 'windows-codex-fixture',
        caseId: requirement.caseId,
        subcaseId: 'baseline',
        runId: `run-${requirement.caseId}-${layer}`,
        candidateId: candidate.candidateId,
        sourceSha: SOURCE_SHA,
        targetIdentitySha256: IDENTITY_SHA,
        cliKind: 'codex',
        cliVersion: 'fixture-1.0.0',
        cliBinarySha256: CLI_SHA,
        status: 'PASS',
        evidenceLayer: layer,
        deskPackageSha256: layer === 'D' ? winPackage.sha256 : null,
        evidence: [{ path: relative, sha256: hash(body) }],
        nonApplicabilityReason: null,
        nonApplicabilityEvidenceSha256: null,
      })
    }
  }

  return {
    root,
    candidateRoot,
    evidenceRoot,
    candidate,
    catalog,
    plan,
    records,
    cleanup: () => rmSync(root, { recursive: true, force: true }),
  }
}

function verify(fx, {
  records = fx.records,
  plan = fx.plan,
  catalog = fx.catalog,
  candidate = fx.candidate,
} = {}) {
  return verifyAcceptance({
    candidate,
    candidateRoot: fx.candidateRoot,
    catalog,
    plan,
    records,
    evidenceRoot: fx.evidenceRoot,
  })
}

function caseRecords(fx, caseId) {
  return fx.records.filter(record => record.caseId === caseId)
}

test('D28_Gate_Complete64CasePlanPasses_01', () => {
  const fx = fixture()
  try {
    const result = verify(fx)
    assert.equal(result.status, 'PASS')
    assert.equal(result.catalogId, fx.catalog.catalogId)
    assert.equal(result.requirementCount, 64)
    assert.equal(result.recordCount, fx.records.length)
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_PlanCannotOmitCatalogCase_02', () => {
  const fx = fixture()
  try {
    const plan = structuredClone(fx.plan)
    plan.targets[0].requirements = plan.targets[0].requirements
      .filter(item => item.caseId !== 'NATIVE-63')
    plan.planId = computeTargetPlanId(plan)
    assert.throws(() => verify(fx, { plan }), /ACCEPTANCE_REQUIRED_CASE_MISSING/)
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_PlanCannotDowngradeMinimumLayer_03', () => {
  const fx = fixture()
  try {
    const plan = structuredClone(fx.plan)
    plan.targets[0].requirements.find(item => item.caseId === 'NATIVE-01').requiredLayers = ['C']
    plan.planId = computeTargetPlanId(plan)
    assert.throws(() => verify(fx, { plan }), /ACCEPTANCE_MINIMUM_LAYER_MISSING/)
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_MissingRequiredLayerFails_04', () => {
  const fx = fixture()
  try {
    const records = fx.records.filter(record =>
      !(record.caseId === 'NATIVE-01' && record.evidenceLayer === 'D'))
    assert.throws(() => verify(fx, { records }), /ACCEPTANCE_REQUIRED_LAYER_MISSING/)
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_DuplicateFinalResultFails_05', () => {
  const fx = fixture()
  try {
    const duplicate = { ...caseRecords(fx, 'NATIVE-64')[0], runId: 'duplicate' }
    assert.throws(
      () => verify(fx, { records: [...fx.records, duplicate] }),
      /ACCEPTANCE_DUPLICATE_RESULT/,
    )
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_FakeNaAndNaConflictFailClosed_06', () => {
  const fx = fixture()
  try {
    const source = caseRecords(fx, 'NATIVE-64')[0]
    const fakeNa = {
      ...source,
      status: 'N_A',
      nonApplicabilityReason: 'not supported by fixture version',
      nonApplicabilityEvidenceSha256: 'd'.repeat(64),
    }
    const without64 = fx.records.filter(record => record.caseId !== 'NATIVE-64')
    assert.throws(
      () => verify(fx, { records: [...without64, fakeNa] }),
      /ACCEPTANCE_NA_EVIDENCE_MISMATCH/,
    )

    const justifiedNa = {
      ...fakeNa,
      nonApplicabilityEvidenceSha256: source.evidence[0].sha256,
    }
    assert.throws(
      () => verify(fx, { records: [...fx.records, justifiedNa] }),
      /ACCEPTANCE_NA_CONFLICT|ACCEPTANCE_DUPLICATE_RESULT/,
    )
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_WrongEvidenceOrPackageHashFails_07', () => {
  const fx = fixture()
  try {
    const badEvidence = fx.records.map(record =>
      record.caseId === 'NATIVE-64'
        ? { ...record, evidence: [{ ...record.evidence[0], sha256: 'e'.repeat(64) }] }
        : record)
    assert.throws(
      () => verify(fx, { records: badEvidence }),
      /ACCEPTANCE_EVIDENCE_HASH_MISMATCH/,
    )

    const badPackage = fx.records.map(record =>
      record.caseId === 'NATIVE-01' && record.evidenceLayer === 'D'
        ? { ...record, deskPackageSha256: 'f'.repeat(64) }
        : record)
    assert.throws(
      () => verify(fx, { records: badPackage }),
      /ACCEPTANCE_PACKAGE_HASH_MISMATCH/,
    )
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_CandidateMutationFails_08', () => {
  const fx = fixture()
  try {
    const win = fx.candidate.files.find(file => file.path.endsWith('.exe'))
    writeFileSync(join(fx.candidateRoot, win.path), 'tampered-package')
    assert.throws(() => verify(fx), /CANDIDATE_FILE_HASH_MISMATCH/)
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_UnplannedLayerOrSubcaseFails_09', () => {
  const fx = fixture()
  try {
    const source = caseRecords(fx, 'NATIVE-64')[0]
    assert.throws(
      () => verify(fx, { records: [
        ...fx.records,
        { ...source, runId: 'run-extra-layer', evidenceLayer: 'B' },
      ] }),
      /ACCEPTANCE_UNPLANNED_LAYER/,
    )
    assert.throws(
      () => verify(fx, { records: [
        ...fx.records,
        { ...source, runId: 'run-extra-subcase', subcaseId: 'undeclared' },
      ] }),
      /ACCEPTANCE_UNPLANNED_RECORD/,
    )
  } finally {
    fx.cleanup()
  }
})

test('D28_Gate_CatalogAndPlanIdentityCannotChangeSilently_10', () => {
  const fx = fixture()
  try {
    const plan = structuredClone(fx.plan)
    plan.targets[0].requirements[0].requiredLayers.push('A')
    assert.throws(() => verify(fx, { plan }), /ACCEPTANCE_PLAN_ID_MISMATCH/)

    const catalog = structuredClone(fx.catalog)
    catalog.policy.note = 'changed without catalog id'
    assert.throws(() => verify(fx, { catalog }), /ACCEPTANCE_CATALOG_INVALID/)
  } finally {
    fx.cleanup()
  }
})
