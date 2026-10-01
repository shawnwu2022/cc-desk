import { existsSync, readFileSync } from 'node:fs'
import { describe, expect, test } from 'vitest'

const read = (path: string) => readFileSync(path, 'utf8')
const nativeSurfaces = [
  'src/components/NativeCliTerminal.vue',
  'src/components/workspace/ProjectResourcesDrawer.vue',
  ...['InstructionsView', 'SettingsView', 'McpList', 'SkillList', 'AgentList', 'PluginList', 'ResourceMetadata']
    .map(name => `src/components/resources/${name}.vue`),
]

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
    for (const path of nativeSurfaces) {
      expect(read(path), path).not.toContain('v-html')
      expect(read(path), path).not.toContain('innerHTML')
    }

    const api = read('src/api/tauri.ts')
    const marker = api.indexOf('// The native document bridge owns the proof')
    expect(marker).toBeGreaterThan(-1)
    const nativeSection = api.slice(marker)
    expect(nativeSection).not.toMatch(/^\s*(?:return|await)\s+invoke(?:<[^>]+>)?\s*\(/m)
    expect(nativeSection).toContain("nativeDocumentBridge().invoke('cli_stop'")
    expect(nativeSection).toContain("nativeDocumentBridge().invoke('cli_resize'")
  })

  test('native security surfaces redact values and do not log user payloads', () => {
    for (const path of nativeSurfaces) {
      const source = read(path)
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
    expect(app).toContain('<AppShell :title=')
    expect(app).toContain('<WorkspaceView v-show=')
    expect(app).not.toContain('<NativeCliWorkbench')
    expect(app).not.toContain('<TerminalView')
    expect(app).not.toContain('openNativeWorkbench')
    expect(nav).toContain("section: 'workspace'")
    expect(nav).toContain("section: 'projects'")
    expect(nav).toContain("section: 'settings'")
    expect(nav).not.toContain('Native CLI')
    expect(workspace).toContain('data-workspace-terminal-host')
    expect(app).not.toContain('LegacyCompatibilityApp')
    expect(app).not.toContain('VITE_CC_DESK_COMPATIBILITY')
    expect(read('src/stores/shell.ts')).not.toContain('isCompatibilityEnabled')
    expect(nav.match(/section: '[^']+'/g)).toEqual(["section: 'workspace'", "section: 'projects'", "section: 'settings'"])
    expect(read('src/components/TerminalView.vue')).not.toMatch(/IconBar|SidebarPanel|TerminalHeader|startProjectSession|pendingResume|terminal:newSession/)
    expect(read('src/components/sidebar/SidebarPanel.vue')).not.toContain('Type-only compatibility')
    expect(read('src/composables/useAppShortcuts.ts')).not.toContain('useLegacyAppShortcuts')

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

  // 所有构建只保留统一入口；不能恢复旧产品页、第二套标签或资源导航。
  test('Surface_RetiresDuplicatePages_001', () => {
    for (const path of [
      'src/components/NativeCliWorkbench.vue', 'src/components/LegacyCompatibilityApp.vue',
      'src/components/WelcomeView.vue', 'src/components/ProjectSelectView.vue',
      'src/components/IconBar.vue', 'src/components/TerminalHeader.vue',
      'src/components/settings/SettingsOverlay.vue', 'src/components/settings/sections/StartupSection.vue',
      'src/stores/nativeWorkbench.ts', 'src/composables/useStartupDecision.ts',
      ...['agents/AgentsPanel', 'skills/SkillsPanel', 'mcp/McpPanel', 'plugins/PluginsPanel'].map(path => `src/components/${path}.vue`),
    ]) expect(existsSync(path), path).toBe(false)
    for (const locale of ['en', 'zh']) {
      expect(read(`src/i18n/locales/${locale}.ts`)).not.toContain('openNativeCliWorkspace:')
      expect(read(`src/i18n/locales/${locale}.ts`)).not.toContain('openLegacyClaudeWorkspace:')
    }
  })

  test('unified session and resource controls stay localized and structured', () => {
    const english = read('src/i18n/locales/en.ts')
    const chinese = read('src/i18n/locales/zh.ts')
    expect(read('src/components/sessions/NewSessionDialog.vue')).toContain("t('newSessionTitle')")
    expect(read('src/components/sessions/ResumeSessionDialog.vue')).toContain("t('resumeSearch')")
    for (const path of nativeSurfaces.slice(1)) expect(read(path), path).not.toContain('JSON.stringify')
    for (const key of ['newSessionTitle', 'resumeSearch', 'resourceCategory_instructions', 'resourceCategory_config',
      'resourceCategory_mcp', 'resourceCategory_skills', 'resourceCategory_agents', 'resourceCategory_plugins']) {
      expect(english).toContain(`${key}:`)
      expect(chinese).toContain(`${key}:`)
    }
    const about = read('src/components/settings/sections/AboutSection.vue')
    expect(about).toContain('developers.openai.com/learn/codex')
    expect(english).toContain('codexDocs:')
    expect(chinese).toContain('codexDocs:')
  })

})
