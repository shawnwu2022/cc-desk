import assert from 'node:assert/strict'
import test from 'node:test'
import { verifyWorkflowRun } from '../../scripts/native-cli/verify-workflow-run.mjs'

function run(overrides = {}) {
  return {
    id: 123,
    path: '.github/workflows/release.yml',
    head_sha: 'a'.repeat(40),
    status: 'completed',
    conclusion: 'success',
    event: 'workflow_dispatch',
    ...overrides,
  }
}

const options = {
  expectedPath: '.github/workflows/release.yml',
  expectedSha: 'a'.repeat(40),
  allowedEvents: ['workflow_dispatch', 'push'],
}

test('D29_RunProvenance_ExactWorkflowHeadAndSuccessPass_01', () => {
  assert.deepEqual(verifyWorkflowRun(run(), options), {
    status: 'PASS',
    runId: 123,
    path: '.github/workflows/release.yml',
    headSha: 'a'.repeat(40),
    event: 'workflow_dispatch',
  })
})

test('D29_RunProvenance_WrongWorkflowCannotForgeArtifactSource_02', () => {
  assert.throws(
    () => verifyWorkflowRun(run({ path: '.github/workflows/ci.yml' }), options),
    /RUN_PROVENANCE_WORKFLOW_MISMATCH/,
  )
})

test('D29_RunProvenance_WrongHeadOrFailedRunRejects_03', () => {
  assert.throws(
    () => verifyWorkflowRun(run({ head_sha: 'b'.repeat(40) }), options),
    /RUN_PROVENANCE_HEAD_MISMATCH/,
  )
  assert.throws(
    () => verifyWorkflowRun(run({ conclusion: 'failure' }), options),
    /RUN_PROVENANCE_NOT_SUCCESSFUL/,
  )
})

test('D29_RunProvenance_UnexpectedEventRejects_04', () => {
  assert.throws(
    () => verifyWorkflowRun(run({ event: 'schedule' }), options),
    /RUN_PROVENANCE_EVENT_MISMATCH/,
  )
})
