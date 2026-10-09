import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

// 工作流策略检查：核心检查必须与已验证的普通 CI 和测试包工具链一致。
describe('Validated Rust workflow toolchain', () => {
  it.each([
    'ci.yml',
    'conpty-integration.yml',
    'd12-scope-core.yml',
    'd14-transport-core.yml',
    'd17-input-core.yml',
    'paste-cli-acceptance.yml',
  ])('CI_RustToolchain_001 %s', workflow => {
    const source = readFileSync(`.github/workflows/${workflow}`, 'utf8')
    const versions = [...source.matchAll(/^[ \t]*(?:-[ \t]*)?uses:[ \t]+dtolnay\/rust-toolchain@(\S+)[ \t]*\r?$/gm)]
      .map(match => match[1])
    expect(versions.length, `${workflow} must install the validated Rust toolchain`).toBeGreaterThan(0)
    for (const version of versions) {
      expect(version, `${workflow} must use the verified Rust 1.98.1 pin, without floating stable drift`).toBe('1.98.1')
    }
  })
})
