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

function allowedContext() {
  const candidateId = 'a'.repeat(64)
  return {
    event: 'workflow_dispatch',
    operation: 'promote',
    gatePassed: true,
    manifestVerified: true,
    sameCandidate: true,
    rebuilt: false,
    tagMatchesVersion: true,
    candidateId,
    approval: `PROMOTE:${candidateId}`,
  }
}

test('D02_PackageOrTagChange_DoesNotPublish_01', async () => {
  const { mayPublish } = await loadPolicy()
  const safe = allowedContext()
  for (const context of [
    { ...safe, event: 'push' },
    { ...safe, event: 'tag' },
    { ...safe, operation: 'build' },
    { ...safe, operation: 'candidate' },
  ]) {
    assert.equal(mayPublish(context), false, JSON.stringify(context))
  }
})

test('D30_OnlyExactManualAcceptedCandidateMayPublish_01', async () => {
  const { mayPublish } = await loadPolicy()
  const safe = allowedContext()
  assert.equal(mayPublish(safe), true)

  for (const mutation of [
    { gatePassed: false },
    { manifestVerified: false },
    { sameCandidate: false },
    { rebuilt: true },
    { tagMatchesVersion: false },
    { candidateId: 'bad' },
    { approval: 'PROMOTE' },
    { event: 'push' },
  ]) {
    assert.equal(mayPublish({ ...safe, ...mutation }), false, JSON.stringify(mutation))
  }
})

test('D02_CandidateWorkflow_HasNoPublishPath_02', () => {
  const workflow = readFileSync(releaseWorkflowPath, 'utf8')

  assert.doesNotMatch(workflow, /^\s{2}release:\s*$/m)
  assert.doesNotMatch(workflow, /softprops\/action-gh-release/)
  assert.doesNotMatch(workflow, /contents:\s*write/)
  assert.doesNotMatch(workflow, /make_latest:\s*true/)
  assert.doesNotMatch(workflow, /gh release create/)
  assert.doesNotMatch(workflow, /gh release edit/)
})

test('D30_CandidateWorkflow_FreezesSignedCandidateIdentity_02', () => {
  const workflow = readFileSync(releaseWorkflowPath, 'utf8')

  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY/)
  assert.match(workflow, /npm run tauri build/)
  assert.match(workflow, /candidate-manifest\.mjs/)
  assert.match(workflow, /cc-desk-candidate-manifest/)
  assert.match(workflow, /actions\/upload-artifact@v4/)
})

test('D30_PromotionWorkflow_IsManualGateBoundAndNeverBuilds_03', () => {
  const workflow = readFileSync(promotionWorkflowPath, 'utf8')

  assert.match(workflow, /workflow_dispatch:/)
  assert.match(workflow, /contents:\s*write/)
  assert.match(workflow, /environment:\s*release-promotion/)
  assert.match(workflow, /prepare-promotion\.mjs/)
  assert.match(workflow, /native-cli-acceptance-result-/)
  assert.match(workflow, /gh release create/)
  assert.match(workflow, /--draft/)
  assert.match(workflow, /verifyPublishedPromotion/)
  assert.match(workflow, /gh release edit/)
  assert.match(workflow, /--draft=false/)
  assert.doesNotMatch(workflow, /npm run tauri build/)
  assert.doesNotMatch(workflow, /cargo build/)
  assert.doesNotMatch(workflow, /tauri build/)
  assert.doesNotMatch(workflow, /^\s*push:\s*$/m)
  assert.doesNotMatch(workflow, /^\s*schedule:\s*$/m)
})
