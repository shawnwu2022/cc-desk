import { describe, it, expect, beforeEach, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { ref } from 'vue'
// crypto.randomUUID polyfill for jsdom：若环境无 webcrypto.randomUUID（旧 jsdom），
// 用 Math.random 生成符合 UUID v4 格式的 id（测试用，非密码学安全，不依赖 node crypto 类型）
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

// 捕获 vue 生命周期回调
let mountedCbs: (() => void)[] = []
let unmountedCbs: (() => void)[] = []

vi.mock('vue', async () => {
  const actual = await vi.importActual<typeof import('vue')>('vue')
  return {
    ...actual,
    onMounted: (fn: () => void) => mountedCbs.push(fn),
    onUnmounted: (fn: () => void) => unmountedCbs.push(fn),
  }
})

// Mock @/api/tauri — 捕获 onHookEvent 回调
let capturedDispatch: ((payload: any) => void) | null = null

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

// Mock Tauri window API
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    requestUserAttention: vi.fn().mockResolvedValue(undefined),
  }),
  UserAttentionType: { Critical: 'Critical' },
}))

import { useSessionStore } from '@/stores/session'
import { useHookStore } from '@/stores/hook'
import type { HookEventPayload, HookEventDetail } from '@/types/hook'
import { useStatusMonitor } from '@/composables/useStatusMonitor'

/** 各事件类型对应的默认 data */
const DEFAULT_DATA: Record<string, unknown> = {
  userPromptSubmit: { prompt: 'test prompt' },
  sessionStart: { model: 'claude-sonnet-4-6', cwd: '/project' },
  preToolUse: { toolName: 'Read' },
  postToolUse: { toolName: 'Read' },
  postToolUseFailure: { toolName: 'Bash', error: 'failed' },
  stop: {},
  stopFailure: { error: 'API error' },
  sessionEnd: { reason: 'prompt_input_exit' },
  subagentStart: { agentId: 'agent-1', agentType: 'Explore' },
  subagentStop: { agentId: 'agent-1', agentType: 'Explore' },
  preCompact: { trigger: 'auto' },
  postCompact: { trigger: 'auto' },
}

/** 构建测试用的 HookEventPayload */
function makePayload(
  type: HookEventDetail['type'],
  ptyId: string = 'pty1',
  extra?: Partial<HookEventPayload>,
): HookEventPayload {
  return {
    ptyId,
    sessionId: 'session1',
    eventName: type,
    state: 'thinking',
    timestamp: Date.now(),
    detail: { type, data: (DEFAULT_DATA[type] ?? null) as any } as HookEventDetail,
    ...extra,
  }
}

/** 构建 notification 类型的 payload */
function makeNotificationPayload(
  notificationType: string,
  ptyId: string = 'pty1',
): HookEventPayload {
  return makePayload('notification', ptyId, {
    detail: {
      type: 'notification',
      data: { notificationType, message: 'test' },
    } as any,
  })
}

/** 创建一个处于 running 状态且绑定 ptyId 的 tab */
function createRunningTab(ptyId = 'pty1', projectPath = '/project'): string {
  const store = useSessionStore()
  const tabId = store.createTab(projectPath)
  const tab = store.tabs.get(tabId)!
  tab.status = 'running'
  tab.ptyId = ptyId
  return tabId
}

/** 挂载 useStatusMonitor 并触发 onMounted */
function mountMonitor() {
  const isFocused = ref(true)
  const isTerminalVisible = ref(true)

  const hookStore = useHookStore()
  hookStore.init()

  useStatusMonitor({ isFocused, isTerminalVisible })

  // 触发 onMounted 回调
  mountedCbs.forEach((fn) => fn())

  return { isFocused, isTerminalVisible }
}

/** 通过 hook store dispatch 发送事件 */
function emit(payload: HookEventPayload) {
  capturedDispatch!(payload)
}

describe('useStatusMonitor', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    capturedDispatch = null
    mountedCbs = []
    unmountedCbs = []
  })

  // ==================== 基本状态转换 ====================

  describe('基本状态转换', () => {
    // userPromptSubmit → working = true
    it('StatusMonitor_UserPromptSubmit_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(true)
    })

    // stop → working = false, pending = true
    it('StatusMonitor_Stop_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('stop', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(true)
    })

    // notification(idle_prompt) → working = false, pending = true
    it('StatusMonitor_NotificationIdle_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makeNotificationPayload('idle_prompt', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(true)
    })
  })

  // ==================== Notification 类型区分 ====================

  describe('Notification 类型区分', () => {
    // computer_use_enter 不会中断 working 状态
    it('StatusMonitor_NotificationComputerUse_NoEffect_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makeNotificationPayload('computer_use_enter', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(true)
      expect(tab.pending).toBe(false)
    })

    // permission_prompt → working=false, pending=true（等待用户授权）
    it('StatusMonitor_NotificationPermPrompt_Pending_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makeNotificationPayload('permission_prompt', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(true)
    })

    // worker_permission_prompt → working=false, pending=true
    it('StatusMonitor_NotificationWorkerPerm_Pending_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makeNotificationPayload('worker_permission_prompt', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(true)
    })

    // permission_prompt 后 PostToolUse 恢复 working（授权后继续工作）
    it('StatusMonitor_PermPrompt_ThenPostToolUse_Restore_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makeNotificationPayload('permission_prompt', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(false)

      // 用户授权后工具执行
      emit(makePayload('postToolUse', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)
      expect(useSessionStore().getTabByPtyId('pty1')!.pending).toBe(false)
    })

    // permission_prompt 后 PostToolUseFailure 也能恢复 working（拒绝后继续）
    it('StatusMonitor_PermPrompt_ThenPostToolFail_Restore_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makeNotificationPayload('permission_prompt', 'pty1'))

      // 用户拒绝后
      emit(makePayload('postToolUseFailure', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)
      expect(useSessionStore().getTabByPtyId('pty1')!.pending).toBe(false)
    })

    // 完整权限流程：prompt → 工具 → 权限提示 → 授权 → 工具完成 → Stop
    it('StatusMonitor_FullPermissionFlow_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)

      emit(makePayload('preToolUse', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)

      emit(makeNotificationPayload('permission_prompt', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(false)
      expect(useSessionStore().getTabByPtyId('pty1')!.pending).toBe(true)

      emit(makePayload('postToolUse', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)
      expect(useSessionStore().getTabByPtyId('pty1')!.pending).toBe(false)

      emit(makePayload('stop', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(false)
      expect(useSessionStore().getTabByPtyId('pty1')!.pending).toBe(true)
    })

    // 非 idle_prompt/permission 的 notification 在 working=false 时被忽略
    it('StatusMonitor_NotificationNotIdleWhenIdle_Ignored_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makeNotificationPayload('computer_use_enter', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(false)
    })
  })

  // ==================== 回合内活跃事件 ====================

  describe('回合内活跃事件', () => {
    // 回合内收到 preToolUse → working 保持 true
    it('StatusMonitor_InTurnActivity_PreToolUse_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('preToolUse', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(true)
      expect(tab.pending).toBe(false)
    })

    // 回合内收到 postToolUse → working 保持 true
    it('StatusMonitor_InTurnActivity_PostToolUse_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('postToolUse', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(true)
      expect(tab.pending).toBe(false)
    })

    // 回合内收到 subagentStart → working 保持 true
    it('StatusMonitor_InTurnActivity_SubagentStart_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('subagentStart', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(true)
      expect(tab.pending).toBe(false)
    })

    // 回合内收到 preCompact → working 保持 true
    it('StatusMonitor_InTurnActivity_PreCompact_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('preCompact', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(true)
      expect(tab.pending).toBe(false)
    })
  })

  // ==================== 回合结束保护（recap 防护） ====================

  describe('回合结束保护（recap 等后续操作不应恢复 working）', () => {
    // Stop 后 PreToolUse 不应恢复 working
    it('StatusMonitor_RecapGuard_PreToolUse_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('stop', 'pty1'))
      emit(makePayload('preToolUse', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(true)
    })

    // Stop 后 PostToolUse 不应恢复 working
    it('StatusMonitor_RecapGuard_PostToolUse_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('stop', 'pty1'))
      emit(makePayload('postToolUse', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(true)
    })

    // Stop 后 SubagentStart 不应恢复 working
    it('StatusMonitor_RecapGuard_SubagentStart_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('stop', 'pty1'))
      emit(makePayload('subagentStart', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(true)
    })

    // Stop 后 SubagentStop 不应恢复 working
    it('StatusMonitor_RecapGuard_SubagentStop_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('stop', 'pty1'))
      emit(makePayload('subagentStop', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(true)
    })

    // Stop 后 PreCompact 不应恢复 working
    it('StatusMonitor_RecapGuard_PreCompact_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('stop', 'pty1'))
      emit(makePayload('preCompact', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(true)
    })

    // idle_prompt 后活动事件也不应恢复 working
    it('StatusMonitor_RecapGuard_AfterIdlePrompt_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makeNotificationPayload('idle_prompt', 'pty1'))
      emit(makePayload('preToolUse', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(true)
    })

    // StopFailure 后活动事件也不应恢复 working
    it('StatusMonitor_RecapGuard_AfterStopFailure_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('stopFailure', 'pty1'))
      emit(makePayload('preToolUse', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(true)
    })

    // UserPromptSubmit 重置回合，活动事件恢复生效
    it('StatusMonitor_TurnReset_AfterNewPrompt_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      // 第一回合
      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('stop', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(false)

      // recap 的活动事件被忽略
      emit(makePayload('preToolUse', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(false)

      // 第二回合：新的 prompt 重置回合标记
      emit(makePayload('userPromptSubmit', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)

      // 新回合内活动事件正常工作
      emit(makePayload('preToolUse', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)
    })
  })

  // ==================== 完整流程 ====================

  describe('完整流程', () => {
    // 正常工具使用流程
    it('StatusMonitor_FullFlow_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)

      emit(makePayload('preToolUse', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)

      emit(makePayload('postToolUse', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)

      emit(makePayload('stop', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(false)
      expect(useSessionStore().getTabByPtyId('pty1')!.pending).toBe(true)
    })

    // computer_use 通知不干扰正常流程
    it('StatusMonitor_ComputerUseFlow_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)

      emit(makeNotificationPayload('computer_use_enter', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)

      emit(makeNotificationPayload('computer_use_exit', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)

      emit(makePayload('stop', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(false)
    })

    // recap 场景：Stop 后 recap 的工具调用不应恢复 working
    it('StatusMonitor_RecapFlow_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      // 正常工作流程
      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('preToolUse', 'pty1'))
      emit(makePayload('postToolUse', 'pty1'))

      // 回合结束
      emit(makePayload('stop', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(false)
      expect(useSessionStore().getTabByPtyId('pty1')!.pending).toBe(true)

      // recap 内部操作被忽略
      emit(makePayload('preToolUse', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(false)
      expect(useSessionStore().getTabByPtyId('pty1')!.pending).toBe(true)

      emit(makePayload('postToolUse', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(false)
      expect(useSessionStore().getTabByPtyId('pty1')!.pending).toBe(true)

      // recap 可能的第二个 Stop 也不影响
      emit(makePayload('stop', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(false)
      expect(useSessionStore().getTabByPtyId('pty1')!.pending).toBe(true)
    })

    // compact 流程（回合内的压缩是活跃操作）
    it('StatusMonitor_CompactFlow_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)

      emit(makePayload('preCompact', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)

      emit(makePayload('postCompact', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(true)

      emit(makePayload('stop', 'pty1'))
      expect(useSessionStore().getTabByPtyId('pty1')!.working).toBe(false)
    })
  })

  // ==================== 不影响原有逻辑 ====================

  describe('原有逻辑不受影响', () => {
    // working = false 时 stop 事件被忽略
    it('StatusMonitor_IgnoreStopWhenIdle_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('stop', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(false)
    })

    // sessionEnd → working = false，不设 pending
    it('StatusMonitor_SessionEnd_NoPending_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('sessionEnd', 'pty1'))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(false)
    })

    // sessionStart 分配 sessionId
    it('StatusMonitor_SessionStart_001', () => {
      createRunningTab('pty1')
      mountMonitor()

      emit(makePayload('sessionStart', 'pty1', {
        sessionId: 'new-session-id',
        detail: {
          type: 'sessionStart',
          data: { model: 'claude-sonnet-4-6' },
        } as any,
      }))

      const tab = useSessionStore().getTabByPtyId('pty1')!
      expect(tab.sessionId).toBe('new-session-id')
      expect(tab.model).toBe('claude-sonnet-4-6')
    })

    // Stop 时用户正在看 tab → pending 立即清除（不产生假 pending）
    it('StatusMonitor_StopWhileWatching_NoPending_001', () => {
      createRunningTab('pty1')
      const { isFocused, isTerminalVisible } = mountMonitor()
      const store = useSessionStore()

      // 用户正在看 tab（focused + visible + active）
      isFocused.value = true
      isTerminalVisible.value = true
      store.activeTabId = store.getTabByPtyId('pty1')!.tabId

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makePayload('stop', 'pty1'))

      const tab = store.getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(false) // 关键：用户在看时不应该有 pending
    })

    // idle_prompt 时用户正在看 tab → pending 立即清除
    it('StatusMonitor_IdlePromptWhileWatching_NoPending_001', () => {
      createRunningTab('pty1')
      const { isFocused, isTerminalVisible } = mountMonitor()
      const store = useSessionStore()

      isFocused.value = true
      isTerminalVisible.value = true
      store.activeTabId = store.getTabByPtyId('pty1')!.tabId

      emit(makePayload('userPromptSubmit', 'pty1'))
      emit(makeNotificationPayload('idle_prompt', 'pty1'))

      const tab = store.getTabByPtyId('pty1')!
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(false)
    })
  })
})
