import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { fileURLToPath, pathToFileURL } from 'node:url'

const policyPath = fileURLToPath(new URL('../../scripts/release-policy.mjs', import.meta.url))
const releaseWorkflowPath = fileURLToPath(
  new URL('../../.github/workflows/release.yml', import.meta.url),
)
const promotionWorkflowPath = fileURLToPath(
  new URL('../../.github/workflows/promote-release.yml', import.meta.url),
)

async function loadPolicy() {
  return import(`${pathToFileURL(policyPath).href}?case=${Date.now()}-${Math.random()}`)
}

test('D02_PackageChange_DoesNotPublish_01', async () => {
  const { mayPublish } = await loadPolicy()
  for (const context of [
    { event: 'push', operation: 'promote', gatePassed: true, manifestVerified: true, sameCandidate: true, explicitApproval: true },
    { event: 'tag', operation: 'promote', gatePassed: true, manifestVerified: true, sameCandidate: true, explicitApproval: true },
    { event: 'workflow_dispatch', operation: 'promote', gatePassed: false, manifestVerified: true, sameCandidate: true, explicitApproval: true },
    { event: 'workflow_dispatch', operation: 'promote', gatePassed: true, manifestVerified: false, sameCandidate: true, explicitApproval: true },
    { event: 'workflow_dispatch', operation: 'promote', gatePassed: true, manifestVerified: true, sameCandidate: false, explicitApproval: true },
    { event: 'workflow_dispatch', operation: 'promote', gatePassed: true, manifestVerified: true, sameCandidate: true, explicitApproval: false },
  ]) {
    assert.equal(mayPublish(context), false, JSON.stringify(context))
  }
})

test('D30_OnlyExplicitAcceptedCandidatePromotionCanPublish_02', async () => {
  const { mayPublish } = await loadPolicy()
  assert.equal(mayPublish({
    event: 'workflow_dispatch',
    operation: 'promote',
    gatePassed: true,
    manifestVerified: true,
    sameCandidate: true,
    explicitApproval: true,
  }), true)
})

test('D30_CandidateWorkflow_HasNoPublishPath_03', () => {
  const workflow = readFileSync(releaseWorkflowPath, 'utf8')
  assert.match(workflow, /workflow_dispatch/)
  assert.doesNotMatch(workflow, /^\s{2}push:\s*$/m)
  assert.doesNotMatch(workflow, /softprops\/action-gh-release/)
  assert.doesNotMatch(workflow, /contents:\s*write/)
  assert.doesNotMatch(workflow, /gh release create/)
  assert.match(workflow, /candidate-manifest\.mjs/)
})

test('D30_PromotionWorkflow_HasNoBuildAndRequiresExactGate_04', () => {
  const workflow = readFileSync(promotionWorkflowPath, 'utf8')
  assert.match(workflow, /environment:\s*release-promotion/)
  assert.match(workflow, /verify-promotion\.mjs/)
  assert.match(workflow, /verify-updater-manifest\.js/)
  assert.match(workflow, /gh release create/)
  assert.match(workflow, /--latest=false/)
  assert.match(workflow, /gh release edit .*--latest/)
  assert.doesNotMatch(workflow, /npm run tauri build/)
  assert.doesNotMatch(workflow, /cargo build/)
})
