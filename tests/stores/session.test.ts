import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useSessionStore, filterDeletable, groupByProject } from '@/stores/session'
import { useAttentionStore } from '@/stores/attention'

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

// Mock @/api/tauri
vi.mock('@/api/tauri', () => ({
  ptyKill: vi.fn().mockResolvedValue(true),
  getSessionCount: vi.fn().mockResolvedValue(0),
  getSessions: vi.fn().mockResolvedValue([]),
  searchSessionMessages: vi.fn().mockResolvedValue([]),
  getProjectsState: vi.fn().mockResolvedValue({ pinnedProjects: [], archivedSessions: {}, displayNames: {} }),
  deleteSessions: vi.fn().mockResolvedValue({ pinnedProjects: [], archivedSessions: {}, displayNames: {} }),
}))

describe('session store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  // ==================== createTab ====================

  describe('createTab', () => {
    // 创建 tab 包含正确的 projectPath
    it('CreateTab_ProjectPath_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/home/user/project-a')
      const tab = store.tabs.get(tabId)!
      expect(tab.projectPath).toBe('/home/user/project-a')
    })

    // 设置 sessionId="abc12345def" 时 name 取前 8 字符 "abc12345"
    it('CreateTab_SessionIdName_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project', { sessionId: 'abc12345def' })
      const tab = store.tabs.get(tabId)!
      expect(tab.name).toBe('abc12345')
    })

    // 不设 sessionId 时 name 为 "New Session"
    it('CreateTab_DefaultName_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project')
      const tab = store.tabs.get(tabId)!
      expect(tab.name).toBe('New Session')
    })

    // 初始 status 为 "stopped"
    it('CreateTab_InitialState_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project')
      const tab = store.tabs.get(tabId)!
      expect(tab.status).toBe('stopped')
      expect(tab.ptyId).toBeNull()
      expect(tab.sessionId).toBeNull()
      expect(tab.working).toBe(false)
      expect(tab.pending).toBe(false)
    })

    // 连续创建两个 tab 的 tabId 不同
    it('CreateTab_UniqueId_001', () => {
      const store = useSessionStore()
      const id1 = store.createTab('/project')
      const id2 = store.createTab('/project')
      expect(id1).not.toBe(id2)
    })
  })

  // ==================== closeTab ====================

  describe('closeTab', () => {
    // 关闭 tab 后从 store 中移除
    it('CloseTab_Remove_001', async () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project')
      store.setActiveTab(tabId)
      await store.closeTab(tabId)
      expect(store.tabs.has(tabId)).toBe(false)
    })

    // 关闭活跃 tab 时 activeTabId 切换到同项目其他 tab
    it('CloseTab_SwitchActive_001', async () => {
      const store = useSessionStore()
      const id1 = store.createTab('/project-x')
      const id2 = store.createTab('/project-x')
      store.setActiveTab(id2)
      await store.closeTab(id2)
      expect(store.activeTabId).toBe(id1)
    })

    // 关闭项目中最后一个 tab 时 activeTabId 变为 null
    it('CloseTab_LastTab_001', async () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project-y')
      store.setActiveTab(tabId)
      await store.closeTab(tabId)
      expect(store.activeTabId).toBeNull()
    })

    // 关闭有 PTY 的 tab 时，tab 立即删除，PTY 异步 kill 不阻塞
    it('CloseTab_ImmediateDelete_001', async () => {
      const { ptyKill } = await import('@/api/tauri')
      const mockKill = ptyKill as ReturnType<typeof vi.fn>

      const store = useSessionStore()
      const tabId = store.createTab('/project')
      store.setTabPty(tabId, 'pty-immediate')

      // closeTab 不再 await ptyKill，tab 应立即删除
      await store.closeTab(tabId)
      expect(store.tabs.has(tabId)).toBe(false)
      expect(mockKill).toHaveBeenCalledWith('pty-immediate')
    })

    // 关闭 tab 后 claimedSessionIds 释放，历史会话自动显示
    it('CloseTab_ReleaseClaim_001', async () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project', { sessionId: 'sess-release' })

      expect(store.claimedSessionIds.has('sess-release')).toBe(true)
      await store.closeTab(tabId)
      expect(store.claimedSessionIds.has('sess-release')).toBe(false)
    })

    // 关闭新会话 tab 后应刷新历史会话，让该会话立即出现在历史列表
    // 复现：新会话的 sessionId 在新建时因时序过早未进入历史缓存，
    // 关闭 tab 释放 claimed 后若不刷新，历史列表仍不显示该会话
    it('CloseTab_RefreshHistory_001', async () => {
      const { getSessions } = await import('@/api/tauri')
      const mockGetSessions = getSessions as ReturnType<typeof vi.fn>

      const store = useSessionStore()
      const tabId = store.createTab('/project')
      store.setTabPty(tabId, 'pty-refresh')
      // 模拟 hook 事件为新会话赋 sessionId
      store.setTabSessionId(tabId, 'sess-new')

      // 模拟新会话的 JSONL 已存在（claude 运行期间创建），刷新即可读到
      mockGetSessions.mockResolvedValue([
        { sessionId: 'sess-new', name: 'New Session', projectPath: '/project', lastActiveAt: 1000 },
      ])
      mockGetSessions.mockClear()

      await store.closeTab(tabId)

      // 关闭后应触发历史刷新
      expect(mockGetSessions).toHaveBeenCalled()
      // 刷新后新会话应出现在历史列表（claimed 已释放）
      expect(store.historySessions.some(s => s.sessionId === 'sess-new')).toBe(true)
    })
  })

  // ==================== closeAllTabs ====================

  describe('closeAllTabs', () => {
    // 关闭所有 tab 后应刷新历史会话，让被关闭的会话出现在历史列表
    it('CloseAllTabs_RefreshHistory_001', async () => {
      const { getSessions } = await import('@/api/tauri')
      const mockGetSessions = getSessions as ReturnType<typeof vi.fn>

      const store = useSessionStore()
      const t1 = store.createTab('/project')
      store.setTabPty(t1, 'pty-1')
      store.setTabSessionId(t1, 'sess-1')
      const t2 = store.createTab('/project')
      store.setTabPty(t2, 'pty-2')
      store.setTabSessionId(t2, 'sess-2')

      mockGetSessions.mockResolvedValue([
        { sessionId: 'sess-1', name: 'S1', projectPath: '/project', lastActiveAt: 1000 },
        { sessionId: 'sess-2', name: 'S2', projectPath: '/project', lastActiveAt: 2000 },
      ])
      mockGetSessions.mockClear()

      await store.closeAllTabs('/project')

      expect(mockGetSessions).toHaveBeenCalled()
      const ids = store.historySessions.map(s => s.sessionId)
      expect(ids).toContain('sess-1')
      expect(ids).toContain('sess-2')
    })
  })

  // ==================== closeOtherTabs ====================

  describe('closeOtherTabs', () => {
    // 关闭其他 tab 后应刷新历史会话（保留的 tab 仍 claimed，不出现在历史）
    it('CloseOtherTabs_RefreshHistory_001', async () => {
      const { getSessions } = await import('@/api/tauri')
      const mockGetSessions = getSessions as ReturnType<typeof vi.fn>

      const store = useSessionStore()
      const keep = store.createTab('/project')
      store.setTabPty(keep, 'pty-keep')
      store.setTabSessionId(keep, 'sess-keep')
      const other = store.createTab('/project')
      store.setTabPty(other, 'pty-other')
      store.setTabSessionId(other, 'sess-other')

      mockGetSessions.mockResolvedValue([
        { sessionId: 'sess-keep', name: 'Keep', projectPath: '/project', lastActiveAt: 1000 },
        { sessionId: 'sess-other', name: 'Other', projectPath: '/project', lastActiveAt: 2000 },
      ])
      mockGetSessions.mockClear()

      await store.closeOtherTabs(keep)

      expect(mockGetSessions).toHaveBeenCalled()
      const ids = store.historySessions.map(s => s.sessionId)
      // 被关闭的 other 应出现在历史；保留的 keep 仍 claimed，不出现
      expect(ids).toContain('sess-other')
      expect(ids).not.toContain('sess-keep')
    })
  })

  // ==================== handlePtyExit ====================

  describe('handlePtyExit', () => {
    // PTY 退出后 tab 状态设为 stopped
    it('PtyExit_Status_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project')
      store.setTabPty(tabId, 'pty-001')
      store.handlePtyExit('pty-001')
      const tab = store.tabs.get(tabId)!
      expect(tab.status).toBe('stopped')
    })

    // PTY 退出后 ptyId 清空
    it('PtyExit_ClearPtyId_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project')
      store.setTabPty(tabId, 'pty-002')
      store.handlePtyExit('pty-002')
      const tab = store.tabs.get(tabId)!
      expect(tab.ptyId).toBeNull()
    })

    // PTY 退出后 working 设为 false
    it('PtyExit_ClearWorking_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project')
      store.setTabPty(tabId, 'pty-003')
      const tab = store.tabs.get(tabId)!
      tab.working = true
      store.handlePtyExit('pty-003')
      expect(tab.working).toBe(false)
    })

    // codex 对抗审查：PTY 退出清 pending，修「权限→pending→PTY 退出」后 stopped tab 残留永久告警的泄漏
    it('PtyExit_ClearPending_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project')
      store.setTabPty(tabId, 'pty-pending')
      const tab = store.tabs.get(tabId)!
      tab.pending = true
      store.handlePtyExit('pty-pending')
      expect(tab.pending).toBe(false)
    })

    // v6 codex batch1 #1：handlePtyExit 后 getRunningTabForProject 不再返回该 tab--
    // 这是 timeout 路径显式调 handlePtyExit 的动机：避免 PTY kill 后 exit 事件找不到实例导致
    // tab 仍 status=running，retry 误判「已有运行 tab」或起重复 Claude 进程。
    it('PtyExit_NotRunningAfterExit_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project-x')
      store.setTabPty(tabId, 'pty-timeout')
      // 启动后该 tab 为运行中，getRunningTabForProject 命中
      expect(store.getRunningTabForProject('/project-x')?.tabId).toBe(tabId)
      // PTY kill 后显式 handlePtyExit（贴 fix #1）-> tab 不再 running
      store.handlePtyExit('pty-timeout')
      expect(store.getRunningTabForProject('/project-x')).toBeNull()
      const tab = store.tabs.get(tabId)!
      expect(tab.status).toBe('stopped')
      expect(tab.ptyId).toBeNull()
    })

    // v6 codex batch1 #1：handlePtyExit 对未知 ptyId 安全 no-op（不抛错、不影响其他 tab）--
    // timeout 路径 ptyKill 后 exit 事件可能再次到达，幂等 handlePtyExit 不应破坏状态。
    it('PtyExit_Idempotent_UnknownPtyId_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project-y')
      store.setTabPty(tabId, 'pty-known')
      store.handlePtyExit('pty-known')
      // 再次调（exit 事件重复到达）或调未知 id：no-op，不抛错
      expect(() => store.handlePtyExit('pty-known')).not.toThrow()
      expect(() => store.handlePtyExit('pty-unknown')).not.toThrow()
      const tab = store.tabs.get(tabId)!
      expect(tab.status).toBe('stopped') // 状态稳定，未被重复 exit 破坏
      expect(tab.ptyId).toBeNull()
    })
  })

  // ==================== claimedSessionIds ====================

  describe('claimedSessionIds', () => {
    // 两个 tab 分别设置 sessionId 时集合包含两个值
    it('ClaimedIds_Include_001', () => {
      const store = useSessionStore()
      const id1 = store.createTab('/project', { sessionId: 'sess-alpha' })
      const id2 = store.createTab('/project', { sessionId: 'sess-beta' })
      store.setTabSessionId(id1, 'sess-alpha')
      store.setTabSessionId(id2, 'sess-beta')
      const claimed = store.claimedSessionIds
      expect(claimed.has('sess-alpha')).toBe(true)
      expect(claimed.has('sess-beta')).toBe(true)
      expect(claimed.size).toBe(2)
    })

    // sessionId 为 null 的 tab 不出现在集合中
    it('ClaimedIds_ExcludeNull_001', () => {
      const store = useSessionStore()
      store.createTab('/project') // no sessionId
      const claimed = store.claimedSessionIds
      expect(claimed.size).toBe(0)
    })
  })

  // ==================== isResume ====================

  describe('isResume', () => {
    // 创建 tab 时传入 sessionId，isResume 为 true
    it('IsResume_True_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project', { sessionId: 'sess-resume-001' })
      const tab = store.tabs.get(tabId)!
      expect(tab.isResume).toBe(true)
    })

    // 创建 tab 时不传 sessionId，isResume 为 false
    it('IsResume_False_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project')
      const tab = store.tabs.get(tabId)!
      expect(tab.isResume).toBe(false)
    })

    // 创建 tab 时传入 sessionId 和 name，isResume 仍为 true
    it('IsResume_WithName_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/project', { sessionId: 'sess-123', name: 'My Session' })
      const tab = store.tabs.get(tabId)!
      expect(tab.isResume).toBe(true)
      expect(tab.name).toBe('My Session')
    })
  })

  // ==================== setActiveTab ack ====================
  describe('setActiveTab ack', () => {
    // setActiveTab 调 ackPty 清非 error（切到 = 已关注，completed 清除）
    it('SetActiveTab_AckCompleted_001', () => {
      const sessionStore = useSessionStore()
      const attentionStore = useAttentionStore()
      const tabId = sessionStore.createTab('/p')
      sessionStore.setTabPty(tabId, 'pty-x')
      attentionStore.ingestEvent({
        ptyId: 'pty-x', sessionId: 's', eventName: 'Notification', state: 'idle', timestamp: 1,
        detail: { type: 'notification', data: { notificationType: 'idle_prompt' } },
      } as any)
      expect(attentionStore.getItem('pty-x')?.kind).toBe('completed')

      sessionStore.setActiveTab(tabId)
      expect(attentionStore.getItem('pty-x')).toBeUndefined() // completed 被清
    })

    // setActiveTab 不清 error（error 粘性，需新回合/clearPty）
    it('SetActiveTab_KeepError_001', () => {
      const sessionStore = useSessionStore()
      const attentionStore = useAttentionStore()
      const tabId = sessionStore.createTab('/p')
      sessionStore.setTabPty(tabId, 'pty-x')
      attentionStore.ingestEvent({
        ptyId: 'pty-x', sessionId: 's', eventName: 'StopFailure', state: 'error', timestamp: 1,
        detail: { type: 'stopFailure', data: { error: 'x' } },
      } as any)
      expect(attentionStore.getItem('pty-x')?.kind).toBe('error')

      sessionStore.setActiveTab(tabId)
      expect(attentionStore.getItem('pty-x')?.kind).toBe('error') // error 保留
    })
  })

  // ==================== deleteSessions（永久删除） ====================
  describe('deleteSessions', () => {
    // 成功:applyReturnedState 清标记 + force 重载历史（getSessions 被调）。
    // 注意:action 先 ensureProjectsStateLoaded,会用 getProjectsState 返回值覆盖本地 map,
    // 所以「删除前状态」必须由 getProjectsState mock 提供,不能手动 set(会被预加载冲掉)。
    it('DeleteSessions_AppliesStateAndForceReloads_001', async () => {
      const { getProjectsState, deleteSessions, getSessions } = await import('@/api/tauri')
      const mockLoad = getProjectsState as ReturnType<typeof vi.fn>
      const mockDelete = deleteSessions as ReturnType<typeof vi.fn>
      const mockGet = getSessions as ReturnType<typeof vi.fn>
      // 预加载:含 s1 存档标记(删除前状态)
      mockLoad.mockResolvedValueOnce({ pinnedProjects: [], archivedSessions: { '/p': ['s1'] }, displayNames: {} })
      // 删除返回:标记已清(删除后状态)
      mockDelete.mockResolvedValueOnce({ pinnedProjects: [], archivedSessions: {}, displayNames: {} })
      mockGet.mockClear()
      const store = useSessionStore()
      await store.deleteSessions('/p', ['s1'])
      expect(mockDelete).toHaveBeenCalledWith('/p', ['s1'])
      expect(store.archivedSessions.has('/p')).toBe(false) // applyReturnedState 清标记
      expect(mockGet).toHaveBeenCalled()                    // force 重载历史
    })

    // 失败:不 apply、不 force 重载,预加载的标记保留
    it('DeleteSessions_Failure_NoApplyNoReload_002', async () => {
      const { getProjectsState, deleteSessions, getSessions } = await import('@/api/tauri')
      const mockLoad = getProjectsState as ReturnType<typeof vi.fn>
      const mockDelete = deleteSessions as ReturnType<typeof vi.fn>
      const mockGet = getSessions as ReturnType<typeof vi.fn>
      mockLoad.mockResolvedValueOnce({ pinnedProjects: [], archivedSessions: { '/p': ['s1'] }, displayNames: {} })
      mockDelete.mockRejectedValueOnce(new Error('delete failed'))
      mockGet.mockClear()
      const store = useSessionStore()
      await expect(store.deleteSessions('/p', ['s1'])).rejects.toThrow()
      expect(mockGet).not.toHaveBeenCalled()                // 失败不 force 重载
      expect(store.archivedSessions.get('/p')).toEqual(['s1']) // 预加载标记未被清
    })

    // opLock 串行:两个并发 delete,第二个必须等第一个完成后才发 invoke
    it('DeleteSessions_OpLockSerializes_003', async () => {
      const { getProjectsState, deleteSessions } = await import('@/api/tauri')
      const mockLoad = getProjectsState as ReturnType<typeof vi.fn>
      const mockDelete = deleteSessions as ReturnType<typeof vi.fn>
      mockLoad.mockResolvedValue({ pinnedProjects: [], archivedSessions: {}, displayNames: {} })
      const emptyState = { pinnedProjects: [], archivedSessions: {}, displayNames: {} }
      // 手控 resolve 顺序:第一个调用未 resolve 前,第二个不得发起。
      // resolve 值必须是合法 state(后续 applyReturnedState 会读它,不能是 null)。
      let resolveFirst!: (v: unknown) => void
      const order: string[] = []
      mockDelete.mockImplementationOnce(() => new Promise(r => { resolveFirst = r; order.push('first-start') }))
      mockDelete.mockImplementationOnce(() => { order.push('second-start'); return Promise.resolve(emptyState) })
      const store = useSessionStore()
      const p1 = store.deleteSessions('/p', ['a'])
      const p2 = store.deleteSessions('/p', ['b'])
      // 宏任务边界:setTimeout(0) 前所有微任务(含 ensureProjectsStateLoaded 的多级 await)排空
      await new Promise(r => setTimeout(r, 0))
      expect(order).toEqual(['first-start']) // 第二个尚未发起(opLock 排队)
      resolveFirst(emptyState)
      await Promise.all([p1, p2])
      expect(order).toEqual(['first-start', 'second-start'])
    })
  })
})

describe('filterDeletable', () => {
  // 运行中会话被滤除,其余保留
  it('FilterDeletable_RemovesRunning_001', () => {
    expect(filterDeletable(['a', 'b', 'c'], new Set(['b'])).sort()).toEqual(['a', 'c'])
  })
  // 空运行中集合:全部保留
  it('FilterDeletable_NoRunning_KeepAll_002', () => {
    expect(filterDeletable(['a', 'b'], new Set())).toEqual(['a', 'b'])
  })
  // 全部运行中:结果为空
  it('FilterDeletable_AllRunning_Empty_003', () => {
    expect(filterDeletable(['a', 'b'], new Set(['a', 'b']))).toEqual([])
  })
  // 空输入
  it('FilterDeletable_EmptyInput_004', () => {
    expect(filterDeletable([], new Set(['a']))).toEqual([])
  })
})

describe('groupByProject', () => {
  // 单项目保持一组
  it('GroupByProject_SingleProject_001', () => {
    const g = groupByProject([{ projectPath: 'p1', sessionId: 'a' }, { projectPath: 'p1', sessionId: 'b' }])
    expect([...g.entries()]).toEqual([['p1', ['a', 'b']]])
  })
  // 跨项目正确拆组
  it('GroupByProject_MultiProject_002', () => {
    const g = groupByProject([
      { projectPath: 'p1', sessionId: 'a' },
      { projectPath: 'p2', sessionId: 'b' },
      { projectPath: 'p1', sessionId: 'c' },
    ])
    expect([...g.entries()]).toEqual([['p1', ['a', 'c']], ['p2', ['b']]])
  })
  // 空输入
  it('GroupByProject_Empty_003', () => {
    expect(groupByProject([]).size).toBe(0)
  })
})
