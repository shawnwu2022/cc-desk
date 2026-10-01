import { describe, expect, it } from 'vitest'
import { existsSync, readFileSync, mkdtempSync, mkdirSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'

const workflowPath = '.github/workflows/unified-visual.yml'
function workflow() {
  expect(existsSync(workflowPath), 'final visual verification needs its own non-publishing workflow').toBe(true)
  return readFileSync(workflowPath, 'utf8')
}

// Source-policy checks only: these do not execute GitHub Actions or render Chromium.
describe('Final visual workflow policy', () => {
  it('runs on the final pull request without triggering development checkpoint pushes', () => {
    const source = workflow()
    expect(source).toMatch(/^  pull_request:/m)
    expect(source).toContain('feat/native-cli-finalization')
    expect(source).not.toMatch(/^  (push|pull_request_target|schedule):/m)
    expect(source).toContain('contents: read')
    expect(source).toContain('persist-credentials: false')
    expect(source).not.toMatch(/contents: write|secrets\.|git (?:push|commit)|gh release|action-gh-release/)
  })
  it('installs the locked official browser and records reproducible environment evidence', () => {
    const source = workflow()
    expect(source).toContain('runs-on: ubuntu-24.04')
    expect(source).toContain('npm ci')
    expect(source).toContain('npx playwright install --with-deps chromium')
    expect(source).toContain('fonts-noto-core fonts-noto-cjk fonts-noto-color-emoji')
    expect(source).toContain('github.event.pull_request.head.sha || github.sha')
    expect(source).toContain('git rev-parse HEAD')
    expect(source).toContain('environment.json')
  })
  it('verifies before capturing unapproved candidates and propagates the original failure', () => {
    const source = workflow()
    const verify = source.indexOf('--update-snapshots=none')
    const capture = source.indexOf('--update-snapshots=all')
    expect(verify).toBeGreaterThan(0)
    expect(capture).toBeGreaterThan(verify)
    expect(source).toContain("steps.baselines.outputs.absent == 'true'")
    expect(source).toContain("steps.verify.outcome == 'failure'")
    expect(source).toContain('candidate-baselines-unapproved')
    expect(source).toContain('EXPECTED_BASELINE_COUNT: 13')
    expect(source).toContain("process.exit(process.env.VERIFY_OUTCOME === 'success' ? 0 : 1)")
    expect(source).toContain('VERIFY_OUTCOME: ${{ steps.verify.outcome }}')
    expect(source).toContain('if: always()')
  })
  it('retains traces and failure output and never relaxes production screenshot policy', () => {
    const source = workflow()
    expect(source).toContain('actions/upload-artifact@v4')
    expect(source).toContain('visual-evidence/')
    expect(source).toContain('test-results/visual-verification')
    expect(source).toContain('test-results/visual-candidates')
    const config = readFileSync('playwright.config.ts', 'utf8')
    expect(config).toContain("updateSnapshots: 'none'")
    expect(config).toContain('maxDiffPixels: 0')
  })
  // 执行实际 run 脚本的管道，失败 producer 必须穿透 tee；不启动浏览器。
  it.each(['Verify committed baselines', 'Capture candidates'])('Pipeline_Failure_001 %s', stepName => {
    const source = workflow()
    const step = source.split('      - name: ').find(block => block.startsWith(stepName))!
    const script = step.split('        run: |\n')[1].split('\n').map(line => line.replace(/^          /, '')).join('\n')
      .replace(/^npx playwright .+?(?= 2>&1 \| tee)/m, 'node -e "process.exit(7)"')
    const directory = mkdtempSync(join(tmpdir(), 'visual-pipeline-'))
    mkdirSync(join(directory, 'visual-evidence'))
    try {
      const explicitBash = /^        shell: bash$/m.test(step) || /defaults:\s*\n\s+run:\s*\n\s+shell: bash/.test(source)
      const result = spawnSync('bash', [...(explicitBash ? ['--noprofile', '--norc', '-eo', 'pipefail'] : ['-e']), '-c', script], { cwd: directory })
      expect(result.error).toBeUndefined()
      expect(result.status, 'Playwright exit 7 must remain failure after tee').toBe(7)
    } finally { rmSync(directory, { recursive: true, force: true }) }
  })
  it.each(['success', 'failure', 'skipped', 'cancelled'])('keeps the actual final gate fail-closed for %s', outcome => {
    const source = workflow()
    const block = source.slice(source.lastIndexOf('run: |'))
    const gate = block.match(/<<'NODE'\n([\s\S]*?)\n\s*NODE/)?.[1]
    expect(gate).toBeDefined()
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', gate!], { env: { ...process.env, VERIFY_OUTCOME: outcome } })
    expect(result.error).toBeUndefined()
    expect(result.status === 0).toBe(outcome === 'success')
  })
})

describe('Final Windows package workflow triggers', () => {
  const source = readFileSync('.github/workflows/conpty-integration.yml', 'utf8')
  it('includes unified UX source, persistence, build configuration and focused tests on pull requests', () => {
    const pullRequest = source.split('  pull_request:\n')[1].split('  workflow_dispatch:')[0]
    for (const path of ['src/**', 'src-tauri/src/commands.rs', 'src-tauri/src/lib.rs',
      'src-tauri/src/store.rs', 'src-tauri/src/tests/store.rs', 'src-tauri/tauri.conf.json',
      'package.json', 'package-lock.json', 'build/**', 'vite.config.ts', 'tsconfig*.json',
      'tests/components/**', 'tests/stores/**', 'tests/native-cli/**', 'tests/config/**']) {
      expect(pullRequest, `final package must cover ${path}`).toContain(`      - ${path}\n`)
    }
  })
  it('does not expand the existing development push trigger or publication authority', () => {
    const push = source.split('  push:\n')[1].split('  pull_request:\n')[0]
    expect(push).toContain('branches: [codex/fix-devtools-text-paste]')
    expect([...push.matchAll(/^      - (.+)$/gm)].map(match => match[1])).toEqual([
      'src-tauri/src/main.rs', 'src-tauri/src/conpty_runtime.rs', 'src-tauri/src/tests/conpty_runtime.rs',
      'src-tauri/build.rs', 'src-tauri/conpty/**', 'src-tauri/tauri.windows.conf.json',
      'scripts/*conpty*', 'tests/scripts/conptyBundle.node.cjs', '.github/workflows/conpty-integration.yml',
    ])
    expect(source).toContain('  workflow_dispatch:\n')
    expect(source).toContain('contents: read')
    expect(source).not.toMatch(/contents: write|action-gh-release|make_latest: true/)
    expect(source).toContain('publishable = $false')
    expect(source).toContain('updaterPublication = $false')
  })
})
