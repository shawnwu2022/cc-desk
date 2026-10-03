import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

describe('version manager capability isolation', () => {
  it('VersionManager_NoOrdinaryCapabilityInheritance_001', () => {
    const ordinary = JSON.parse(readFileSync('src-tauri/capabilities/default.json', 'utf8'))
    expect(ordinary.windows).toEqual(['main'])
    expect(ordinary.permissions).not.toContain('core:window:allow-create')
    expect(ordinary.permissions).not.toContain('core:webview:allow-create-webview')
    const manager = JSON.parse(readFileSync('src-tauri/capabilities/version-manager.json', 'utf8'))
    expect(manager.windows).toEqual(['version-manager'])
    expect(manager.permissions).toEqual([])
  })
})
