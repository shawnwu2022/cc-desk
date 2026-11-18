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

// 保留三平台覆盖，不把不受支持的 macOS runner 取消误当测试成功。
it.each(['d14-transport-core.yml', 'd17-input-core.yml'])('CI_CoreSupportedMatrix_002 %s', workflow => {
  const source = readFileSync(`.github/workflows/${workflow}`, 'utf8')
  expect(source).toMatch(/os: \[ubuntu-22\.04, windows-2022, macos-15\]/)
  expect(source).toMatch(/fail-fast: false/)
})

// Node policy 会编译真实 Rust fixture，须在同 job 显式固定工具链并记录身份。
it('CI_FrontendPolicyRustPrerequisite_003', () => {
  const source = readFileSync('.github/workflows/ci.yml', 'utf8')
  const frontend = source.slice(source.indexOf('  frontend:'), source.indexOf('  rust-compile:'))
  const setup = frontend.indexOf('uses: dtolnay/rust-toolchain@1.98.1')
  const rustc = frontend.indexOf('rustc -Vv')
  const active = frontend.indexOf('rustup show active-toolchain')
  const policy = frontend.indexOf('- name: Run Node policy tests')
  expect(setup).toBeGreaterThan(0)
  expect(rustc).toBeGreaterThan(setup)
  expect(active).toBeGreaterThan(rustc)
  expect(policy).toBeGreaterThan(active)
})
