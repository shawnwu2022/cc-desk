import assert from 'node:assert/strict'
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import test from 'node:test'
import { buildCandidateManifest } from '../../scripts/native-cli/candidate-manifest.mjs'
import {
  preparePromotion,
  verifyPublishedPromotion,
} from '../../scripts/native-cli/prepare-promotion.mjs'

const SOURCE_SHA = '1'.repeat(40)

function write(root, relative, value) {
  const path = join(root, relative)
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, value)
}

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'cc-desk-promotion-'))
  const candidateRoot = join(root, 'candidate')
  const outputRoot = join(root, 'publish')

  write(candidateRoot, 'cc-desk-candidate-sha-windows/CC Desk_1.2.3_x64-setup.exe', 'win-package')
  write(candidateRoot, 'cc-desk-candidate-sha-windows/CC Desk_1.2.3_x64-setup.exe.sig', 'win-signature')
  write(candidateRoot, 'cc-desk-candidate-sha-macos/CC Desk_aarch64.app.tar.gz', 'mac-package')
  write(candidateRoot, 'cc-desk-candidate-sha-macos/CC Desk_aarch64.app.tar.gz.sig', 'mac-signature')
  write(candidateRoot, 'cc-desk-candidate-sha-macos/CC Desk_aarch64.dmg', 'mac-dmg')
  write(candidateRoot, 'cc-desk-candidate-sha-linux/CC Desk_1.2.3_amd64.AppImage', 'linux-package')
  write(candidateRoot, 'cc-desk-candidate-sha-linux/CC Desk_1.2.3_amd64.AppImage.sig', 'linux-signature')

  const candidate = buildCandidateManifest({
    root: candidateRoot,
    sourceSha: SOURCE_SHA,
    version: '1.2.3',
  })
  const acceptance = {
    schemaVersion: 1,
    status: 'PASS',
    candidateId: candidate.candidateId,
    catalogId: '5'.repeat(64),
    planId: '2'.repeat(64),
    targetCount: 3,
    requirementCount: 64,
    recordCount: 100,
  }
  return {
    root,
    candidateRoot,
    outputRoot,
    candidate,
    acceptance,
    cleanup: () => rmSync(root, { recursive: true, force: true }),
  }
}

function prepare(fx, overrides = {}) {
  return preparePromotion({
    candidate: fx.candidate,
    candidateRoot: fx.candidateRoot,
    acceptance: fx.acceptance,
    expectedCandidateId: fx.candidate.candidateId,
    tag: 'v1.2.3',
    approval: `PROMOTE:${fx.candidate.candidateId}`,
    outputRoot: fx.outputRoot,
    ...overrides,
  })
}

test('D30_Promotion_StagesExactAcceptedCandidate_01', () => {
  const fx = fixture()
  try {
    const plan = prepare(fx)
    assert.equal(plan.candidateId, fx.candidate.candidateId)
    assert.equal(plan.sourceSha, SOURCE_SHA)
    assert.equal(plan.tag, 'v1.2.3')
    assert.equal(plan.acceptanceCatalogId, fx.acceptance.catalogId)
    assert.equal(plan.files.length, fx.candidate.files.length)
    for (const file of plan.files) {
      assert.equal(file.publishedName.includes(' '), false)
      assert.equal(readFileSync(join(fx.outputRoot, file.publishedName)).length, file.size)
    }
    assert.equal(verifyPublishedPromotion(plan, fx.outputRoot, { allowLatestJson: false }), true)
  } finally {
    fx.cleanup()
  }
})

test('D30_Promotion_RequiresExactCandidateTagApprovalAndGate_02', () => {
  for (const mutate of [
    fx => ({ expectedCandidateId: '3'.repeat(64) }),
    fx => ({ tag: 'v1.2.4' }),
    fx => ({ approval: 'PROMOTE' }),
    fx => ({ acceptance: { ...fx.acceptance, status: 'BLOCKED' } }),
    fx => ({ acceptance: { ...fx.acceptance, candidateId: '4'.repeat(64) } }),
  ]) {
    const fx = fixture()
    try {
      assert.throws(() => prepare(fx, mutate(fx)), /PROMOTION_/)
    } finally {
      fx.cleanup()
    }
  }
})

test('D30_Promotion_RejectsMutatedCandidateBytes_03', () => {
  const fx = fixture()
  try {
    const file = fx.candidate.files.find(item => item.path.endsWith('.exe'))
    writeFileSync(join(fx.candidateRoot, file.path), 'mutated')
    assert.throws(() => prepare(fx), /CANDIDATE_FILE_HASH_MISMATCH/)
  } finally {
    fx.cleanup()
  }
})

test('D30_Promotion_VerifiesDownloadedReleaseBytesAndExtras_04', () => {
  const fx = fixture()
  try {
    const plan = prepare(fx)
    writeFileSync(join(fx.outputRoot, 'latest.json'), '{}')
    assert.equal(verifyPublishedPromotion(plan, fx.outputRoot), true)

    const first = plan.files[0]
    writeFileSync(join(fx.outputRoot, first.publishedName), 'tampered')
    assert.throws(
      () => verifyPublishedPromotion(plan, fx.outputRoot),
      /PROMOTION_PUBLISHED_HASH_MISMATCH/,
    )
  } finally {
    fx.cleanup()
  }

  const fx = fixture()
  try {
    const plan = prepare(fx)
    writeFileSync(join(fx.outputRoot, 'unexpected.bin'), 'unexpected')
    assert.throws(
      () => verifyPublishedPromotion(plan, fx.outputRoot, { allowLatestJson: false }),
      /PROMOTION_UNDECLARED_PUBLISHED_FILE/,
    )
  } finally {
    fx.cleanup()
  }
})

test('D30_Promotion_NormalizedAssetNameCollisionFails_05', () => {
  const fx = fixture()
  try {
    write(fx.candidateRoot, 'support/A B.txt', 'one')
    write(fx.candidateRoot, 'support/A.B.txt', 'two')
    fx.candidate = buildCandidateManifest({
      root: fx.candidateRoot,
      sourceSha: SOURCE_SHA,
      version: '1.2.3',
    })
    fx.acceptance = { ...fx.acceptance, candidateId: fx.candidate.candidateId }
    assert.throws(
      () => prepare(fx),
      /PROMOTION_ASSET_NAME_COLLISION/,
    )
  } finally {
    fx.cleanup()
  }
})
