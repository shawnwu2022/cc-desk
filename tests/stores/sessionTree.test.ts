import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
// @ts-expect-error - node:crypto 是 Node 内置模块，项目未安装 @types/node（与其它测试文件一致的 polyfill 模式）
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
import type { TerminalTab } from '@/stores/session'

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
      store.toggleExpand('/p-active', { hasActive: true, isCurrent: false })  // 手动折叠
      expect(store.isExpanded('/p-active', { hasActive: true, isCurrent: false })).toBe(false)
    })

    // 主路径：调用方传 opts 时切换行为正确
    it('Expand_ToggleWithOpts_001', () => {
      const store = useSessionStore()
      // hasActive=true，默认展开，toggle 后变为折叠
      expect(store.isExpanded('/p-a', { hasActive: true, isCurrent: false })).toBe(true)
      store.toggleExpand('/p-a', { hasActive: true, isCurrent: false })
      expect(store.isExpanded('/p-a', { hasActive: true, isCurrent: false })).toBe(false)
      store.toggleExpand('/p-a', { hasActive: true, isCurrent: false })
      expect(store.isExpanded('/p-a', { hasActive: true, isCurrent: false })).toBe(true)
    })

    // override 优先级高于 isCurrent
    it('Expand_OverridePriority_001', () => {
      const store = useSessionStore()
      // 先手动 override=true
      store.toggleExpand('/p-a', { hasActive: false, isCurrent: false })
      expect(store.isExpanded('/p-a', { hasActive: false, isCurrent: false })).toBe(true)
      // 即使 isCurrent=true，override 仍优先
      expect(store.isExpanded('/p-a', { hasActive: false, isCurrent: true })).toBe(true)
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

  // ==================== buildProjectGroups（分组 + 孤儿） ====================
  describe('buildProjectGroups', () => {
    it('Group_BasicCount_001', () => {
      const store = useSessionStore()
      const t1 = store.createTab('/p-a'); store.setTabPty(t1, 'pty-1')
      const t2 = store.createTab('/p-a'); store.setTabPty(t2, 'pty-2')
      const groups = store.buildProjectGroups(
        [{ path: '/p-a', name: 'a' }],
      )
      const a = groups.find(g => g.projectPath === '/p-a')!
      expect(a.tabs).toHaveLength(2)
      expect(a.runningCount).toBe(2)
      expect(a.hasActive).toBe(true)
      expect(a.isOrphan).toBe(false)
    })

    // pending 计入徽标（hasActive = running 或 pending）
    it('Group_PendingHasActive_001', () => {
      const store = useSessionStore()
      const t1 = store.createTab('/p-a'); store.setTabPty(t1, 'pty-1')
      store.tabs.get(t1)!.pending = true
      const groups = store.buildProjectGroups(
        [{ path: '/p-a', name: 'a' }],
      )
      expect(groups.find(g => g.projectPath === '/p-a')!.hasActive).toBe(true)
      expect(groups.find(g => g.projectPath === '/p-a')!.pendingCount).toBe(1)
    })

    // 孤儿：tab 的 projectPath 不在 cachedProjects（对抗审查 C）
    it('Group_OrphanTab_001', () => {
      const store = useSessionStore()
      const t = store.createTab('/tmp-not-saved'); store.setTabPty(t, 'pty-x')
      const groups = store.buildProjectGroups([])
      const orphan = groups.find(g => g.projectPath === '/tmp-not-saved')!
      expect(orphan).toBeTruthy()
      expect(orphan.isOrphan).toBe(true)
      expect(orphan.name).toBe('tmp-not-saved')
    })

    // 无 tab、无 cwd 命中的项目也出现（折叠态展示），runningCount=0
    it('Group_EmptyProjectShown_001', () => {
      const store = useSessionStore()
      const groups = store.buildProjectGroups(
        [{ path: '/p-empty', name: 'empty' }],
      )
      const e = groups.find(g => g.projectPath === '/p-empty')!
      expect(e.tabs).toHaveLength(0)
      expect(e.runningCount).toBe(0)
      expect(e.hasActive).toBe(false)
    })
  })

  // ==================== sortProjectGroups ====================
  describe('sortProjectGroups', () => {
    it('Sort_CurrentFirst_001', () => {
      const store = useSessionStore()
      const groups = [
        { projectPath: '/p-a', name: 'a', tabs: [], runningCount: 1, pendingCount: 0, hasActive: true, isOrphan: false },
        { projectPath: '/p-cur', name: 'cur', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
      ]
      const sorted = store.sortProjectGroups(groups, '/p-cur')
      expect(sorted[0].projectPath).toBe('/p-cur')
    })

    it('Sort_ActiveBeforeIdle_001', () => {
      const store = useSessionStore()
      const groups = [
        { projectPath: '/idle', name: 'idle', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
        { projectPath: '/active', name: 'active', tabs: [], runningCount: 1, pendingCount: 0, hasActive: true, isOrphan: false },
      ]
      const sorted = store.sortProjectGroups(groups, '/other')
      expect(sorted[0].projectPath).toBe('/active')
    })

    it('Sort_OrphanLast_001', () => {
      const store = useSessionStore()
      const groups = [
        { projectPath: '/orphan', name: 'o', tabs: [], runningCount: 1, pendingCount: 0, hasActive: true, isOrphan: true },
        { projectPath: '/idle', name: 'idle', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
      ]
      const sorted = store.sortProjectGroups(groups, '/other')
      // 孤儿即使 active 也排到非孤儿之后
      expect(sorted[sorted.length - 1].projectPath).toBe('/orphan')
    })

    // 第 4 档：current/orphan/hasActive 全持平，仅 tabs 最近活跃时间不同 → 降序
    it('Sort_LastActiveDesc_001', () => {
      const store = useSessionStore()
      const groups = [
        {
          projectPath: '/old',
          name: 'old',
          tabs: [{
            tabId: 'tab-old',
            projectPath: '/old',
            ptyId: null,
            sessionId: null,
            name: 'Old Session',
            status: 'stopped',
            createdAt: 1000,
            lastActiveAt: 100,
            working: false,
            pending: false,
            isResume: false,
          } as TerminalTab],
          runningCount: 0,
          pendingCount: 0,
          hasActive: false,
          isOrphan: false,
        },
        {
          projectPath: '/new',
          name: 'new',
          tabs: [{
            tabId: 'tab-new',
            projectPath: '/new',
            ptyId: null,
            sessionId: null,
            name: 'New Session',
            status: 'stopped',
            createdAt: 2000,
            lastActiveAt: 500,
            working: false,
            pending: false,
            isResume: false,
          } as TerminalTab],
          runningCount: 0,
          pendingCount: 0,
          hasActive: false,
          isOrphan: false,
        },
      ]
      const sorted = store.sortProjectGroups(groups, '/other')
      expect(sorted.map(g => g.projectPath)).toEqual(['/new', '/old'])
    })
  })

  // ==================== filterProjectGroups（搜索范围限制） ====================
  describe('filterProjectGroups', () => {
    it('Filter_MatchProjectName_001', () => {
      const store = useSessionStore()
      const groups = [
        { projectPath: '/alpha', name: 'alpha', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
        { projectPath: '/beta', name: 'beta', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
      ]
      const out = store.filterProjectGroups(groups, 'alp')
      expect(out.map(g => g.projectPath)).toEqual(['/alpha'])
    })

    // 空查询返回全部（无 matchedHistoryIds）
    it('Filter_EmptyQueryReturnsAll_001', () => {
      const store = useSessionStore()
      const groups = [
        { projectPath: '/a', name: 'a', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
      ]
      const out = store.filterProjectGroups(groups, '')
      expect(out).toHaveLength(1)
      expect(out[0].matchedHistoryIds).toBeUndefined()
    })

    // 命中会话名（仅当该会话在该组的 tabs 或已加载历史中）
    it('Filter_MatchSessionName_001', async () => {
      const { getSessions } = await import('@/api/tauri')
      ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
        { sessionId: 's-fix-bug', name: 'fix bug', projectPath: '/p-a', lastActiveAt: 1000 },
      ])
      const store = useSessionStore()
      await store.loadHistorySessions('/p-a')
      const groups = store.buildProjectGroups([{ path: '/p-a', name: 'a' }])
      const out = store.filterProjectGroups(groups, 'fix')
      expect(out).toHaveLength(1)
      expect(out[0].matchedHistoryIds).toEqual(['s-fix-bug'])
    })
  })
})

// 辅助：loadHistorySessions 的薄封装，便于测试中复用 store 实例
async function store_loadHistory(path: string) {
  const store = useSessionStore()
  await store.loadHistorySessions(path)
}
