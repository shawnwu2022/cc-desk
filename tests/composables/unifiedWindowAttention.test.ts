import { randomUUID } from 'node:crypto'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { defineComponent, ref } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { useStatusMonitor } from '@/composables/useStatusMonitor'
import { useUnifiedWindowAttention } from '@/composables/useUnifiedWindowAttention'
import { useAttentionStore } from '@/stores/attention'
import { useSessionStore } from '@/stores/session'
import { useNativeTabsStore, captureNativeAttempt } from '@/stores/nativeTabs'
import type { HookEventPayload } from '@/types/hook'

const io = vi.hoisted(() => ({ focused: false, attention: vi.fn(), readFocus: vi.fn(), subscribe: vi.fn(), unlisten: vi.fn(), focus: null as null | ((event: { payload: boolean }) => void) }))
vi.mock('@tauri-apps/api/window', () => ({ UserAttentionType: { Critical: 2 }, getCurrentWindow: () => ({
  isFocused: io.readFocus,
  onFocusChanged: async (handler: typeof io.focus) => { io.focus = handler; return io.subscribe() },
  requestUserAttention: io.attention,
}) }))
const wrappers: VueWrapper[] = []
beforeEach(() => { vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID }); setActivePinia(createPinia()); vi.clearAllMocks(); io.focused = false; io.focus = null; io.attention.mockResolvedValue(undefined); io.readFocus.mockImplementation(async () => io.focused); io.subscribe.mockResolvedValue(io.unlisten) })
afterEach(() => { wrappers.splice(0).forEach(wrapper => wrapper.unmount()); vi.unstubAllGlobals() })
function render(active: string | null = null, legacyConsumer = false) {
  const activeSessionId = ref(active), visible = ref(true)
  const wrapper = mount(defineComponent({ setup() { const { isFocused } = useUnifiedWindowAttention({ activeSessionId, visible }); if (legacyConsumer) useStatusMonitor({ isFocused, isTerminalVisible: visible, requestWindowAttention: false }); return () => null } }))
  wrappers.push(wrapper)
  return { wrapper, activeSessionId, visible }
}
function legacy() {
  const sessions = useSessionStore(), id = sessions.createTab('/repo', { sessionId: 'session' })
  sessions.setTabPty(id, `pty-${id}`); sessions.tabs.get(id)!.status = 'running'
  return sessions.tabs.get(id)!
}
function event(ptyId: string, detail: HookEventPayload['detail'], timestamp = 1, sessionId = 'session') {
  useAttentionStore().ingestEvent({ ptyId, sessionId, eventName: 'Test', state: 'idle', timestamp, detail })
}
const completed: HookEventPayload['detail'] = { type: 'notification', data: { notificationType: 'idle_prompt' } }

describe('Unified window attention ownership', () => {
  it('Attention_OwnedCompletionOnlyOnce_001', async () => {
    const tab = legacy(); render(); await flushPromises()
    event(tab.ptyId!, completed); await flushPromises()
    expect(io.attention).toHaveBeenCalledExactlyOnceWith(2)
    tab.name = 'New display name'; await flushPromises()
    io.focus!({ payload: true }); await flushPromises()
    io.focus!({ payload: false }); await flushPromises()
    expect(io.attention.mock.calls.filter(([kind]) => kind === 2)).toHaveLength(1)
    event(tab.ptyId!, completed, 2); await flushPromises()
    expect(io.attention.mock.calls.filter(([kind]) => kind === 2)).toHaveLength(2)
  })
  it('Attention_RejectsClosedWrongSessionAndStopped_002', async () => {
    const tab = legacy(); render(); await flushPromises()
    event('not-owned', completed)
    event(tab.ptyId!, completed, 2, 'wrong-session'); await flushPromises()
    expect(io.attention).not.toHaveBeenCalled()
    useAttentionStore().clearPty(tab.ptyId!)
    event(tab.ptyId!, completed, 3); await flushPromises()
    expect(io.attention).not.toHaveBeenCalled()
    const stopped = legacy(); stopped.status = 'stopped'
    event(stopped.ptyId!, completed, 4); await flushPromises()
    expect(io.attention).not.toHaveBeenCalled()
  })
  it('Attention_AcknowledgesOnlyVisibleSelectedFocusedCause_003', async () => {
    io.focused = true
    const tab = legacy(), other = legacy()
    useSessionStore().setActiveTab(tab.tabId)
    const { activeSessionId, visible } = render(`legacy-tab:${tab.tabId}`); await flushPromises()
    event(tab.ptyId!, completed); event(other.ptyId!, completed); await flushPromises()
    expect(useAttentionStore().getItem(tab.ptyId!)).toBeUndefined()
    expect(useAttentionStore().getItem(other.ptyId!)).toBeDefined()
    visible.value = false; useSessionStore().setActiveTab(other.tabId); activeSessionId.value = `legacy-tab:${other.tabId}`; await flushPromises()
    expect(useAttentionStore().getItem(other.ptyId!)).toBeDefined()
    visible.value = true; await flushPromises()
    expect(useAttentionStore().getItem(other.ptyId!)).toBeUndefined()
    event(other.ptyId!, { type: 'stopFailure', data: { error: 'error' } }, 3); await flushPromises()
    expect(useAttentionStore().getItem(other.ptyId!)?.kind).toBe('error')
    expect(io.attention.mock.calls.filter(([kind]) => kind === 2)).toHaveLength(0)
  })
  it('Attention_NativeOrderedWaitingButNeverUnknownOrRawCompletion_004', async () => {
    render(); await flushPromises()
    const tabs = useNativeTabsStore()
    for (const [cli, action] of [['codex', { kind: 'new' }], ['claude', { kind: 'raw', argv: [] }], ['claude', { kind: 'new' }]] as const) {
      const tab = tabs.create({ cli, action: { ...action, ...('argv' in action ? { argv: [...action.argv] } : {}) } as any, projectId: 'project', projectPath: '/repo', profileId: cli, profileRevision: '1' })
      tabs.tab(tab.tabId)!.status = 'running'
      tabs.applyObservation(tab.tabId, captureNativeAttempt(tab), { observation: 'active', activity: 'unknown' }); await flushPromises()
      expect(io.attention).not.toHaveBeenCalled()
      tabs.applyObservation(tab.tabId, captureNativeAttempt(tab), { observation: 'active', activity: 'waiting' }); await flushPromises()
      if (cli === 'claude' && action.kind !== 'raw') expect(io.attention).toHaveBeenCalledExactlyOnceWith(2)
      else expect(io.attention).not.toHaveBeenCalled()
    }
    const rows = [...tabs.tabs.values()], tab = rows[rows.length - 1]!
    tab.status = 'exited'; await flushPromises()
    expect(io.attention.mock.calls.filter(([kind]) => kind === 2)).toHaveLength(1)
  })
  it('Attention_UnmountStopsSubscriptions_005', async () => {
    const tab = legacy(); const { wrapper } = render(); await flushPromises()
    wrapper.unmount(); wrappers.splice(wrappers.indexOf(wrapper), 1)
    io.attention.mockClear()
    event(tab.ptyId!, completed); await flushPromises()
    expect(io.attention).not.toHaveBeenCalled()
    expect(io.unlisten).toHaveBeenCalledOnce()
  })
})

// Legacy消费者必须等真实初始焦点，而不是把ref初值视为可见确认。
it('Attention_DeferredFocusCannotAcknowledge_006', async () => {
  const tab = legacy(); useSessionStore().setActiveTab(tab.tabId)
  event(tab.ptyId!, completed)
  let finish!: (focused: boolean) => void
  io.readFocus.mockImplementationOnce(() => new Promise<boolean>(resolve => { finish = resolve }))
  render(`legacy-tab:${tab.tabId}`, true); await flushPromises()
  expect(useAttentionStore().getItem(tab.ptyId!)?.kind).toBe('completed')
  finish(false); await flushPromises()
  expect(useAttentionStore().getItem(tab.ptyId!)?.kind).toBe('completed')
  io.focus!({ payload: true }); await flushPromises()
  expect(useAttentionStore().getItem(tab.ptyId!)).toBeUndefined()
})

// 卸载时尚未完成的监听注册必须立即释放，迟到焦点回调不能写入或发通知。
it('Attention_DeferredSubscriptionDisposes_007', async () => {
  let finish!: (unsubscribe: () => void) => void
  io.subscribe.mockImplementationOnce(() => new Promise<() => void>(resolve => { finish = resolve }))
  const { wrapper } = render(); await flushPromises()
  wrapper.unmount(); wrappers.splice(wrappers.indexOf(wrapper), 1)
  io.attention.mockClear()
  finish(io.unlisten); await flushPromises()
  io.focus!({ payload: true }); await flushPromises()
  expect(io.unlisten).toHaveBeenCalledOnce()
  expect(io.readFocus).not.toHaveBeenCalled()
  expect(io.attention).not.toHaveBeenCalled()
})
