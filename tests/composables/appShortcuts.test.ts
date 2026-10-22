import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { flushPromises } from '@vue/test-utils'
import { useAppShortcuts } from '@/composables/useAppShortcuts'
import { useAppStore } from '@/stores/app'
import { useShellStore } from '@/stores/shell'
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}), currentMonitor: async () => null }))
let cleanup: Array<() => void> = []
beforeEach(() => { setActivePinia(createPinia()); mockIPC(command => command === 'get_app_config' ? { language: 'en', terminalTheme: 'cc-box-light' } : undefined) })
afterEach(() => { cleanup.splice(0).forEach(fn => fn()); document.body.innerHTML = ''; clearMocks(); useAppStore().$dispose(); vi.restoreAllMocks() })
function shortcuts(onToggleSidebar?: () => boolean) {
  const actions: string[] = []
  cleanup = (useAppShortcuts as any)({ onAction: (action: string) => actions.push(action), onToggleSidebar }).setupShortcutListeners()
  return actions
}
function key(key: string, options: KeyboardEventInit = {}, target: EventTarget = window) {
  const event = new KeyboardEvent('keydown', { key, code: key === ',' ? 'Comma' : key === 'F2' ? 'F2' : `Key${key.toUpperCase()}`, bubbles: true, cancelable: true, ...options })
  target.dispatchEvent(event); return event
}
describe('Normal app shortcut routing', () => {
  // 捕获层必须把终端/编辑焦点的 Ctrl+B 原样留给所属控件，不能只看事件 target。
  it.each(['input', 'select', 'textarea.xterm-helper-textarea', 'div[contenteditable=""]', 'div[contenteditable="plaintext-only"]', 'div.xterm', 'div.native-cli-terminal'])('Shortcuts_SidebarRetainsOwnedInput_009 %s', async selector => {
    const shell = useShellStore()
    shortcuts(() => { shell.toggleSidebar(); return true }); await flushPromises()
    const holder = document.createElement('div')
    holder.innerHTML = selector.startsWith('input') ? '<input>' : selector.startsWith('select') ? '<select><option>One</option></select>'
      : selector.startsWith('textarea') ? '<textarea class="xterm-helper-textarea"></textarea>'
      : selector.includes('contenteditable') ? `<div ${selector.slice(4, -1)} tabindex="0"><span>Editable</span></div>`
      : `<div class="${selector.slice(4)}" tabindex="0"><span>Terminal</span></div>`
    document.body.append(holder)
    const focused = holder.firstElementChild as HTMLElement; focused.focus()
    const received: KeyboardEvent[] = []; focused.addEventListener('keydown', event => received.push(event))
    const event = key('b', { ctrlKey: true }, focused)
    expect(event.defaultPrevented).toBe(false); expect(received).toEqual([event]); expect(shell.sidebarVisible).toBe(true)
    expect(key('b', { ctrlKey: true }).defaultPrevented).toBe(false)
    expect(shell.sidebarVisible).toBe(true)
  })

  it('Shortcuts_SidebarHonorsRepeatAndExistingBindings_010', async () => {
    const shell = useShellStore(), app = useAppStore()
    const actions = shortcuts(() => { shell.toggleSidebar(); return true }); await flushPromises()
    expect(key('b', { ctrlKey: true }).defaultPrevented).toBe(true); expect(shell.sidebarVisible).toBe(false)
    for (const options of [{ repeat: true }, { isComposing: true }, { keyCode: 229 }, { altKey: true }, { shiftKey: true }, { metaKey: true }]) {
      expect(key('b', { ctrlKey: true, ...options }).defaultPrevented).toBe(false)
      expect(shell.sidebarVisible).toBe(false)
    }
    const consumed = new KeyboardEvent('keydown', { key: 'b', code: 'KeyB', ctrlKey: true, cancelable: true }); consumed.preventDefault(); window.dispatchEvent(consumed)
    expect(shell.sidebarVisible).toBe(false)
    const modal = document.createElement('div'); modal.setAttribute('role', 'dialog'); modal.setAttribute('aria-modal', 'true'); document.body.append(modal)
    expect(key('b', { ctrlKey: true }).defaultPrevented).toBe(false); expect(shell.sidebarVisible).toBe(false); modal.remove()
    await app.setShortcutBindings({ ...app.shortcutBindings, projects: 'Mod+KeyB' })
    key('b', { ctrlKey: true }); expect(actions).toEqual(['projects']); expect(shell.sidebarVisible).toBe(false)
    cleanup.splice(0).forEach(fn => fn())
    expect(key('b', { ctrlKey: true }).defaultPrevented).toBe(false)
  })

  // 两个平台修饰键都进入统一操作端口，不借道旧PTY、标签或窗口重启。
  it.each(['ctrlKey', 'metaKey'])('Shortcuts_DefaultActions_%s_001', async modifier => {
    const actions = shortcuts(); await flushPromises()
    for (const value of ['n', 'w', 'p', ',']) expect(key(value, { [modifier]: true }).defaultPrevented).toBe(true)
    key('F2')
    expect(actions).toEqual(['new-session', 'close-session', 'projects', 'settings'])
  })
  // 重复、输入法、额外修饰键和已有处理事件不消费，普通编辑字段保留自己的编辑语义。
  it('Shortcuts_InputAndRepeatOwnership_002', async () => {
    const actions = shortcuts(); await flushPromises()
    key('n', { ctrlKey: true, repeat: true }); key('n', { ctrlKey: true, isComposing: true }); key('n', { ctrlKey: true, shiftKey: true })
    const input = document.createElement('input'); document.body.append(input); key('w', { ctrlKey: true }, input)
    expect(actions).toEqual([])
    const terminal = document.createElement('textarea'); terminal.className = 'xterm-helper-textarea'; document.body.append(terminal)
    key('n', { ctrlKey: true }, terminal); expect(actions).toEqual(['new-session'])
  })
  // 模态捕获和确认拥有键盘，卸载后的监听不再触发任何动作。
  it('Shortcuts_ModalAndListenerCleanup_003', async () => {
    const actions = shortcuts(); await flushPromises()
    const dialog = document.createElement('section'); dialog.setAttribute('role', 'dialog'); dialog.setAttribute('aria-modal', 'true'); document.body.append(dialog)
    key('n', { ctrlKey: true }); expect(actions).toEqual([])
    dialog.remove(); cleanup.splice(0).forEach(fn => fn()); key('n', { ctrlKey: true }); expect(actions).toEqual([])
  })
  // 真正保存的新绑定即刻用于路由，旧绑定不再吞掉终端输入。
  it('Shortcuts_CustomBindingAndRollback_004', async () => {
    const app = useAppStore() as any; const actions = shortcuts(); await flushPromises()
    expect(typeof app.setShortcutBindings).toBe('function')
    const previous = { ...app.shortcutBindings }
    await app.setShortcutBindings({ ...previous, 'new-session': 'Mod+KeyK' })
    key('n', { ctrlKey: true }); key('k', { ctrlKey: true }); expect(actions).toEqual(['new-session'])
    mockIPC(command => { if (command === 'update_app_config') throw new Error('TOKEN=private'); return {} })
    expect(await app.setShortcutBindings(previous)).toBe(false)
    actions.length = 0; key('k', { metaKey: true }); expect(actions).toEqual(['new-session'])
  })
  // 聚焦会话行时，重命名由该行处理；窗口入口只处理真正选中的活动会话。
  it('Shortcuts_FocusedRenameOwnership_005', async () => {
    const actions = shortcuts(); await flushPromises()
    const row = document.createElement('div'); row.dataset.sessionRow = 'focused'; document.body.append(row)
    key('F2', {}, row); expect(actions).toEqual([])
  })

  // 全局捕获不得先吞掉终端 F2；同一终端里的既有全局命令仍然生效。
  it('Shortcuts_TerminalRetainsRenameKey_007', async () => {
    const actions = shortcuts(); await flushPromises()
    const terminal = document.createElement('textarea'); terminal.className = 'xterm-helper-textarea'
    document.body.append(terminal); terminal.focus()
    const received: KeyboardEvent[] = []
    terminal.addEventListener('keydown', event => received.push(event))
    const event = key('F2', {}, terminal)
    expect(event.defaultPrevented).toBe(false)
    expect(received).toEqual([event]); expect(actions).toEqual([])
    key('w', { ctrlKey: true }, terminal); expect(actions).toEqual(['close-session'])
    terminal.blur(); expect(key('F2').defaultPrevented).toBe(false)
    expect(actions).toEqual(['close-session'])
  })

  it.each(['', 'true', 'plaintext-only'])('Shortcuts_EditableInheritanceOwnsKeys_008 %s', async value => {
    const actions = shortcuts(); await flushPromises()
    const editable = document.createElement('div'); editable.setAttribute('contenteditable', value)
    const child = document.createElement('span'); editable.append(child); document.body.append(editable)
    expect(key('n', { ctrlKey: true }, child).defaultPrevented).toBe(false)
    expect(actions).toEqual([])
  })

  // 未知整组写可能已提交，失败的读回阻止后续写；成功只读协调后才允许新的明确更改。
  it('Shortcuts_UnknownBindingGroupNoReplay_006', async () => {
    const app = useAppStore(); const original = { ...app.shortcutBindings }
    let stored = { language: 'en', terminalTheme: 'cc-box-light', shortcutBindings: original }; let reads = 0; let writes = 0; let readable = false
    mockIPC((command, payload) => {
      if (command === 'get_app_config') { if (++reads > 1 && !readable) throw new Error('private read failure'); return stored }
      if (command === 'update_app_config') { ++writes; stored = { ...stored, ...(payload as any).updates }; if (writes === 1) throw { code: 'COMMIT_STATE_UNKNOWN' } }
      return undefined
    })
    await app.loadSettingsPreferences()
    expect(await app.setShortcutBindings({ ...original, 'new-session': 'Mod+KeyK' })).toBe(false)
    expect(app.shortcutBindingsError).toBe('settingsSaveReloadFailed')
    expect(await app.setShortcutBindings({ ...original, projects: 'Mod+KeyJ' })).toBe(false); expect(writes).toBe(1)
    readable = true; await app.loadSettingsPreferences(true)
    expect(app.shortcutBindings['new-session']).toBe('Mod+KeyK')
    expect(await app.setShortcutBindings({ ...app.shortcutBindings, projects: 'Mod+KeyJ' })).toBe(true)
    expect(writes).toBe(2)
  })

})
