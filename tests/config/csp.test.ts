import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/**
 * CSP 严格化回归测试
 *
 * 历史状态：app.security.csp 为 null，WebView 不设任何内容安全策略。
 * 本测试锁定 tauri.conf.json 的生产 csp 必须为非空严格策略：
 * - default-src 'self' 基线（前端全部资源本地打包，无 CDN/远程依赖）
 * - 不放开 unsafe-eval / script unsafe-inline（Tauri 编译期为注入脚本自动追加 hash/nonce）
 * - connect-src 放行 Tauri IPC（Windows 走 http://ipc.localhost，其余平台走 ipc:）
 * - style-src 允许运行时注入（Vue 异步组件 chunk 与 xterm 运行期插入 <style>）
 * - 未启用 asset protocol 时不得放行 asset:（convertFileSrc 全仓零使用）
 * - devCsp（仅 dev 生效）放行 Vite HMR websocket
 */
const configPath = resolve(__dirname, '../../src-tauri/tauri.conf.json')
const config = JSON.parse(readFileSync(configPath, 'utf-8'))
const security: { csp?: unknown; devCsp?: unknown } = config.app?.security ?? {}

function directives(policy: unknown): Record<string, string[]> {
  if (typeof policy !== 'string' || policy.length === 0) return {}
  return Object.fromEntries(
    policy.split(';').map(d => d.trim()).filter(Boolean).map(d => {
      const [name, ...sources] = d.split(/\s+/)
      return [name, sources]
    }),
  )
}

describe('Config_CspStrictness', () => {
  it('ProductionCsp_NotNull_001 — 生产 csp 必须为非空字符串策略', () => {
    expect(typeof security.csp).toBe('string')
    expect((security.csp as string).length).toBeGreaterThan(0)
  })

  it('ProductionCsp_DefaultSelfBaseline_002 — 生产 csp 以 default-src self 为基线', () => {
    expect(directives(security.csp)['default-src']).toContain("'self'")
  })

  it('ProductionCsp_NoUnsafeEval_003 — 不放开 unsafe-eval，script-src 不得含 unsafe-inline', () => {
    expect(security.csp).not.toContain('unsafe-eval')
    const scriptSrc = directives(security.csp)['script-src']
    // script-src 未声明时回落 default-src 'self'（Tauri 注入脚本靠编译期 hash/nonce 放行）
    expect(scriptSrc === undefined || !scriptSrc.includes("'unsafe-inline'")).toBe(true)
  })

  it('ProductionCsp_IpcConnectSrc_004 — connect-src 放行 Tauri IPC 双协议', () => {
    const connectSrc = directives(security.csp)['connect-src'] ?? directives(security.csp)['default-src']
    expect(connectSrc).toContain('ipc:')
    expect(connectSrc).toContain('http://ipc.localhost')
  })

  it('ProductionCsp_RuntimeStyleInjection_005 — style-src 允许运行时注入样式', () => {
    const styleSrc = directives(security.csp)['style-src']
    expect(styleSrc).toContain("'unsafe-inline'")
  })

  it('ProductionCsp_ImageSources_006 — img-src 允许本打包资源且未启用 asset protocol 时不放行 asset:', () => {
    const imgSrc = directives(security.csp)['img-src'] ?? directives(security.csp)['default-src']
    expect(imgSrc).toContain("'self'")
    expect(security.csp).not.toContain('asset.localhost')
  })

  it('DevCsp_HmrWebsocket_007 — devCsp 放行 Vite HMR websocket（仅 dev 生效，生产不受影响）', () => {
    expect(typeof security.devCsp).toBe('string')
    expect(security.devCsp).toContain('ws://localhost:1420')
    expect(security.devCsp).not.toContain('unsafe-eval')
  })
})
