import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { fileURLToPath, pathToFileURL } from 'node:url'

const policyPath = fileURLToPath(new URL('../../scripts/release-policy.mjs', import.meta.url))
const releaseWorkflowPath = fileURLToPath(
  new URL('../../.github/workflows/release.yml', import.meta.url),
)
const promotionWorkflowPath = fileURLToPath(
  new URL('../../.github/workflows/promote-native-candidate.yml', import.meta.url),
)
const legacyReleasePath = fileURLToPath(
  new URL('../../scripts/release.js', import.meta.url),
)

async function loadPolicy() {
  return import(`${pathToFileURL(policyPath).href}?case=${Date.now()}-${Math.random()}`)
}

test('D02_PackageChange_DoesNotPublish_01', async () => {
  const { mayPublish } = await loadPolicy()
  const contexts = [
    { event: 'push', operation: 'build', gatePassed: true, manifestVerified: true },
    { event: 'push', operation: 'promote', gatePassed: true, manifestVerified: true },
    { event: 'tag', operation: 'promote', gatePassed: true, manifestVerified: true },
    {
      event: 'workflow_dispatch',
      operation: 'promote',
      gatePassed: false,
      manifestVerified: false,
    },
    {
      event: 'workflow_dispatch',
      operation: 'promote',
      gatePassed: true,
      manifestVerified: true,
    },
  ]

  for (const context of contexts) {
    assert.equal(mayPublish(context), false, JSON.stringify(context))
  }
})

test('D02_ReleaseWorkflow_HasNoPublishPath_02', () => {
  const workflow = readFileSync(releaseWorkflowPath, 'utf8')

  assert.doesNotMatch(workflow, /^\s{2}release:\s*$/m)
  assert.doesNotMatch(workflow, /softprops\/action-gh-release/)
  assert.doesNotMatch(workflow, /contents:\s*write/)
  assert.doesNotMatch(workflow, /make_latest:\s*true/)
  assert.doesNotMatch(workflow, /gh release create/)
})

test('D02_ReleaseWorkflow_StillBuildsSignedCandidates_03', () => {
  const workflow = readFileSync(releaseWorkflowPath, 'utf8')

  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY/)
  assert.match(workflow, /npm run tauri build/)
  assert.match(workflow, /actions\/upload-artifact@v4/)
  assert.match(workflow, /build-candidate-manifest\.mjs/)
  assert.match(workflow, /cc-desk-candidate-\$\{\{ github\.sha \}\}-manifest/)
})

test('D30_Policy_AllowsOnlyFullyVerifiedPromotion_04', async () => {
  const { mayPublish, promotionComplete } = await loadPolicy()
  const good = {
    event: 'workflow_dispatch',
    operation: 'promote',
    gatePassed: true,
    manifestVerified: true,
    sameCandidate: true,
    explicitApproval: true,
    rebuildPerformed: false,
  }
  assert.equal(mayPublish(good), true)
  assert.equal(promotionComplete({ ...good, publishedBytesVerified: true }), true)
  assert.equal(promotionComplete({ ...good, publishedBytesVerified: false }), false)

  for (const key of [
    'gatePassed',
    'manifestVerified',
    'sameCandidate',
    'explicitApproval',
  ]) {
    assert.equal(mayPublish({ ...good, [key]: false }), false, key)
  }
  assert.equal(mayPublish({ ...good, rebuildPerformed: true }), false)
  assert.equal(mayPublish({ ...good, event: 'push' }), false)
})

test('D30_PromotionWorkflow_ReusesCandidateAndReverifiesPublishedBytes_05', () => {
  const workflow = readFileSync(promotionWorkflowPath, 'utf8')

  assert.match(workflow, /contents:\s*write/)
  assert.match(workflow, /actions:\s*read/)
  assert.match(workflow, /environment:\s*native-release-promotion/)
  assert.match(workflow, /promote-candidate\.mjs/)
  assert.match(workflow, /verify-published-promotion\.mjs/)
  assert.match(workflow, /check-promotion-policy\.mjs/)
  assert.match(workflow, /--phase pre/)
  assert.match(workflow, /--phase complete/)
  assert.match(workflow, /verify-updater-manifest\.js/)
  assert.match(workflow, /gh release create/)
  assert.match(workflow, /gh release download/)
  assert.doesNotMatch(workflow, /npm run tauri build/)
  assert.doesNotMatch(workflow, /cargo build/)
})

test('D31_LegacyReleaseScript_CannotPublishOrTag_06', () => {
  const source = readFileSync(legacyReleasePath, 'utf8')
  assert.match(source, /LEGACY_RELEASE_DISABLED_USE_NATIVE_PROMOTION/)
  assert.doesNotMatch(source, /gh release (?:create|edit|upload)/)
  assert.doesNotMatch(source, /git tag/)
  assert.doesNotMatch(source, /git push/)
})
