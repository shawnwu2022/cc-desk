import assert from 'node:assert/strict'
import test from 'node:test'
import { checkPromotionPolicy } from '../../scripts/native-cli/check-promotion-policy.mjs'

const machine = [
  '--event', 'workflow_dispatch',
  '--operation', 'promote',
  '--gate-passed',
  '--manifest-verified',
  '--same-candidate',
  '--no-rebuild',
]

const approved = [...machine, '--explicit-approval']

test('D30_PromotionPolicy_MachineVerificationPrecedesApproval_01', () => {
  assert.deepEqual(
    checkPromotionPolicy(['--phase', 'verify', ...machine]),
    { status: 'PASS', phase: 'verify' },
  )
  assert.throws(
    () => checkPromotionPolicy([
      '--phase', 'verify',
      '--event', 'workflow_dispatch',
      '--operation', 'promote',
      '--gate-passed',
      '--manifest-verified',
      '--no-rebuild',
    ]),
    /PROMOTION_POLICY_REJECTED/,
  )
  assert.throws(
    () => checkPromotionPolicy(['--phase', 'pre', ...machine]),
    /PROMOTION_POLICY_REJECTED/,
  )
  assert.deepEqual(
    checkPromotionPolicy(['--phase', 'pre', ...approved]),
    { status: 'PASS', phase: 'pre' },
  )
})

test('D30_PromotionPolicy_CompletionRequiresApprovalAndPublishedVerification_02', () => {
  assert.throws(
    () => checkPromotionPolicy(['--phase', 'complete', ...approved]),
    /PROMOTION_POLICY_REJECTED/,
  )
  assert.throws(
    () => checkPromotionPolicy([
      '--phase', 'complete',
      ...approved,
      '--published-bytes-verified',
    ]),
    /PROMOTION_POLICY_REJECTED/,
  )
  assert.deepEqual(
    checkPromotionPolicy([
      '--phase', 'complete',
      ...approved,
      '--published-bytes-verified',
      '--updater-verified',
    ]),
    { status: 'PASS', phase: 'complete' },
  )
})
