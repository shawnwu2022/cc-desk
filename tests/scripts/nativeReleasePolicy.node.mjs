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
const legacyReleasePath = fileURLToPath(
  new URL('../../scripts/release.js', import.meta.url),
)
const acceptanceWorkflowPath = fileURLToPath(
  new URL('../../.github/workflows/native-cli-acceptance.yml', import.meta.url),
)
const canaryWorkflowPath = fileURLToPath(
  new URL('../../.github/workflows/native-cli-canary.yml', import.meta.url),
)
const canaryIdentityPath = fileURLToPath(
  new URL('../../scripts/native-cli/canary-identity.mjs', import.meta.url),
)
const releaseTargetsPath = fileURLToPath(
  new URL('../../docs/testing/native-cli-release-targets.json', import.meta.url),
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


test('D31_LegacyDirectReleasePathIsFailClosed_05', () => {
  const source = readFileSync(legacyReleasePath, 'utf8')
  const ossBranch = source.indexOf('if (args.ossOnly)')
  const disabled = source.indexOf('DIRECT_RELEASE_DISABLED_USE_PROMOTION_WORKFLOW')
  const legacyMutation = source.indexOf('// 参数检查：--bump 或 --exact 二选一')
  assert.ok(ossBranch >= 0)
  assert.ok(disabled > ossBranch)
  assert.ok(legacyMutation > disabled)
})


test('D28_AcceptanceWorkflow_BindsCandidateCommitTargetPlanAndEvidenceBytes_06', () => {
  const workflow = readFileSync(acceptanceWorkflowPath, 'utf8')
  assert.match(workflow, /candidate_run_id/)
  assert.match(workflow, /Signed candidate packages/)
  assert.match(workflow, /head_sha/)
  assert.match(workflow, /head_branch/)
  assert.match(workflow, /workflow_dispatch/)
  assert.match(workflow, /Stable candidate must be built from main/)
  assert.match(workflow, /Checkout exact candidate source/)
  assert.match(workflow, /docs\/testing\/native-cli-release-targets\.json/)
  assert.match(workflow, /verify-acceptance\.mjs/)
  assert.match(workflow, /RUNNER_TEMP\/evidence/)
})

test('D29_Canary_NeverClaimsCertification_07', () => {
  const workflow = readFileSync(canaryWorkflowPath, 'utf8')
  const identity = readFileSync(canaryIdentityPath, 'utf8')
  assert.match(identity, /status:\s*'NOT_CERTIFIED'/)
  assert.match(identity, /certificationRequired:\s*true/)
  assert.match(workflow, /"status":"BLOCKED"/)
  assert.doesNotMatch(identity, /status:\s*'PASS'/)
  assert.doesNotMatch(workflow, /"status":"PASS"/)
})

test('D30_Promotion_PublishesOnlyVerifierListedFiles_08', () => {
  const workflow = readFileSync(promotionWorkflowPath, 'utf8')
  assert.match(workflow, /promotion-plan\.json/)
  assert.match(workflow, /relative_files/)
  assert.match(workflow, /Verified candidate artifact set must contain exactly seven files/)
  assert.match(workflow, /Signed candidate packages/)
  assert.match(workflow, /head_sha/)
  assert.match(workflow, /head_branch/)
  assert.match(workflow, /workflow_dispatch/)
  assert.match(workflow, /Stable candidate must be built from main/)
  assert.doesNotMatch(workflow, /find candidate -type f \\\(/)
})

test('D31_ReleaseTargets_DefaultToBlocked_09', () => {
  const value = JSON.parse(readFileSync(releaseTargetsPath, 'utf8'))
  assert.equal(value.schemaVersion, 1)
  assert.equal(value.status, 'BLOCKED')
  assert.deepEqual(value.targets, [])
})
