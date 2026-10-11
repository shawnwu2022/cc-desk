import { describe, it, expect, beforeEach, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { ref } from 'vue'

// crypto.randomUUID polyfill（同 statusMonitor.test / session.test）
if (typeof globalThis.crypto === 'undefined' || !globalThis.crypto.randomUUID) {
  Object.defineProperty(globalThis, 'crypto', {
    value: {
      ...globalThis.crypto,
      randomUUID: () =>
        'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
          const r = (Math.random() * 16) | 0
          const v = c === 'x' ? r : (r & 0x3) | 0x8
          return v.toString(16)
        }),
    },
    writable: true,
    configurable: true,
  })
}

// 捕获 vue onMounted（手动触发,控制 useStatusMonitor 订阅时机）
let mountedCbs: (() => void)[] = []
vi.mock('vue', async () => {
  const actual = await vi.importActual<typeof import('vue')>('vue')
  return {
    ...actual,
    onMounted: (fn: () => void) => mountedCbs.push(fn),
    onUnmounted: () => {},
  }
})

// 捕获 hook dispatch（onHookEvent 回调）
let capturedDispatch: ((p: any) => void) | null = null
vi.mock('@/api/tauri', () => ({
  ptyKill: vi.fn().mockResolvedValue(true),
  getSessionCount: vi.fn().mockResolvedValue(0),
  getSessions: vi.fn().mockResolvedValue([]),
  searchSessionMessages: vi.fn().mockResolvedValue([]),
  onHookEvent: vi.fn((cb: (p: any) => void) => {
    capturedDispatch = cb
    return Promise.resolve(() => {})
  }),
}))

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    requestUserAttention: vi.fn().mockResolvedValue(undefined),
  }),
  UserAttentionType: { Critical: 'Critical' },
}))

import { useSessionStore } from '@/stores/session'
import { useHookStore } from '@/stores/hook'
import { useAttentionStore } from '@/stores/attention'
import { useStatusMonitor } from '@/composables/useStatusMonitor'
import type { HookEventPayload } from '@/types/hook'

// 生产路径：后端 hook_events.rs 对所有 hook 事件恒打 claude-hook 鉴权标签
let authEventSeq = 0
function withAuth(payload: HookEventPayload): HookEventPayload {
  return { ...payload, runId: payload.ptyId ?? undefined, generation: 1, eventId: `evt-${++authEventSeq}`, observerSource: 'claude-hook' }
}

/** 构造 notification payload */
function notif(notificationType: string, ptyId = 'pty1'): HookEventPayload {
  return withAuth({
    ptyId, sessionId: 's', eventName: 'Notification', state: 'idle', timestamp: 1,
    detail: { type: 'notification', data: { notificationType } },
  } as HookEventPayload)
}

/** 构造 stopFailure payload */
function stopFail(ptyId = 'pty1'): HookEventPayload {
  return withAuth({
    ptyId, sessionId: 's', eventName: 'StopFailure', state: 'error', timestamp: 1,
    detail: { type: 'stopFailure', data: { error: 'x' } },
  } as HookEventPayload)
}

/**
 * 集成测试:真实 attention store + useStatusMonitor + hookStore,守护订阅顺序(codex P0)。
 *
 * P0 根因:hook dispatch 按 Set 插入顺序,若 useStatusMonitor(ack)订阅早于 attention(ingest),
 * active+focused 的 notification 会先 ack 空(item 未 ingest)再 ingest 残留,违背"看了就清"。
 * 修复:attention.init() 在 App setup(早于 TerminalView mount)→ attention 先订阅 → ingest 先于 ack。
 */
describe('attention 订阅顺序（看了就清,codex P0 守护）', () => {
  let session: ReturnType<typeof useSessionStore>
  let hook: ReturnType<typeof useHookStore>
  let attention: ReturnType<typeof useAttentionStore>

  beforeEach(() => {
    setActivePinia(createPinia())
    capturedDispatch = null
    mountedCbs = []
    session = useSessionStore()
    hook = useHookStore()
    attention = useAttentionStore()
    hook.init()
  })

  /** mount useStatusMonitor(active+focused+visible),触发其 onMounted 订阅 */
  function mountStatusMonitor() {
    const isFocused = ref(true)
    const isTerminalVisible = ref(true)
    useStatusMonitor({ isFocused, isTerminalVisible })
    mountedCbs.forEach((fn) => fn())
  }

  /** 建 active+running+working 的 tab(notification/stopFailure handler 需 working=true 才处理) */
  function createActiveWorkingTab(ptyId = 'pty1') {
    const tabId = session.createTab('/p')
    session.setTabPty(tabId, ptyId)
    session.setActiveTab(tabId)
    session.tabs.get(tabId)!.working = true
    return tabId
  }

  // 正确顺序(attention 先订阅,模拟 App setup 提前 init):active idle_prompt → ingest 后 ack 清
  it('AttentionOrder_AttentionFirst_IdleCleared_001', () => {
    createActiveWorkingTab()
    attention.init() // 先订阅(ingest)
    mountStatusMonitor() // 后订阅(ack)

    capturedDispatch!(notif('idle_prompt'))

    expect(attention.getItem('pty1')).toBeUndefined()
  })

  // permission 同理:active permission_prompt → 清
  it('AttentionOrder_AttentionFirst_PermissionCleared_001', () => {
    createActiveWorkingTab()
    attention.init()
    mountStatusMonitor()

    capturedDispatch!(notif('permission_prompt'))

    expect(attention.getItem('pty1')).toBeUndefined()
  })

  // error 即使 active 也保留(粘性:active stopFailure → error 红点持续,ackPty 默认不清 error)
  it('AttentionOrder_AttentionFirst_ErrorKept_001', () => {
    createActiveWorkingTab()
    attention.init()
    mountStatusMonitor()

    capturedDispatch!(stopFail())

    expect(attention.getItem('pty1')?.kind).toBe('error')
  })

  // 错误顺序(statusMonitor 先订阅,模拟 P0 未修):active idle_prompt → ack 空 + ingest 残留
  // 守护 hookStore dispatch 按 Set 顺序的契约 + 文档化 P0 根因
  it('AttentionOrder_StatusFirst_IdleLeaked_001', () => {
    createActiveWorkingTab()
    mountStatusMonitor() // 先订阅(ack,错误顺序)
    attention.init() // 后订阅(ingest)

    capturedDispatch!(notif('idle_prompt'))

    // P0 演示:statusMonitor ack 空(item 未 ingest),attention 后 ingest 残留
    expect(attention.getItem('pty1')?.kind).toBe('completed')
  })
})
