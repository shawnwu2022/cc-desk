import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../../', import.meta.url))
const script = new URL('../../scripts/release-build-admission.mjs', import.meta.url)
const sha = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim()
const admitted = () => ({ event: 'push', ref: 'refs/heads/main', sha, checkoutSha: sha,
  main: { sha, protected: true }, versions: ['1.2.3', '1.2.3', '1.2.3'] })

async function admission() {
  assert.ok(existsSync(script), 'parallel candidate builds require an independent exact-source admission gate')
  return import(script.href)
}

test('ReleaseBuildAdmission_ProtectedExactSource_001', async () => {
  const { assertBuildAdmission } = await admission()
  assert.doesNotThrow(() => assertBuildAdmission(admitted()))
  assert.doesNotThrow(() => assertBuildAdmission({ ...admitted(), event: 'workflow_dispatch' }))
})

test('ReleaseBuildAdmission_ReleaseCandidateRequiresProtectedExactSource_020', async () => {
  const { assertBuildAdmission } = await admission()
  const candidate = { ...admitted(), versions: ['0.24.0-rc.1', '0.24.0-rc.1', '0.24.0-rc.1'] }
  assert.doesNotThrow(() => assertBuildAdmission(candidate))
  candidate.main = { ...candidate.main, protected: false }
  assert.throws(() => assertBuildAdmission(candidate), /build admission blocked/)
  candidate.main.protected = true
  candidate.versions = ['0.24.0-rc.1', '0.24.0', '0.24.0-rc.1']
  assert.throws(() => assertBuildAdmission(candidate), /build admission blocked/)
})

// Each refusal would fail if the corresponding source check were removed.
for (const [name, mutate] of [
  ['main advanced', c => { c.main.sha = '2'.repeat(40) }],
  ['branch unprotected', c => { c.main.protected = false }],
  ['protection unknown', c => { delete c.main.protected }],
  ['checkout differs', c => { c.checkoutSha = '2'.repeat(40) }],
  ['feature branch', c => { c.ref = 'refs/heads/feature' }],
  ['pull request', c => { c.event = 'pull_request' }],
  ['tag event', c => { c.ref = 'refs/tags/v1.2.3' }],
  ['invalid source', c => { c.sha = '1'.repeat(39) }],
  ['version mismatch', c => { c.versions[1] = '1.2.4' }],
  ['version missing', c => { c.versions.pop() }],
  ['version unknown', c => { c.versions = [undefined, undefined, undefined] }],
  ['version trailing newline', c => { c.versions = ['1.2.3\n', '1.2.3\n', '1.2.3\n'] }],
  ['version unsafe', c => { c.versions = ['1.2.3\noutput=injected', '1.2.3\noutput=injected', '1.2.3\noutput=injected'] }],
]) {
  test(`ReleaseBuildAdmission_Refuses_${name}`, async () => {
    const { assertBuildAdmission } = await admission()
    const context = admitted(); mutate(context)
    assert.throws(() => assertBuildAdmission(context), /build admission blocked/)
  })
}

// Execute the real CLI; network access is substituted at the HTTP boundary only.
for (const [name, fixture, expected] of [
  ['pending CI is not publication authority', 'valid', 0],
  ['current main changed', 'advanced', 1],
  ['protection unavailable', 'unprotected', 1],
  ['API denied', 'denied', 1],
  ['known failed exact-source CI blocks redundant signed builds', 'failed-ci', 1],
  ['known cancelled exact-source CI blocks redundant signed builds', 'cancelled-ci', 1],
  ['latest pending attempt may build while full preflight waits', 'pending-ci', 0],
  ['CI API failure cannot be treated as pending CI', 'ci-denied', 1],
]) {
  test(`ReleaseBuildAdmission_CLI_${name}`, () => {
    assert.ok(existsSync(script), 'exact-source build admission CLI is required')
    const directory = mkdtempSync(join(tmpdir(), 'release-admission-'))
    const output = join(directory, 'github-output')
    const harness = `
      globalThis.fetch = async (url, options) => {
        if (url === 'https://api.github.com/repos/shawnwu2022/cc-desk/actions/workflows/ci.yml/runs?head_sha=' + process.env.GITHUB_SHA + '&branch=main&event=push&per_page=100') {
          if (process.env.FIXTURE === 'ci-denied') return { ok: false, status: 403 };
          const fixture = process.env.FIXTURE;
          const conclusion = fixture === 'failed-ci' ? 'failure' : fixture === 'cancelled-ci' ? 'cancelled' : null;
          const run = { id: 11, head_sha: process.env.GITHUB_SHA, head_branch: 'main', event: 'push', path: '.github/workflows/ci.yml', status: conclusion ? 'completed' : 'in_progress', conclusion };
          return { ok: true, json: async () => ({ workflow_runs: [run, { ...run, id: 10, status: 'completed', conclusion: 'failure' }] }) };
        }
        if (url !== 'https://api.github.com/repos/shawnwu2022/cc-desk/branches/main'
          || (options.method ?? 'GET') !== 'GET') throw new Error('unexpected admission API');
        if (process.env.FIXTURE === 'denied') return { ok: false, status: 403 };
        return { ok: true, json: async () => ({ protected: process.env.FIXTURE !== 'unprotected',
          commit: { sha: process.env.FIXTURE === 'advanced' ? '2'.repeat(40) : process.env.GITHUB_SHA } }) };
      };
      process.argv = ['node', ${JSON.stringify(fileURLToPath(script))}];
      await import(${JSON.stringify(script.href)});
    `
    try {
      const result = spawnSync(process.execPath, ['--input-type=module', '-e', harness], {
        cwd: root, encoding: 'utf8', timeout: 5000,
        env: { ...process.env, GITHUB_REPOSITORY: 'shawnwu2022/cc-desk', GITHUB_TOKEN: 'synthetic-token',
          GITHUB_SHA: sha, GITHUB_REF: 'refs/heads/main', GITHUB_EVENT_NAME: 'push', GITHUB_OUTPUT: output,
          FIXTURE: fixture },
      })
      assert.ifError(result.error)
      assert.equal(result.status, expected, result.stderr)
      if (expected === 0) {
        const version = JSON.parse(readFileSync(new URL('../../package.json', import.meta.url), 'utf8')).version
        assert.equal(readFileSync(output, 'utf8'), `version=${version}\ntag=v${version}\n`)
        assert.match(result.stdout, /Build source admitted/)
        assert.match(result.stdout, /publication still requires exact CI coverage and signatures/)
      } else {
        assert.equal(existsSync(output), false, 'a refused source cannot publish admission outputs')
        if (fixture === 'failed-ci' || fixture === 'cancelled-ci') assert.match(result.stderr, /Release wait: current-source CI 11 failed/)
        if (fixture === 'ci-denied') assert.match(result.stderr, /build admission CI API failed: HTTP 403/)
      }
    } finally { rmSync(directory, { recursive: true, force: true }) }
  })
}
