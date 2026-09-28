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

test('D30_PublishedAssets_MustMatchPromotionHashes_01', () => {
  const root = mkdtempSync(join(tmpdir(), 'cc-desk-published-'))
  const bytes = Buffer.from('candidate bytes\n')
  writeFileSync(join(root, 'CC.Desk-setup.exe'), bytes)
  const promotion = {
    schemaVersion: 1,
    status: 'READY_FOR_PROMOTION',
    candidateId: 'a'.repeat(64),
    tag: 'v0.17.7',
    files: [{
      assetName: 'CC.Desk-setup.exe',
      sha256: sha(bytes),
    }],
  }
  assert.deepEqual(verifyPublishedPromotion(root, promotion), {
    status: 'PASS',
    candidateId: 'a'.repeat(64),
    tag: 'v0.17.7',
    verifiedFiles: 1,
  })

  writeFileSync(join(root, 'CC.Desk-setup.exe'), 'tampered\n')
  assert.throws(
    () => verifyPublishedPromotion(root, promotion),
    /PUBLISHED_ASSET_HASH_MISMATCH/,
  )
})
