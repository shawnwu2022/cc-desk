import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { buildCandidateManifest } from '../../scripts/native-cli/candidate-manifest.mjs'
import { verifyPromotion } from '../../scripts/native-cli/verify-promotion.mjs'

function fixture() {
  const root = mkdtempSync(path.join(os.tmpdir(), 'ccdesk-candidate-'))
  const names = [
    'windows/CC.Desk-setup.exe',
    'windows/CC.Desk-setup.exe.sig',
    'linux/CC.Desk.AppImage',
    'linux/CC.Desk.AppImage.sig',
    'macos/CC.Desk.app.tar.gz',
    'macos/CC.Desk.app.tar.gz.sig',
    'macos/CC.Desk.dmg',
  ]
  for (const name of names) {
    const file = path.join(root, name)
    mkdirSync(path.dirname(file), { recursive: true })
    writeFileSync(file, 'fixture:' + name)
  }
  return root
}

function targetPlan() {
  return {
    schemaVersion: 1,
    status: 'READY',
    targets: [{
      targetId: 'windows-codex',
      required: [{ caseId: 'NATIVE-01', evidenceLayers: ['D'] }],
    }],
  }
}

function acceptance(candidate) {
  const packageSha = candidate.files.find(file => file.kind === 'windows-package').sha256
  const targets = structuredClone(targetPlan().targets)
  return {
    schemaVersion: 1,
    candidate: structuredClone(candidate),
    targets,
    records: [{
      targetId: 'windows-codex',
      caseId: 'NATIVE-01',
      subcaseId: null,
      candidateId: candidate.candidateId,
      status: 'PASS',
      evidenceLayer: 'D',
      packageSha256: packageSha,
      evidence: [{
        kind: 'installed-package',
        path: 'evidence/windows-codex.json',
        sha256: 'e'.repeat(64),
      }],
      nonApplicabilityReason: null,
    }],
  }
}

test('D30_Candidate_IdentityChangesWithArtifactBytes_01', () => {
  const root = fixture()
  const commitSha = '1'.repeat(40)
  const first = buildCandidateManifest({ root, commitSha })
  writeFileSync(path.join(root, 'linux/CC.Desk.AppImage'), 'changed')
  const second = buildCandidateManifest({ root, commitSha })
  assert.notEqual(first.candidateId, second.candidateId)
})

test('D30_Promotion_AcceptsExactAcceptedCandidate_02', () => {
  const root = fixture()
  const commitSha = '2'.repeat(40)
  const candidate = buildCandidateManifest({ root, commitSha })
  const result = verifyPromotion({
    candidate,
    acceptance: acceptance(candidate),
    targetPlan: targetPlan(),
    artifactsRoot: root,
    expectedCommitSha: commitSha,
  })
  assert.equal(result.ok, true)
  assert.equal(result.files.length, 7)
})

test('D30_Promotion_RejectsRebuiltOrMutatedArtifact_03', () => {
  const root = fixture()
  const commitSha = '3'.repeat(40)
  const candidate = buildCandidateManifest({ root, commitSha })
  writeFileSync(path.join(root, 'windows/CC.Desk-setup.exe'), 'rebuilt bytes')
  assert.throws(() => verifyPromotion({
    candidate,
    acceptance: acceptance(candidate),
    targetPlan: targetPlan(),
    artifactsRoot: root,
    expectedCommitSha: commitSha,
  }), /CANDIDATE_FILE_(SIZE|HASH)_MISMATCH/)
})

test('D30_Promotion_RejectsDifferentCommitOrCandidate_04', () => {
  const root = fixture()
  const commitSha = '4'.repeat(40)
  const candidate = buildCandidateManifest({ root, commitSha })
  assert.throws(() => verifyPromotion({
    candidate,
    acceptance: acceptance(candidate),
    targetPlan: targetPlan(),
    artifactsRoot: root,
    expectedCommitSha: '5'.repeat(40),
  }), /CANDIDATE_COMMIT_MISMATCH/)

  const wrongAcceptance = acceptance(candidate)
  wrongAcceptance.candidate.files[0].sha256 = 'f'.repeat(64)
  assert.throws(() => verifyPromotion({
    candidate,
    acceptance: wrongAcceptance,
    targetPlan: targetPlan(),
    artifactsRoot: root,
    expectedCommitSha: commitSha,
  }), /INVALID_CANDIDATE_IDENTITY/)
})

test('D30_Candidate_RejectsDuplicatePlatformArtifact_05', () => {
  const root = fixture()
  writeFileSync(path.join(root, 'windows/Other-setup.exe'), 'duplicate')
  assert.throws(() => buildCandidateManifest({
    root,
    commitSha: '6'.repeat(40),
  }), /CANDIDATE_PLATFORM_INCOMPLETE/)
})

test('D30_Promotion_RejectsTargetPlanMismatch_06', () => {
  const root = fixture()
  const commitSha = '7'.repeat(40)
  const candidate = buildCandidateManifest({ root, commitSha })
  const plan = targetPlan()
  plan.targets[0].required[0].evidenceLayers = ['A']
  assert.throws(() => verifyPromotion({
    candidate,
    acceptance: acceptance(candidate),
    targetPlan: plan,
    artifactsRoot: root,
    expectedCommitSha: commitSha,
  }), /TARGET_PLAN_MISMATCH/)
})
