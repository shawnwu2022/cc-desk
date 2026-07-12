import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { randomUUID } from 'crypto'

if (typeof globalThis.crypto === 'undefined' || !globalThis.crypto.randomUUID) {
  Object.defineProperty(globalThis, 'crypto', {
    value: { ...globalThis.crypto, randomUUID: () => randomUUID() },
    writable: true, configurable: true,
  })
}

vi.mock('@/api/tauri', () => ({
  ptyKill: vi.fn().mockResolvedValue(true),
  getSessionCount: vi.fn().mockResolvedValue(0),
  getSessions: vi.fn().mockResolvedValue([]),
  searchSessionMessages: vi.fn().mockResolvedValue([]),
}))

import { useSessionStore } from '@/stores/session'

describe('session store — 全局树', () => {
  beforeEach(() => setActivePinia(createPinia()))

  // ==================== expandedProjectPaths ====================
  describe('toggleExpand / isExpanded', () => {
    // toggleExpand 后 isExpanded 反映手动展开状态
    it('Expand_ToggleManual_001', () => {
      const store = useSessionStore()
      expect(store.isExpanded('/p-a', { hasActive: false, isCurrent: false })).toBe(false)
      store.toggleExpand('/p-a')
      expect(store.isExpanded('/p-a', { hasActive: false, isCurrent: false })).toBe(true)
      store.toggleExpand('/p-a')
      expect(store.isExpanded('/p-a', { hasActive: false, isCurrent: false })).toBe(false)
    })

    // 有运行中 tab 的项目默认展开（即便未手动 toggle）
    it('Expand_DefaultActive_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/p-active')
      store.setTabPty(tabId, 'pty-1')  // status -> running
      expect(store.isExpanded('/p-active', { hasActive: true, isCurrent: false })).toBe(true)
    })

    // 当前项目默认展开
    it('Expand_DefaultCurrent_001', () => {
      const store = useSessionStore()
      expect(store.isExpanded('/p-cur', { hasActive: false, isCurrent: true })).toBe(true)
    })

    // 手动折叠覆盖「活的项目默认展开」
    it('Expand_ManualCollapseOverridesActive_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/p-active')
      store.setTabPty(tabId, 'pty-1')
      store.toggleExpand('/p-active')  // 手动折叠
      expect(store.isExpanded('/p-active', { hasActive: true, isCurrent: false })).toBe(false)
    })
  })

  // ==================== getHistoryFor（多项目历史） ====================
  describe('getHistoryFor', () => {
    // 两个项目各自的历史互不干扰（对抗审查 A 的核心）
    it('HistoryFor_MultiProject_001', async () => {
      const { getSessions } = await import('@/api/tauri')
      const mockGetSessions = getSessions as ReturnType<typeof vi.fn>

      // 项目 A 历史首次加载
      mockGetSessions.mockResolvedValueOnce([
        { sessionId: 'a-1', name: 'A1', projectPath: '/p-a', lastActiveAt: 1000 },
      ])
      await store_loadHistory('/p-a')

      // 项目 B 历史首次加载
      mockGetSessions.mockResolvedValueOnce([
        { sessionId: 'b-1', name: 'B1', projectPath: '/p-b', lastActiveAt: 2000 },
      ])
      await store_loadHistory('/p-b')

      const store = useSessionStore()
      expect(store.getHistoryFor('/p-a').map(s => s.sessionId)).toEqual(['a-1'])
      expect(store.getHistoryFor('/p-b').map(s => s.sessionId)).toEqual(['b-1'])
    })

    // tab 占用的 sessionId 在该项目历史中去重
    it('HistoryFor_DedupClaimed_001', async () => {
      const { getSessions } = await import('@/api/tauri')
      ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
        { sessionId: 'claimed-1', name: 'C', projectPath: '/p-a', lastActiveAt: 1000 },
        { sessionId: 'free-1', name: 'F', projectPath: '/p-a', lastActiveAt: 2000 },
      ])
      const store = useSessionStore()
      await store.loadHistorySessions('/p-a')
      store.createTab('/p-a', { sessionId: 'claimed-1' })  // 占用
      const ids = store.getHistoryFor('/p-a').map(s => s.sessionId)
      expect(ids).not.toContain('claimed-1')
      expect(ids).toContain('free-1')
    })
  })
})

// 辅助：loadHistorySessions 的薄封装，便于测试中复用 store 实例
async function store_loadHistory(path: string) {
  const store = useSessionStore()
  await store.loadHistorySessions(path)
}
