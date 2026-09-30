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
    expect(app).not.toContain('await appStore.runChecks')
    expect(app).not.toContain('decideStartupView')
    expect(app).not.toContain('startProjectSession')
    const workspace = read('src/components/workspace/WorkspaceView.vue')
    expect(workspace).toContain("['claude', 'codex']")
    expect(workspace).toContain("cliAvailability[cli] === 'unavailable'")
    expect(workspace).not.toContain('check-failed-overlay')
    expect(workspace).not.toContain('ptySpawn')
    expect(workspace).not.toContain('ptyInput')
    expect(workspace).not.toContain('ptyKill')
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
    expect(nativeSection).not.toMatch(/^\s*(?:return|await)\s+invoke(?:<[^>]+>)?\s*\(/m)
    expect(nativeSection).toContain("nativeDocumentBridge().invoke('cli_stop'")
    expect(nativeSection).toContain("nativeDocumentBridge().invoke('cli_resize'")
  })

  test('native security surfaces redact values and do not log user payloads', () => {
    const workbench = read('src/components/NativeCliWorkbench.vue')
    const terminal = read('src/components/NativeCliTerminal.vue')
    for (const source of [workbench, terminal]) {
      expect(source).not.toContain('console.')
      expect(source).not.toContain('logMessage(')
      expect(source).not.toContain('v-html')
      expect(source).not.toContain('innerHTML')
    }

    const snapshot = read('src-tauri/src/cli/snapshot.rs')
    const launch = read('src-tauri/src/cli/launch_service.rs')
    const document = read('src-tauri/src/cli/document.rs')
    expect(snapshot).toContain('LaunchSnapshot(<redacted>)')
    expect(launch).toContain('RunAccess(<redacted>)')
    expect(document).toContain('DocumentAuthority(<redacted>)')
    expect(document).toContain('DocumentBinding(<redacted>)')
    expect(document).not.toContain('serde_json::from_slice(bytes).map_err(|error|')
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

  test('dual-CLI sessions share one unified product shell', () => {
    const app = read('src/App.vue')
    const nav = read('src/components/shell/PrimaryNav.vue')
    const workspace = read('src/components/workspace/WorkspaceView.vue')
    expect(app).toContain('<AppShell v-else')
    expect(app).toContain('<WorkspaceView v-show=')
    expect(app).not.toContain('<NativeCliWorkbench')
    expect(app).not.toContain('<TerminalView')
    expect(app).not.toContain('openNativeWorkbench')
    expect(nav).toContain("section: 'workspace'")
    expect(nav).toContain("section: 'projects'")
    expect(nav).toContain("section: 'settings'")
    expect(nav).not.toContain('Native CLI')
    expect(workspace).toContain('data-workspace-terminal-host')
    expect(read('src/stores/shell.ts')).toContain("return dev && flag === '1'")
    expect(app).toContain('isCompatibilityEnabled(import.meta.env.DEV,')

    for (const path of [
      'README.md',
      'README_CN.md',
      'PRODUCT.md',
      'docs/roadmap.md',
      'docs/vision.md',
      'docs/native-cli-v3.md',
    ]) {
      const document = read(path)
      expect(document).toContain('Claude Code')
      expect(document).toContain('Codex CLI')
      expect(document).not.toContain('src-tauri/src/providers.rs')
      expect(document).not.toContain('src/api/provider.ts')
    }
  })

  test('release documentation matches the enforced candidate-only policy', () => {
    const release = read('.github/workflows/release.yml')
    const policy = read('scripts/release-policy.mjs')
    const docs = read('docs/release-process.md')
    expect(policy).toContain('return false')
    expect(release).toContain('Upload candidate artifacts')
    expect(release).not.toContain('softprops/action-gh-release')
    expect(release).not.toContain('contents: write')
    expect(docs).toContain('signed candidates only')
    expect(docs).toContain('publishing stays disabled')
  })
  test('package and installer metadata describe the dual-CLI product', () => {
    const packageJson = JSON.parse(read('package.json')) as {
      description?: string
      keywords?: string[]
    }
    const tauri = read('src-tauri/tauri.conf.json')
    expect(packageJson.description).toContain('Claude Code and Codex CLI')
    expect(packageJson.keywords).toContain('codex-cli')
    expect(tauri).toContain('Claude Code and Codex CLI')
  })

  test('native workbench user-facing controls stay localized', () => {
    const workbench = read('src/components/NativeCliWorkbench.vue')
    const english = read('src/i18n/locales/en.ts')
    const chinese = read('src/i18n/locales/zh.ts')
    expect(workbench).toContain("useI18n")
    expect(workbench).toContain("t('nativeSelectProject')")
    expect(workbench).toContain("t('nativeResumePicker')")
    expect(workbench).toContain("nativeStatusLabel(tab.status)")
    expect(workbench).toContain("nativeResourceLabel(kind)")

    for (const key of [
      'nativeCreateCodexProfile',
      'nativeCreateClaudeProfile',
      'nativeSelectProject',
      'nativeNewSession',
      'nativeResumePicker',
      'nativeWorkspaceLoading',
      'nativeRecover',
      'nativeStop',
      'nativeRestart',
      'nativeResources',
      'nativeStatusRunning',
      'nativeResource_history',
    ]) {
      expect(english).toContain(`${key}:`)
      expect(chinese).toContain(`${key}:`)
    }

    const about = read('src/components/settings/sections/AboutSection.vue')
    expect(about).toContain('developers.openai.com/learn/codex')
    expect(english).toContain("codexDocs:")
    expect(chinese).toContain("codexDocs:")
  })

})
