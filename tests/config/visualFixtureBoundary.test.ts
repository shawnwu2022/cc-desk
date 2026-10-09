import { describe, expect, it } from 'vitest'
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { visualFixturePlugin } from '../../build/visualFixture'

// 真实插件路由和模块解析必须在普通开发/生产关闭；不调用Vite内部或真实宿主。
describe('Visual fixture module boundary', () => {
  it.each([
    { command: 'serve' as const, mode: 'visual', flag: '1', fixture: true },
    { command: 'serve' as const, mode: 'visual', flag: '0', fixture: false },
    { command: 'serve' as const, mode: 'development', flag: '1', fixture: false },
    { command: 'build' as const, mode: 'visual', flag: '1', fixture: false },
    { command: 'build' as const, mode: 'production', flag: '1', fixture: false },
  ])('VisualGate_VersionIsFrozenOnlyInsideExplicitFixture_005: $command $mode $flag', async ({ command, mode, flag, fixture }) => {
    // Run the actual Vite resolver in Node, independently of jsdom's typed-array realm.
    const definitions = JSON.parse(execFileSync(process.execPath, ['--input-type=module', '-e', `
      import { resolveConfig } from 'vite';
      const config = await resolveConfig({ configFile: 'vite.config.ts', mode: process.argv[2] }, process.argv[1], process.argv[2]);
      console.log(JSON.stringify(config.define));
    `, command, mode], { encoding: 'utf8', timeout: 10000, env: { ...process.env, CC_DESK_VISUAL_FIXTURE: flag } }))
    const actualVersion = JSON.parse(readFileSync('package.json', 'utf8')).version
    expect(definitions.__APP_VERSION__).toBe(JSON.stringify(fixture ? '0.18.1' : actualVersion))
  })
  it('VisualGate_RequiresServeModeAndFlag_001', () => {
    const source = readFileSync('vite.config.ts', 'utf8')
    expect(source).toContain("command === 'serve' && mode === 'visual' && process.env.CC_DESK_VISUAL_FIXTURE === '1'")
    expect(readFileSync('src/main.ts', 'utf8')).not.toContain('visual')
    expect(readFileSync('src/App.vue', 'utf8')).not.toContain('visual')
  })
  it('VisualGate_RequiresReviewedBaselines_004', () => {
    const config = readFileSync('playwright.config.ts', 'utf8')
    expect(config).toContain("updateSnapshots: 'none'")
    expect(config).toContain('maxDiffPixels: 0')
    const spec = readFileSync('tests/visual/unified-workspace.spec.ts', 'utf8')
    for (const name of ['workspace-empty-1024-zh', 'workspace-mixed-1366-zh', 'workspace-hover-action-1366-en',
      'workspace-resources-overlay-1024', 'projects-150-percent', 'new-session-dialog', 'archived-sessions',
      'settings-terminal-light-gui-dark-terminal', 'settings-launch-configurations', 'confirm-stop-and-archive']) expect(spec).toContain(name)
  })
  it.each([false, true])('VisualGate_EnforcesModuleBoundary_002: enabled=%s', enabled => {
    const root = resolve('visual-fixture-repo')
    const plugin = visualFixturePlugin(root, enabled) as any
    expect(plugin.resolveId('@tauri-apps/api/core')).toBe(enabled ? join(root, 'src', 'visual', 'tauriStub.ts') : undefined)
    expect(plugin.resolveId('@/stores/app')).toBeUndefined()
    for (const id of [join(root, 'src', 'visual', 'entry.ts'), '/repo/src/visual/entry.ts', 'C:\\repo\\src\\visual\\entry.ts']) {
      if (enabled) expect(plugin.load(id)).toBeUndefined()
      else expect(() => plugin.load(id)).toThrow('VISUAL_FIXTURE_DISABLED')
    }
  })
  it.each([false, true])('VisualGate_ServesOnlyExplicitRoute_003: enabled=%s', enabled => {
    let middleware!: (request: { url: string }, response: any, next: () => void) => void
    ;(visualFixturePlugin('/repo', enabled) as any).configureServer({ middlewares: { use: (value: typeof middleware) => { middleware = value } } })
    function request(url: string) {
      let next = false; let body = ''; const response = { statusCode: 200, setHeader() {}, end(text: string) { body = text } }
      middleware({ url }, response, () => { next = true }); return { ...response, next, body }
    }
    expect(request('/__visual__/')).toMatchObject(enabled ? { statusCode: 200, next: false } : { statusCode: 404, next: false })
    if (enabled) expect(request('/__visual__/').body).toContain('/src/visual/entry.ts')
    expect(request('/src/visual/fixtures.ts')).toMatchObject(enabled ? { next: true } : { statusCode: 404, next: false })
    expect(request('/src/main.ts')).toMatchObject(enabled ? { statusCode: 404, next: false } : { next: true })
    expect(request('/src/components/ui/AppButton.vue').next).toBe(true)
  })
})
