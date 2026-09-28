import assert from 'node:assert/strict'
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { createHash } from 'node:crypto'
import { verifyPublishedPromotion } from '../../scripts/native-cli/verify-published-promotion.mjs'

function sha(value) {
  return createHash('sha256').update(value).digest('hex')
}

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'cc-desk-published-'))
  const bytes = Buffer.from('candidate bytes\n')
  const latest = Buffer.from('{"version":"0.17.7"}\n')
  writeFileSync(join(root, 'CC.Desk-setup.exe'), bytes)
  writeFileSync(join(root, 'latest.json'), latest)
  return {
    root,
    bytes,
    latest,
    promotion: {
      schemaVersion: 1,
      status: 'READY_FOR_PROMOTION',
      candidateId: 'a'.repeat(64),
      tag: 'v0.17.7',
      files: [{
        assetName: 'CC.Desk-setup.exe',
        sha256: sha(bytes),
      }],
    },
    updater: {
      assetName: 'latest.json',
      sha256: sha(latest),
    },
  }
}

test('D30_PublishedAssets_MustMatchPromotionAndUpdaterHashes_01', () => {
  const value = fixture()
  assert.deepEqual(verifyPublishedPromotion(value.root, value.promotion, value.updater), {
    status: 'PASS',
    candidateId: 'a'.repeat(64),
    tag: 'v0.17.7',
    verifiedFiles: 2,
  })

  writeFileSync(join(value.root, 'CC.Desk-setup.exe'), 'tampered\n')
  assert.throws(
    () => verifyPublishedPromotion(value.root, value.promotion, value.updater),
    /PUBLISHED_ASSET_HASH_MISMATCH/,
  )
})

test('D30_PublishedAssets_RejectUnexpectedOrMissingReleaseAsset_02', () => {
  const extra = fixture()
  writeFileSync(join(extra.root, 'unexpected.bin'), 'unexpected')
  assert.throws(
    () => verifyPublishedPromotion(extra.root, extra.promotion, extra.updater),
    /PUBLISHED_ASSET_SET_MISMATCH/,
  )

  const wrongUpdater = fixture()
  writeFileSync(join(wrongUpdater.root, 'latest.json'), 'tampered\n')
  assert.throws(
    () => verifyPublishedPromotion(wrongUpdater.root, wrongUpdater.promotion, wrongUpdater.updater),
    /PUBLISHED_UPDATER_HASH_MISMATCH/,
  )
})
