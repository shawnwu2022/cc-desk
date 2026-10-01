import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import NativeCliTerminal from '@/components/NativeCliTerminal.vue'
import XTermTerminal from '@/components/XTermTerminal.vue'
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { getTerminalTheme } from '@/config/terminalThemes'
const io = vi.hoisted(() => ({ webglAttempt: vi.fn(), terms: [] as any[], fits: [] as any[], webgl: [] as any[], failWebgl: false,
  start: vi.fn(), input: vi.fn(), resize: vi.fn(), stop: vi.fn(), spawn: vi.fn(), output: null as any, config: vi.fn() }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {
  options: any; element!: HTMLElement; textarea!: HTMLTextAreaElement; cols = 80; rows = 24; output = ''; selection = 'selected text'; modes = { bracketedPasteMode: false }; unicode = { activeVersion: '6' }; buffer = { active: { length: 1 } }
  dispose = vi.fn(); focus = vi.fn(); refresh = vi.fn(); data: any
  constructor(options: any) { this.options = options; io.terms.push(this) }
  open(el: HTMLElement) { this.element = el; this.textarea = document.createElement('textarea'); el.append(this.textarea) }
  loadAddon(addon: any) { addon.activate?.(this) } onData(fn: any) { this.data ??= fn; return { dispose() {} } } onResize() {} attachCustomKeyEventHandler() {} getSelection() { return this.selection } write(text: string) { this.output += text }
} }))
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { fit = vi.fn(); term: any; constructor() { io.fits.push(this) } activate(term: any) { this.term = term } } }))
vi.mock('@xterm/addon-unicode11', () => ({ Unicode11Addon: class {} }))
vi.mock('@xterm/addon-webgl', () => ({ WebglAddon: class { loss: any; dispose = vi.fn(); constructor() { io.webglAttempt(); if (io.failWebgl) throw new Error('GPU unavailable'); io.webgl.push(this) } onContextLoss(fn: any) { this.loss = fn } } }))
vi.mock('@tauri-apps/api/core', async original => ({ ...await original<object>(), Channel: class { onmessage: any } }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMinimized: async () => false }) }))
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }) }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ readText: vi.fn(), readImage: vi.fn(), writeText: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), getAppConfig: io.config, updateAppConfig: vi.fn().mockResolvedValue(undefined), cliResize: io.resize, cliStop: io.stop,
  ptySpawn: io.spawn, ptyResize: io.resize, ptyInput: io.input, ptyKill: io.stop, logMessage: vi.fn(), getSessions: vi.fn().mockResolvedValue([]), onPtyOutput: async (fn: any) => { io.output = fn; return () => {} }, onPtyExit: async () => () => {} }))
vi.mock('@/terminal/nativeLaunchEntry', () => ({ createNativeLaunchEntry: () => ({ start: io.start, recover: vi.fn() }) }))
vi.mock('@/terminal/deskNativeTerminal', () => ({ createDeskNativeTerminalBinding: () => ({ dispose() {}, sendUserText: io.input, acceptOutput: () => true, reserveUserPaste: vi.fn() }) }))
const wrappers: VueWrapper[] = []
beforeEach(async () => {
  setActivePinia(createPinia()); vi.clearAllMocks(); io.terms.length = 0; io.fits.length = 0; io.webgl.length = 0; io.failWebgl = false
  let id = 0; vi.stubGlobal('crypto', { randomUUID: () => `legacy-${++id}`, getRandomValues: window.crypto.getRandomValues })
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} }); vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => { fn(0); return 1 })
  io.config.mockResolvedValue({ theme: 'light', terminalTheme: 'cc-box-light', language: 'en' })
  io.start.mockImplementation(async input => ({ requestId: input.requestId, run: { runId: input.runId, generation: input.generation }, phase: 'running', revision: '1', failure: null }))
  io.resize.mockResolvedValue(undefined); io.stop.mockResolvedValue(undefined); io.input.mockResolvedValue(undefined); io.spawn.mockImplementation(async ({ id }: any) => ({ id }))
  useCliProfilesStore().profiles = ['claude', 'codex'].map(cli => ({ id: cli, cli, revision: '1', name: cli, launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} })) as any
  await useAppStore().loadSettingsPreferences()
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); useAppStore().$dispose(); vi.unstubAllGlobals(); vi.restoreAllMocks() })
async function open(kind: 'legacy' | 'claude' | 'codex', active = true) {
  let wrapper: VueWrapper
  if (kind === 'legacy') {
    const id = useSessionStore().createTab('/repo')
    wrapper = mount(XTermTerminal, { props: { visible: active } }); wrappers.push(wrapper); await flushPromises(); await (wrapper.vm as any).startTab(id)
  } else {
    const tab = useNativeTabsStore().create({ cli: kind, projectId: 'project', projectPath: '/repo', profileId: kind, profileRevision: '1', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active } }); wrappers.push(wrapper)
  }
  await flushPromises(); io.fits.forEach(f => f.fit.mockClear()); io.resize.mockClear(); return wrapper!
}

describe('Live terminal appearance continuity', () => {
  // 三类实际宿主都消费共享完整偏好，禁止新建第二个终端选项来源。
  it.each(['legacy', 'claude', 'codex'] as const)('Terminal_CompletePreferences_001 %s', async kind => {
    const app = useAppStore() as any
    await app.setTerminalFontFamily('Fira Code'); await app.setFontSize(17); await app.setTerminalLineHeight(1.5); await app.setTerminalCursorStyle('underline'); await app.setTerminalCursorBlink(false)
    await open(kind)
    expect(io.terms[0].options).toMatchObject({ fontFamily: app.terminalPreferences.fontFamily, fontSize: 17, lineHeight: 1.5, cursorStyle: 'underline', cursorBlink: false, theme: getTerminalTheme('cc-box-light') })
  })
  // 颜色更新不fit/resize/重建，不改变输入、滚动内容或选择。
  it.each(['legacy', 'claude', 'codex'] as const)('Terminal_ColorsOnly_002 %s', async kind => {
    await open(kind); const term = io.terms[0]; term.output = 'retained scrollback'
    await useAppStore().setTerminalTheme('dracula'); await flushPromises()
    expect(term.options.theme).toEqual(getTerminalTheme('dracula')); expect(io.terms).toHaveLength(1); expect(term.dispose).not.toHaveBeenCalled()
    expect(io.fits.every(f => !f.fit.mock.calls.length)).toBe(true); expect(io.resize).not.toHaveBeenCalled(); expect(io.input).not.toHaveBeenCalled(); expect(io.stop).not.toHaveBeenCalled()
    expect(term.output).toBe('retained scrollback'); expect(term.selection).toBe('selected text'); expect(io.start.mock.calls.length + io.spawn.mock.calls.length).toBe(1)
  })
  // 同一tick字体族/字号/行高变化合并为一次可见fit。
  it.each(['legacy', 'claude', 'codex'] as const)('Terminal_OneMetricFit_003 %s', async kind => {
    await open(kind); const app = useAppStore() as any
    const changes = [app.setTerminalFontFamily('Fira Code'), app.setFontSize(18), app.setTerminalLineHeight(1.5)]
    await Promise.all(changes); await flushPromises()
    expect(io.fits.reduce((n, f) => n + f.fit.mock.calls.length, 0)).toBe(1)
    expect(io.terms[0].options).toMatchObject({ fontSize: 18, lineHeight: 1.5 }); expect(io.terms).toHaveLength(1)
  })
  // 后台字体变化不测量，显示时只做一次延迟fit。
  it.each(['legacy', 'claude', 'codex'] as const)('Terminal_HiddenDefersFit_004 %s', async kind => {
    const view = await open(kind, false); const app = useAppStore() as any
    await app.setTerminalLineHeight(1.6); await app.setFontSize(19); await flushPromises()
    expect(io.fits.every(f => !f.fit.mock.calls.length)).toBe(true); expect(io.resize).not.toHaveBeenCalled()
    await view.setProps(kind === 'legacy' ? { visible: true } : { active: true }); await flushPromises()
    expect(io.fits.reduce((n, f) => n + f.fit.mock.calls.length, 0)).toBe(1); expect(io.terms[0].options.lineHeight).toBe(1.6)
  })
  // 光标样式/闪烁更新原options，不改变字体尺寸或测量。
  it.each(['legacy', 'claude', 'codex'] as const)('Terminal_CursorNoFit_005 %s', async kind => {
    await open(kind); const app = useAppStore() as any
    await app.setTerminalCursorStyle('block'); await app.setTerminalCursorBlink(false); await flushPromises()
    expect(io.terms[0].options).toMatchObject({ cursorStyle: 'block', cursorBlink: false }); expect(io.fits.every(f => !f.fit.mock.calls.length)).toBe(true); expect(io.resize).not.toHaveBeenCalled()
  })
  // GUI浅/暗与终端浅/暗四种组合不互相改写颜色。
  it.each([['light', 'cc-box-light'], ['light', 'cc-box-dark'], ['dark', 'cc-box-light'], ['dark', 'cc-box-dark']]
    .flatMap(([gui, terminal]) => (['legacy', 'claude', 'codex'] as const).map(kind => ({ gui, terminal, kind }))))('Terminal_GuiColorMatrix_006 $kind $gui $terminal', async ({ kind, gui, terminal }) => {
    await useAppStore().setTheme(gui); await useAppStore().setTerminalTheme(terminal); await open(kind)
    expect(useAppStore().theme).toBe(gui); expect(io.terms[0].options.theme).toEqual(getTerminalTheme(terminal))
    io.fits.forEach(f => f.fit.mockClear()); await useAppStore().setTheme(gui === 'dark' ? 'light' : 'dark'); await flushPromises()
    expect(io.terms[0].options.theme).toEqual(getTerminalTheme(terminal)); expect(io.fits.every(f => !f.fit.mock.calls.length)).toBe(true)
  })
  // 可选WebGL加载失败保留相同颜色与实例，不能触发再启动。
  it.each(['legacy', 'claude', 'codex'] as const)('Terminal_WebglFallbackColors_007 %s', async kind => {
    const quiet = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await useAppStore().setWebglRenderer(true); await useAppStore().setTerminalTheme('dracula'); io.failWebgl = true
    await open(kind); expect(io.webglAttempt).toHaveBeenCalledOnce(); expect(io.terms[0].options.theme).toEqual(getTerminalTheme('dracula')); expect(io.terms).toHaveLength(1)
    await useAppStore().setTerminalTheme('cc-box-light'); await flushPromises(); expect(io.terms[0].options.theme).toEqual(getTerminalTheme('cc-box-light'))
    expect(io.start.mock.calls.length + io.spawn.mock.calls.length).toBe(1); expect(io.stop).not.toHaveBeenCalled(); quiet.mockRestore()
  })
  // renderer偏好只对新开的终端生效，现有实例不重建。
  it.each(['legacy', 'codex'] as const)('Terminal_RendererNextOpen_008 %s', async kind => {
    const view = await open(kind); await useAppStore().setWebglRenderer(true); await flushPromises()
    expect(io.webgl).toHaveLength(0); expect(io.terms).toHaveLength(1); expect(io.fits.every(f => !f.fit.mock.calls.length)).toBe(true)
    if (kind === 'legacy') { const id = useSessionStore().createTab('/second'); await (view.vm as any).startTab(id); await flushPromises() }
    else await open(kind)
    expect(io.webgl).toHaveLength(1)
  })
  // 同一动画帧内多个字体意图合并，隐藏发生在帧之前时不测量。
  it.each(['legacy', 'claude', 'codex'] as const)('Terminal_FrameCoalescesMetrics_009 %s', async kind => {
    const view = await open(kind); const frames: FrameRequestCallback[] = []
    vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => { frames.push(fn); return frames.length })
    await useAppStore().setFontSize(16); await flushPromises(); await useAppStore().setTerminalLineHeight(1.6); await flushPromises()
    expect(frames).toHaveLength(1); expect(io.fits.every(f => !f.fit.mock.calls.length)).toBe(true)
    await view.setProps(kind === 'legacy' ? { visible: false } : { active: false })
    frames.shift()!(0); expect(io.fits.every(f => !f.fit.mock.calls.length)).toBe(true); expect(io.resize).not.toHaveBeenCalled()
    vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => { fn(0); return 1 })
    await view.setProps(kind === 'legacy' ? { visible: true } : { active: true }); await flushPromises()
    expect(io.fits.reduce((n, f) => n + f.fit.mock.calls.length, 0)).toBe(1)
  })
  // WebGL丢失后仍沿用最新颜色、同一终端、滚动内容和选择。
  it.each(['legacy', 'claude', 'codex'] as const)('Terminal_ContextLossColors_010 %s', async kind => {
    await useAppStore().setWebglRenderer(true); await open(kind); const term = io.terms[0]; term.output = 'retained'
    await useAppStore().setTerminalTheme('nord'); await flushPromises(); const addon = io.webgl[0]; addon.loss(); await flushPromises()
    expect(term.options.theme).toEqual(getTerminalTheme('nord')); expect(term.output).toBe('retained'); expect(term.selection).toBe('selected text')
    expect(term.dispose).not.toHaveBeenCalled(); expect(io.terms).toHaveLength(1); expect(io.input).not.toHaveBeenCalled(); expect(io.stop).not.toHaveBeenCalled()
  })
})
