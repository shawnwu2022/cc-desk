import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

describe('prepared package version', () => {
  // npm、Cargo 与 Tauri 必须标识同一个准备版本，不能混入依赖版本。
  it('PackageVersion_Identity_001', () => {
    const manifest = JSON.parse(readFileSync('package.json', 'utf8'))
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
      npm: '0.18.1',
      npmLock: '0.18.1',
      npmRootPackage: '0.18.1',
      cargo: '0.18.1',
      cargoLock: '0.18.1',
      tauri: '0.18.1',
    })
  })

  // 最新版本说明保留未发布身份，旧 0.18.0 仍明确是测试构建。
  it('PackageVersion_Changelog_002', () => {
    const changelog = readFileSync('CHANGELOG.md', 'utf8')
    const headings = [...changelog.matchAll(/^## \[([^\]]+)\] - (.+)$/gm)]
    expect(headings[0]?.[1]).toBe('0.18.1')
    expect(headings[0]?.[2]).toContain('(unreleased candidate)')
    expect(headings.find(match => match[1] === '0.18.0')?.[2])
      .toBe('2026-10-01 (unreleased test build)')
  })
})
