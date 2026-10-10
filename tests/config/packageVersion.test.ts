import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

describe('prepared package version', () => {
  // npm、Cargo 与 Tauri 必须标识同一个准备版本，不能混入依赖版本。
  it('PackageVersion_Identity_001', () => {
    const manifest = JSON.parse(readFileSync('package.json', 'utf8'))
    expect(__APP_VERSION__).toBe(manifest.version)
    const lock = JSON.parse(readFileSync('package-lock.json', 'utf8'))
    const tauri = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'))
    const cargo = readFileSync('src-tauri/Cargo.toml', 'utf8')
      .split(/^\[/m).find(section => section.startsWith('package]'))
    const cargoLock = readFileSync('src-tauri/Cargo.lock', 'utf8')
      .split('[[package]]').find(section => /^name = "cc-desk"$/m.test(section))

    expect({
      npm: manifest.version,
      npmLock: lock.version,
      npmRootPackage: lock.packages[''].version,
      cargo: cargo?.match(/^version = "([^"]+)"$/m)?.[1],
      cargoLock: cargoLock?.match(/^version = "([^"]+)"$/m)?.[1],
      tauri: tauri.version,
    }).toEqual({
      npm: '0.18.4',
      npmLock: '0.18.4',
      npmRootPackage: '0.18.4',
      cargo: '0.18.4',
      cargoLock: '0.18.4',
      tauri: '0.18.4',
    })
  })

  // 最新版本说明保留未发布身份，旧 0.18.0 仍明确是测试构建。
  it('PackageVersion_Changelog_002', () => {
    const changelog = readFileSync('CHANGELOG.md', 'utf8')
    const headings = [...changelog.matchAll(/^## \[([^\]]+)\] - (.+)$/gm)]
    expect(headings[0]?.[1]).toBe('0.18.4')
    expect(headings[0]?.[2]).toContain('(unreleased candidate)')
    expect(headings.find(match => match[1] === '0.18.0')?.[2])
      .toBe('2026-10-01 (unreleased test build)')
  })

  // 普通 Windows scope fixture 跟随编译版本，避免版本准备先于路径检查拒绝测试数据。
  it('PackageVersion_ScopeFixture_003', () => {
    const fixture = readFileSync('src-tauri/src/tests/version_history_scope.rs', 'utf8')
    expect(fixture).toMatch(/\("DisplayVersion",\s*env!\("CARGO_PKG_VERSION"\)\.into\(\)\)/)
    expect(fixture).not.toMatch(/\("DisplayVersion",\s*"\d+\.\d+\.\d+"\.into\(\)\)/)
  })

  // fixture 更新不能放宽生产注册版本匹配，也不能改写已有 0.18.0 roundtrip 绑定。
  it('PackageVersion_ScopeAdmission_004', () => {
    const scope = readFileSync('src-tauri/src/version_history/windows/scope.rs', 'utf8')
    const registration = scope.split('fn registered_directory(')[1]?.split('fn verify_x64_header(')[0]
    expect(registration).toMatch(/\|\| text_field\(record, "DisplayVersion"\)\?\.as_deref\(\) != Some\(env!\("CARGO_PKG_VERSION"\)\)/)
    expect(registration).toMatch(/"DisplayVersion"[\s\S]*?\{\s*return Err\(ScopeBlock::UnsupportedRegistration\);\s*\}/)
    const acceptance = readFileSync('src-tauri/src/version_history/acceptance.rs', 'utf8')
    expect(acceptance).toContain('env!("CARGO_PKG_VERSION") != "0.18.0"')
  })
})
