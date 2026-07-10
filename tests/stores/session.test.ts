import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useSessionStore } from '@/stores/session'
import { randomUUID } from 'crypto'

// crypto.randomUUID polyfill for jsdom
if (typeof globalThis.crypto === 'undefined' || !globalThis.crypto.randomUUID) {
  Object.defineProperty(globalThis, 'crypto', {
    value: {
      ...globalThis.crypto,
      randomUUID: () => randomUUID(),
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
})
