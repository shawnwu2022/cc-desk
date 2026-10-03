import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { readFileSync } from 'node:fs'
import SettingsView from '@/components/settings/SettingsView.vue'
import { useAppStore } from '@/stores/app'
import { useSidebarStore } from '@/stores/sidebar'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { TERMINAL_THEMES } from '@/config/terminalThemes'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ isMaximized: async () => false, onResized: async () => () => {} }) }))
let wrapper: VueWrapper | undefined
let stored: Record<string, unknown>
let writes: Record<string, unknown>[]
beforeEach(() => {
  clearMocks(); setActivePinia(createPinia()); writes = []
  stored = { theme: 'light', guiThemeMode: 'light', terminalTheme: 'cc-box-light', fontSize: 12, language: 'en', claudeEnvVars: { TEST: 'keep' } }
  mockIPC((command, args) => {
    if (command === 'get_app_config') return { ...stored }
    if (command === 'update_app_config') { const patch = (args as any).updates; writes.push(patch); stored = { ...stored, ...patch } }
  })
})
afterEach(() => { wrapper?.unmount(); wrapper = undefined; useAppStore().$dispose(); clearMocks(); vi.restoreAllMocks() })
async function render(locale = 'en') {
  await useAppStore().loadSettingsPreferences(); useSidebarStore().activeSettingsSection = 'terminal'
  wrapper = mount(SettingsView, { global: { plugins: [createI18n({ legacy: false, locale, messages: { en, zh } })] } })
  await flushPromises(); return wrapper
}
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (reason: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no }); return { promise, resolve, reject } }

describe('Terminal settings and serialized persistence', () => {
  // 正常Terminal分类暴露全部偏好与静态预览，不借用真实终端或桥接。
  it('TerminalSettings_CompleteSection_001', async () => {
    const view = await render()
    for (const field of ['font-family', 'font-size', 'line-height', 'cursor-style', 'cursor-blink', 'renderer']) expect(view.find(`[data-terminal-${field}]`).exists()).toBe(true)
    expect(view.findAll('[data-terminal-theme]')).toHaveLength(TERMINAL_THEMES.length)
    expect(view.find('[data-terminal-preview]').exists()).toBe(true)
    expect(view.findComponent({ name: 'XTermTerminal' }).exists()).toBe(false); expect(view.findComponent({ name: 'NativeCliTerminal' }).exists()).toBe(false)
    expect(writes).toEqual([])
  })
  // 主题选择即时预览并仅提交终端字段，不改变GUI或会话选择。
  it('TerminalSettings_ThemePreview_002', async () => {
    const view = await render(); const app = useAppStore(); useUnifiedSessionsStore().activeSessionId = 'held'
    await view.get('[data-terminal-theme="dracula"]').trigger('click'); await flushPromises()
    expect(app.terminalTheme).toBe('dracula'); expect(app.theme).toBe('light'); expect(useUnifiedSessionsStore().activeSessionId).toBe('held')
    expect(writes).toEqual([{ terminalTheme: 'dracula' }]); expect(view.get('[data-terminal-theme="dracula"]').attributes('aria-pressed')).toBe('true')
    expect(view.get('[data-terminal-preview]').attributes('style')).toContain('#282a36')
  })
  // 数字输入保留中间草稿，完整失焦只写一次，失效值撤回。
  it('TerminalSettings_MetricDraft_003', async () => {
    const view = await render(); const input = view.get('[data-terminal-font-size]')
    await input.setValue(''); await input.setValue('1'); expect(writes).toEqual([]); expect(useAppStore().fontSize).toBe(12)
    await input.setValue('18'); await input.trigger('blur'); await flushPromises()
    expect(useAppStore().fontSize).toBe(18); expect(writes).toEqual([{ fontSize: 18 }])
    await input.trigger('blur'); await flushPromises(); expect(writes).toHaveLength(1)
    await input.setValue(''); await input.trigger('keydown', { key: 'Enter' }); await flushPromises(); expect((input.element as HTMLInputElement).value).toBe('18')
  })
  // 七种偏好共用持久化入口，且 renderer 生效范围明确告知用户。
  it('TerminalSettings_AllFieldsPersist_004', async () => {
    const view = await render()
    await view.get('[data-terminal-font-family]').setValue('Fira Code')
    await view.get('[data-terminal-line-height]').setValue('1.5'); await view.get('[data-terminal-line-height]').trigger('blur')
    await view.get('[data-terminal-cursor-style]').setValue('underline')
    await view.get('[data-terminal-cursor-blink]').setValue('false')
    await view.get('[data-terminal-renderer]').setValue('webgl'); await flushPromises()
    expect(writes).toEqual([{ terminalFontFamily: 'Fira Code' }, { terminalLineHeight: 1.5 }, { terminalCursorStyle: 'underline' }, { terminalCursorBlink: false }, { webglRenderer: true }])
    expect(view.text()).toContain('newly opened terminals')
  })
  // 已确认字体值用于已知保存失败回退，并更新输入预览和安全错误。
  it('TerminalSettings_Rollback_005', async () => {
    const view = await render(); await useAppStore().setFontSize(16)
    mockIPC(command => { if (command === 'update_app_config') throw new Error('TOKEN=/private'); return stored })
    await view.get('[data-terminal-font-size]').setValue('20'); await view.get('[data-terminal-font-size]').trigger('blur'); await flushPromises()
    expect(useAppStore().fontSize).toBe(16); expect((view.get('[data-terminal-font-size]').element as HTMLInputElement).value).toBe('16')
    expect(view.html()).not.toContain('TOKEN'); expect(useAppStore().settingsSaveError).toBe('settingsSaveFailed')
  })
  // 终端写入必须在GUI写入之后使用同一序列化队列。
  it('TerminalSettings_SharedWriter_006', async () => {
    const app = useAppStore(); await app.loadSettingsPreferences(); const first = deferred<void>()
    mockIPC((command, args) => { if (command === 'update_app_config') { writes.push((args as any).updates); if (writes.length === 1) return first.promise } return stored })
    const gui = app.setTheme('dark'); const terminal = app.setTerminalTheme('dracula'); await flushPromises()
    expect(writes).toEqual([{ guiThemeMode: 'dark', theme: 'dark' }]); expect(app.terminalTheme).toBe('dracula')
    first.resolve(); await gui; await terminal; expect(writes[1]).toEqual({ terminalTheme: 'dracula' })
  })
  // 较晚启动读取和迁移不得覆盖用户已经确认的终端主题。
  it('TerminalSettings_StartupIntent_007', async () => {
    const app = useAppStore(); const read = deferred<any>(); mockIPC(command => command === 'get_app_config' ? read.promise : undefined)
    const loading = app.loadAppConfig(); await flushPromises(); const saving = app.setTerminalTheme('dracula')
    mockIPC((command, args) => { if (command === 'update_app_config') writes.push((args as any).updates); return stored })
    read.resolve({ theme: 'dark', language: 'en', claudeEnvVars: { TEST: 'keep' } }); await loading; await saving
    expect(app.terminalTheme).toBe('dracula'); expect(writes.filter(row => 'terminalTheme' in row)).toEqual([{ terminalTheme: 'dracula' }])
  })
  // 未知终端写与读取失败不能越过原来的恢复屏障再次提交。
  it('TerminalSettings_UnknownBarrier_008', async () => {
    const app = useAppStore(); await app.loadSettingsPreferences(); let readable = false
    mockIPC((command, args) => {
      if (command === 'get_app_config') { if (!readable) throw new Error('read failed'); return stored }
      if (command === 'update_app_config') { const patch = (args as any).updates; writes.push(patch); stored = { ...stored, ...patch }; if (writes.length === 1) throw { code: 'COMMIT_STATE_UNKNOWN' } }
    })
    expect(await app.setTerminalTheme('dracula')).toBe(false); expect(app.settingsSaveError).toBe('settingsSaveReloadFailed')
    expect(await app.setFontSize(18)).toBe(false); expect(writes).toHaveLength(1)
    readable = true; expect(await app.setFontSize(18)).toBe(true); expect(writes).toHaveLength(2); expect(app.terminalTheme).toBe('dracula')
  })
  // 终端设置 hydration 也继承原始共享读取的提交 fence。
  it('TerminalSettings_ReadOriginFence_009', async () => {
    const app = useAppStore(); await app.loadSettingsPreferences(); const read = deferred<any>(); let reject = false
    mockIPC(command => { if (command === 'get_app_config') return read.promise; if (command === 'update_app_config' && reject) throw new Error('failed') })
    const visibility = app.loadProjectVisibility(true); await flushPromises(); await app.setTerminalTheme('dracula'); await app.setFontSize(18)
    const loading = app.loadAppConfig(); await flushPromises(); read.resolve(stored); await visibility; await loading
    expect(app.terminalTheme).toBe('dracula'); expect(app.fontSize).toBe(18)
    reject = true; await app.setFontSize(20); expect(app.fontSize).toBe(18)
  })
  // 偏好边界统一归一化，写入值和共享计算值保持一致。
  it('TerminalSettings_NormalizePersist_010', async () => {
    const app = useAppStore() as any; await app.loadSettingsPreferences()
    await app.setFontSize(999); await app.setTerminalLineHeight(9); await app.setTerminalFontFamily('unrecognized'); await app.setTerminalCursorStyle('invalid')
    expect(writes).toEqual([{ fontSize: 24 }, { terminalLineHeight: 2 }, { terminalFontFamily: 'system' }, { terminalCursorStyle: 'bar' }])
    expect(app.terminalPreferences).toMatchObject({ fontSize: 24, lineHeight: 2, cursorStyle: 'bar', cursorBlink: true, renderer: 'dom' })
  })
  // 中文终端设置和预览说明均已本地化。
  it('TerminalSettings_Chinese_011', async () => {
    const view = await render('zh'); expect(view.text()).toContain('行高'); expect(view.text()).toContain('光标闪烁'); expect(view.text()).toContain('预览')
    expect(view.text()).not.toContain('settingsTerminalPending')
  })
  // Rust 仅读DTO字段为可选，未增加命令；真实Rust门禁另行执行。
  it('TerminalSettings_ReadDtoFields_012', () => {
    const source = readFileSync('src-tauri/src/store.rs', 'utf8')
    for (const key of ['terminalFontFamily', 'terminalLineHeight', 'terminalCursorStyle', 'terminalCursorBlink', 'webglRenderer']) expect(source).toContain(`rename = "${key}"`)
  })
  // 新增字段读回实际false值，和已有字号/theme/renderer兼容键组成统一快照。
  it('TerminalSettings_ReadAllFields_013', async () => {
    stored = { ...stored, terminalTheme: 'nord', terminalFontFamily: 'Fira Code', fontSize: 16, terminalLineHeight: 1.5, terminalCursorStyle: 'block', terminalCursorBlink: false, webglRenderer: true }
    const app = useAppStore() as any; await app.loadAppConfig()
    expect(app.terminalPreferences).toMatchObject({ themeId: 'nord', fontSize: 16, lineHeight: 1.5, cursorStyle: 'block', cursorBlink: false, renderer: 'webgl' })
    expect(app.terminalPreferences.fontFamily).toMatch(/^"Fira Code"/)
  })
  // 缺失终端字段只推断一次；未知GUI写读回不能再次用新GUI颜色改写终端。
  it('TerminalSettings_MigrateOnce_014', async () => {
    delete stored.terminalTheme
    const app = useAppStore(); await app.loadSettingsPreferences(); expect(app.terminalTheme).toBe('cc-box-light')
    mockIPC((command, args) => {
      if (command === 'get_app_config') return { ...stored }
      if (command === 'update_app_config') { stored = { ...stored, ...(args as any).updates }; throw { code: 'COMMIT_STATE_UNKNOWN' } }
    })
    expect(await app.setTheme('dark')).toBe(false)
    expect(app.terminalTheme).toBe('cc-box-light')
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {})
    await app.loadAppConfig().catch(() => undefined); expect(stored.terminalTheme).toBe('cc-box-light'); quiet.mockRestore()
  })
})
