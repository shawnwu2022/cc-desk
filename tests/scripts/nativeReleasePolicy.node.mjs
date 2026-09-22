import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { fileURLToPath, pathToFileURL } from 'node:url'

const policyPath = fileURLToPath(new URL('../../scripts/release-policy.mjs', import.meta.url))
const releaseWorkflowPath = fileURLToPath(
  new URL('../../.github/workflows/release.yml', import.meta.url),
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
  assert.doesNotMatch(workflow, /Generate updater manifest/)
  assert.doesNotMatch(workflow, /Publish GitHub Release/)
  assert.doesNotMatch(workflow, /Verify published update channel/)
})

test('D02_ReleaseWorkflow_StillBuildsSignedCandidates_03', () => {
  const workflow = readFileSync(releaseWorkflowPath, 'utf8')

  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY/)
  assert.match(workflow, /npm run tauri build/)
  assert.match(workflow, /actions\/upload-artifact@v4/)
})
