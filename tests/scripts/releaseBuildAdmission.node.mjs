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
]) {
  test(`ReleaseBuildAdmission_CLI_${name}`, () => {
    assert.ok(existsSync(script), 'exact-source build admission CLI is required')
    const directory = mkdtempSync(join(tmpdir(), 'release-admission-'))
    const output = join(directory, 'github-output')
    const harness = `
      globalThis.fetch = async (url, options) => {
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
      }
    } finally { rmSync(directory, { recursive: true, force: true }) }
  })
}
