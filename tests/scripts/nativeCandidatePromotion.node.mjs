import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
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

function acceptance(candidate) {
  const packageSha = candidate.files.find(file => file.kind === 'windows-package').sha256
  return {
    schemaVersion: 1,
    candidate,
    targets: [{
      targetId: 'windows-codex',
      required: [{ caseId: 'NATIVE-01' }],
    }],
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
    artifactsRoot: root,
    expectedCommitSha: '5'.repeat(40),
  }), /CANDIDATE_COMMIT_MISMATCH/)

  const wrong = acceptance(candidate)
  wrong.candidate.candidateId = 'different'
  assert.throws(() => verifyPromotion({
    candidate,
    acceptance: wrong,
    artifactsRoot: root,
    expectedCommitSha: commitSha,
  }), /ACCEPTANCE_CANDIDATE_MISMATCH/)
})
