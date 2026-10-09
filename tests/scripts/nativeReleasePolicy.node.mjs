import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { fileURLToPath, pathToFileURL } from 'node:url'

const legacyReleasePath = fileURLToPath(new URL('../../scripts/release.js', import.meta.url))
const packagePath = fileURLToPath(new URL('../../package.json', import.meta.url))

const policyPath = fileURLToPath(new URL('../../scripts/release-policy.mjs', import.meta.url))
const releaseWorkflowPath = fileURLToPath(
  new URL('../../.github/workflows/release.yml', import.meta.url),
)
const testPackageWorkflowPath = fileURLToPath(
  new URL('../../.github/workflows/conpty-integration.yml', import.meta.url),
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

test('D02_ReleaseWorkflow_RequiresProtectedMainAndDisclosedCI_02', () => {
  const workflow = readFileSync(releaseWorkflowPath, 'utf8')

  assert.match(workflow, /needs: \[preflight, build\]/)
  assert.match(workflow, /github\.ref == 'refs\/heads\/main' && github\.ref_protected/)
  assert.match(workflow, /node scripts\/release-preflight\.mjs --artifacts/)
  assert.match(workflow, /--coverage coverage\/windows-native-coverage\.json/)
  assert.match(workflow, /--resolve-coverage/)
  assert.match(workflow, /pattern: cc-desk-candidate-\$\{\{ github\.sha \}\}-\*/)
  assert.equal((workflow.match(/contents: write/g) ?? []).length, 1)
  assert.match(workflow, /fail_on_unmatched_files: true/)
})

test('D02_ReleaseWorkflow_StillBuildsSignedCandidates_03', () => {
  const workflow = readFileSync(releaseWorkflowPath, 'utf8')

  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY/)
  assert.match(workflow, /npm run tauri build/)
  assert.match(workflow, /actions\/upload-artifact@v4/)
})

test('D28_TestPackageWorkflow_RemainsTestOnly_04', () => {
  const workflow = readFileSync(testPackageWorkflowPath, 'utf8')

  assert.match(workflow, /channel = 'test-only'/)
  assert.match(workflow, /publishable = \$false/)
  assert.match(workflow, /updaterPublication = \$false/)
  assert.match(workflow, /createUpdaterArtifacts\":false/)
  assert.match(workflow, /actions\/upload-artifact@v4/)
  assert.doesNotMatch(workflow, /softprops\/action-gh-release/)
  assert.doesNotMatch(workflow, /contents:\s*write/)
  assert.doesNotMatch(workflow, /make_latest:\s*true/)
})

// 在任何子进程执行前，检查旧入口只包含固定提示与失败退出码。
// 该 allowlist 故意拒绝新增依赖、分支和副作用；RED 阶段绝不执行旧发布器。
function assertDisabledLegacyShim() {
  const source = readFileSync(legacyReleasePath, 'utf8')
  assert.ok(
    /^#!\/usr\/bin\/env node\r?\n\s*console\.error\(\s*'[^'\\\r\n]*'\s*(?:\+\s*'[^'\\\r\n]*'\s*)*,?\s*\)\s*;?\s*process\.exitCode\s*=\s*1\s*;?\s*$/.test(source),
    'Legacy release entry must be a side-effect-free refusal shim before it can be executed',
  )
}

// 检查旧发布器不存在可执行发布、镜像上传或配置修改路径。
test('Release_LegacyShim_IsInert_005', () => {
  assertDisabledLegacyShim()
})

// 检查旧参数（包括跳过 CI、自动确认、OSS 与 help）均不能恢复发布。
test('Release_LegacyArgs_Reject_006', () => {
  assertDisabledLegacyShim()
  const cases = [
    [],
    ['--help'],
    ['-h'],
    ['--bump', 'patch', '--notes', 'test'],
    ['--exact', '--notes', 'test', '--skip-ci', '--yes'],
    ['--oss-only'],
    ['--oss-only', 'v0.18.1'],
    ['--force', '--promote'],
  ]

  for (const args of cases) {
    const result = spawnSync(process.execPath, [legacyReleasePath, ...args], {
      encoding: 'utf8',
      timeout: 5000,
    })
    assert.ifError(result.error)
    assert.equal(result.signal, null, JSON.stringify(args))
    assert.equal(result.status, 1, JSON.stringify(args))
    assert.equal(result.stdout, '', JSON.stringify(args))
    assert.match(result.stderr, /signed candidates only/)
    assert.match(result.stderr, /promotion is not enabled/i)
    assert.match(result.stderr, /docs\/release-process\.md/)
  }
})

// 检查两个 npm 入口仍直达同一个拒绝 shim，且没有前后置执行逃逸。
test('Release_NpmEntries_Reject_007', () => {
  assertDisabledLegacyShim()
  const { scripts } = JSON.parse(readFileSync(packagePath, 'utf8'))
  const entries = [
    ['release', 'node scripts/release.js', []],
    ['release:oss', 'node scripts/release.js --oss-only', ['--oss-only', 'v0.18.1']],
  ]

  for (const [name, command, args] of entries) {
    assert.equal(scripts[name], command, `${name} must route to the disabled legacy shim`)
    assert.equal(scripts[`pre${name}`], undefined, `${name} must not have a pre-publish escape`)
    assert.equal(scripts[`post${name}`], undefined, `${name} must not have a post-publish escape`)
    const result = spawnSync(process.execPath, [legacyReleasePath, ...args], {
      encoding: 'utf8',
      timeout: 5000,
    })
    assert.ifError(result.error)
    assert.equal(result.status, 1, name)
    assert.match(result.stderr, /signed candidates only/)
    assert.match(result.stderr, /promotion is not enabled/i)
    assert.match(result.stderr, /docs\/release-process\.md/)
  }
})
