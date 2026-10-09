import { describe, expect, it } from 'vitest'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { visualFixturePlugin } from '../../build/visualFixture'

describe('isolated manager visual fixture boundary', () => {
  // 真实开发插件只在显式测试路由返回管理器 fixture，普通模式拒绝入口与模块。
  it.each([false, true])('ManagerVisual_ExplicitRoute_001:%s', enabled => {
    let middleware!: (request: { url: string }, response: any, next: () => void) => void
    const plugin = visualFixturePlugin('/repo', enabled) as any
    plugin.configureServer({ middlewares: { use(handler: typeof middleware) { middleware = handler } } })
    const request = (url: string) => {
      let body = ''; let passed = false
      const response = { statusCode: 200, setHeader() {}, end(value: string) { body = value } }
      middleware({ url }, response, () => { passed = true })
      return { status: response.statusCode, body, passed }
    }
    const route = request('/__visual__/version-manager/?phase=installed-unconfirmed&locale=en')
    expect(route).toMatchObject({ status: enabled ? 200 : 404, passed: false })
    if (enabled) expect(route.body).toContain('/src/visual/managerEntry.ts')
    expect(request('/src/visual/managerFixture.ts')).toMatchObject(enabled ? { passed: true } : { status: 404, passed: false })
    for (const file of ['managerEntry.ts', 'managerFixture.ts', 'ManagerVisualFixtureApp.vue']) {
      if (enabled) expect(plugin.load(`/repo/src/visual/${file}`)).toBeUndefined()
      else expect(() => plugin.load(`/repo/src/visual/${file}`)).toThrow('VISUAL_FIXTURE_DISABLED')
    }
    for (const path of ['/version-manager.html', '/src/manager/main.ts']) {
      expect(request(path)).toMatchObject(enabled ? { status: 404, passed: false } : { passed: true })
    }
  })

  // 独立 fixture 的传递模块不能导入普通 App、持久化 store 或 CLI runtime。
  it('ManagerVisual_NoOrdinaryGraph_002', () => {
    expect(existsSync('src/visual/managerEntry.ts')).toBe(true)
    const pending = [resolve('src/visual/managerEntry.ts')]
    const visited = new Set<string>()
    while (pending.length) {
      const file = pending.pop()!
      if (visited.has(file)) continue
      visited.add(file)
      expect(file).not.toMatch(/[/\\](?:stores|terminal)[/\\]|[/\\](?:App|VisualFixtureApp)\.vue$/)
      const source = readFileSync(file, 'utf8')
      expect(source).not.toMatch(/createPinia|from ['"]@tauri-apps|from ['"](?:@\/|\.\.\/)api\//)
      for (const match of source.matchAll(/(?:from\s+|import\s*)['"]([^'"]+)['"]/g)) {
        const name = match[1]
        if (!name.startsWith('.') && !name.startsWith('@/')) continue
        const base = name.startsWith('@/') ? resolve('src', name.slice(2)) : resolve(dirname(file), name)
        const target = [base, `${base}.ts`, `${base}.vue`].find(existsSync)
        expect(target).toBeDefined()
        if (target && !target.endsWith('.css')) pending.push(target)
      }
    }
    const entry = readFileSync('src/visual/managerEntry.ts', 'utf8')
    expect(entry).toContain("import.meta.env.MODE !== 'visual'")
    expect(entry).toContain("location.pathname !== '/__visual__/version-manager/'")
    expect(readFileSync('src/manager/main.ts', 'utf8')).not.toContain('visual')
  })
})
