import { describe, expect, it } from 'vitest'
import { existsSync, readFileSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'

const workflowPath = '.github/workflows/unified-visual.yml'
// Run the actual policy and shell checks with both checkout encodings on every OS.
const lineEndings = [{ name: 'LF', value: '\n' }, { name: 'CRLF', value: '\r\n' }]

function workflow(lineEnding: string) {
  expect(existsSync(workflowPath), 'final visual verification needs its own non-publishing workflow').toBe(true)
  return readFileSync(workflowPath, 'utf8').replace(/\r?\n/g, lineEnding).replace(/\r\n/g, '\n')
}

// Source-policy checks only: these do not execute GitHub Actions or render Chromium.
describe.each(lineEndings)('Final visual workflow policy ($name)', ({ value: lineEnding }) => {
  it('runs on the final pull request without triggering development checkpoint pushes', () => {
    const source = workflow(lineEnding)
    expect(source).toMatch(/^  pull_request:/m)
    expect(source).toContain('feat/native-cli-finalization')
    expect(source).not.toMatch(/^  (push|pull_request_target|schedule):/m)
    expect(source).toContain('contents: read')
    expect(source).toContain('persist-credentials: false')
    expect(source).not.toMatch(/contents: write|secrets\.|git (?:push|commit)|gh release|action-gh-release/)
  })
  it('installs the locked official browser and records reproducible environment evidence', () => {
    const source = workflow(lineEnding)
    expect(source).toContain('runs-on: ubuntu-24.04')
    expect(source).toContain('npm ci')
    expect(source).toContain('npx playwright install --with-deps chromium')
    expect(source).toContain('fonts-noto-core fonts-noto-cjk fonts-noto-color-emoji')
    expect(source).toContain('github.event.pull_request.head.sha || github.sha')
    expect(source).toContain('git rev-parse HEAD')
    expect(source).toContain('environment.json')
  })
  it('verifies before capturing unapproved candidates and propagates the original failure', () => {
    const source = workflow(lineEnding)
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
    const source = workflow(lineEnding)
    expect(source).toContain('actions/upload-artifact@v4')
    expect(source).toContain('visual-evidence/')
    expect(source).toContain('test-results/visual-verification')
    expect(source).toContain('test-results/visual-candidates')
    const config = readFileSync('playwright.config.ts', 'utf8')
    expect(config).toContain("updateSnapshots: 'none'")
    expect(config).toContain('maxDiffPixels: 0')
  })
  // 执行工作流中的真实 inventory gate；无关 accessible name 不能变成快照文件。
  it.each(['absent', 'complete', 'partial', 'unexpected', 'duplicate', 'missing-row', 'missing-declaration'])('Inventory_DeclaredRows_002 %s', inventory => {
    const step = workflow(lineEnding).split('      - name: ').find(block => block.startsWith('Check baseline inventory'))!
    const gate = step.match(/<<'NODE'\n([\s\S]*?)\n\s*NODE/)?.[1]
    expect(gate).toBeDefined()
    const names = ['workspace-empty-1024-zh', 'workspace-mixed-1366-zh', 'workspace-hover-action-1366-en',
      'workspace-resources-overlay-1024', 'projects-150-percent', 'new-session-dialog', 'archived-sessions',
      'settings-terminal-light-gui-dark-terminal', 'settings-launch-configurations', 'confirm-stop-and-archive',
      'workspace-menu-1024-en', 'workspace-dark-gui-light-terminal', 'tooltip-transformed-1024']
    let spec = readFileSync('tests/visual/unified-workspace.spec.ts', 'utf8')
      + "\npage.getByRole('textbox', { name: 'Unrelated accessible name', exact: true })\n"
    if (inventory === 'duplicate') spec = spec.replace("name: 'workspace-empty-1024-zh'", "name: 'workspace-mixed-1366-zh'")
    if (inventory === 'missing-row') spec = spec.replace(/^  \{ name: 'workspace-empty-1024-zh'.*\r?\n/m, '')
    if (inventory === 'missing-declaration') spec = spec.replace('const snapshots = [', 'const unrelated = [')
    const directory = mkdtempSync(join(tmpdir(), 'visual-inventory-'))
    const output = join(directory, 'github-output')
    const visual = join(directory, 'tests', 'visual')
    mkdirSync(visual, { recursive: true })
    writeFileSync(join(visual, 'unified-workspace.spec.ts'), spec.replace(/\r?\n/g, lineEnding))
    if (['complete', 'partial', 'unexpected'].includes(inventory)) {
      const snapshots = join(visual, '__screenshots__'); mkdirSync(snapshots)
      const files = inventory === 'partial' ? names.slice(1) : inventory === 'unexpected' ? ['unexpected', ...names.slice(1)] : names
      for (const name of files) writeFileSync(join(snapshots, `${name}.png`), '')
    }
    try {
      const result = spawnSync(process.execPath, ['--input-type=module', '-e', gate!], {
        cwd: directory, encoding: 'utf8', env: { ...process.env, EXPECTED_BASELINE_COUNT: '13', GITHUB_OUTPUT: output },
      })
      expect(result.error).toBeUndefined()
      if (inventory === 'absent' || inventory === 'complete') {
        expect(result.status, result.stderr).toBe(0)
        expect(readFileSync(output, 'utf8')).toBe(`absent=${inventory === 'absent'}\n`)
      } else {
        expect(result.status, `${inventory} must fail closed`).not.toBe(0)
        expect(existsSync(output), 'rejected inventory must not publish admission output').toBe(false)
      }
    } finally { rmSync(directory, { recursive: true, force: true }) }
  })
  // 执行实际 run 脚本的管道，失败 producer 必须穿透 tee；不启动浏览器。
  it.each(['Verify committed baselines', 'Capture candidates'])('Pipeline_Failure_001 %s', stepName => {
    const source = workflow(lineEnding)
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
    const source = workflow(lineEnding)
    const block = source.slice(source.lastIndexOf('run: |'))
    const gate = block.match(/<<'NODE'\n([\s\S]*?)\n\s*NODE/)?.[1]
    expect(gate).toBeDefined()
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', gate!], { env: { ...process.env, VERIFY_OUTCOME: outcome } })
    expect(result.error).toBeUndefined()
    expect(result.status === 0).toBe(outcome === 'success')
  })
})

describe.each(lineEndings)('Final Windows package workflow triggers ($name)', ({ value: lineEnding }) => {
  const source = readFileSync('.github/workflows/conpty-integration.yml', 'utf8').replace(/\r?\n/g, lineEnding).replace(/\r\n/g, '\n')
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
