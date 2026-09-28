import assert from 'node:assert/strict'
import test from 'node:test'
import { checkPromotionPolicy } from '../../scripts/native-cli/check-promotion-policy.mjs'

const base = [
  '--event', 'workflow_dispatch',
  '--operation', 'promote',
  '--gate-passed',
  '--manifest-verified',
  '--same-candidate',
  '--explicit-approval',
  '--no-rebuild',
]

test('D30_PromotionPolicy_PrePublishRequiresAllImmutableGates_01', () => {
  assert.deepEqual(
    checkPromotionPolicy(['--phase', 'pre', ...base]),
    { status: 'PASS', phase: 'pre' },
  )
  assert.throws(
    () => checkPromotionPolicy([
      '--phase', 'pre',
      '--event', 'workflow_dispatch',
      '--operation', 'promote',
      '--gate-passed',
      '--manifest-verified',
      '--explicit-approval',
      '--no-rebuild',
    ]),
    /PROMOTION_POLICY_REJECTED/,
  )
})

test('D30_PromotionPolicy_CompletionRequiresPublishedByteVerification_02', () => {
  assert.throws(
    () => checkPromotionPolicy(['--phase', 'complete', ...base]),
    /PROMOTION_POLICY_REJECTED/,
  )
  assert.deepEqual(
    checkPromotionPolicy([
      '--phase', 'complete',
      ...base,
      '--published-bytes-verified',
    ]),
    { status: 'PASS', phase: 'complete' },
  )
})
