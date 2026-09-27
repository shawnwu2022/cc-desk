import { existsSync, readFileSync } from 'node:fs'
import { describe, expect, test } from 'vitest'

const read = (path: string) => readFileSync(path, 'utf8')

describe('CC Desk product boundary', () => {
  test('does not ship downstream Provider management', () => {
    expect(existsSync('src-tauri/src/providers.rs')).toBe(false)
    expect(existsSync('src/api/provider.ts')).toBe(false)
    expect(existsSync('src/stores/providers.ts')).toBe(false)
    expect(existsSync('src/types/provider.ts')).toBe(false)
    expect(existsSync('src/config/providerPresets.ts')).toBe(false)
    expect(read('src-tauri/src/lib.rs')).not.toContain('commands::activate_provider')
  })

  test('does not distribute or overwrite Claude and Git binaries', () => {
    expect(existsSync('src-tauri/src/installer.rs')).toBe(false)
    const api = read('src/api/tauri.ts')
    expect(api).not.toContain('downloadAndInstallClaude')
    expect(api).not.toContain('killClaudeProcesses')
    expect(api).not.toContain('listClaudeVersions')
  })

  test('native dual-CLI workbench never falls back to legacy Claude PTY APIs', () => {
    const terminal = read('src/components/NativeCliTerminal.vue')
    expect(terminal).toContain('createNativeLaunchEntry')
    expect(terminal).toContain('createDeskNativeTerminalBinding')
    expect(terminal).toContain('cliResize')
    expect(terminal).toContain('cliStop')
    expect(terminal).not.toContain('ptySpawn')
    expect(terminal).not.toContain('ptyInput')
    expect(terminal).not.toContain('ptyKill')
    expect(terminal).not.toContain('claudeOptions')

    const app = read('src/App.vue')
    expect(app).toContain("appStore.checkFailed && currentView !== 'native'")
    expect(app).toContain("startupError && currentView !== 'native'")
  })

  test('native workbench DOM and IPC surfaces stay inert and authenticated', () => {
    const workbench = read('src/components/NativeCliWorkbench.vue')
    const terminal = read('src/components/NativeCliTerminal.vue')
    expect(workbench).not.toContain('v-html')
    expect(terminal).not.toContain('v-html')
    expect(workbench).not.toContain('innerHTML')
    expect(terminal).not.toContain('innerHTML')

    const api = read('src/api/tauri.ts')
    const marker = api.indexOf('// The native document bridge owns the proof')
    expect(marker).toBeGreaterThan(-1)
    const nativeSection = api.slice(marker)
    expect(nativeSection).not.toMatch(/\binvoke\s*\(/)
    expect(nativeSection).toContain("nativeDocumentBridge().invoke('cli_stop'")
    expect(nativeSection).toContain("nativeDocumentBridge().invoke('cli_resize'")
  })

  test('native capability panels are projection-only', () => {
    const commands = read('src-tauri/src/lib.rs')
    expect(commands).not.toContain('commands::set_skill_enabled')
    expect(commands).not.toContain('commands::set_agent_enabled')
    expect(commands).not.toContain('commands::set_mcp_server_enabled')
    expect(commands).not.toContain('commands::set_plugin_enabled')
    expect(commands).not.toContain('commands::get_mcp_server_detail')
    expect(existsSync('src-tauri/src/mcp.rs')).toBe(false)
  })
})
