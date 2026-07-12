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
  getProjectsState: vi.fn().mockResolvedValue({ pinnedProjects: [], archivedSessions: {} }),
  updateProjectsState: vi.fn().mockResolvedValue(undefined),
}))

import { useSessionStore } from '@/stores/session'

describe('session store - 全局树', () => {
  beforeEach(() => setActivePinia(createPinia()))

  // ==================== isExpanded / toggleExpand（v3 纯手动） ====================
  describe('toggleExpand / isExpanded', () => {
    // 未 toggle 时默认折叠（v3 纯手动，无自动展开）
    it('Expand_DefaultCollapsed_001', () => {
      const store = useSessionStore()
      expect(store.isExpanded('/p-a')).toBe(false)
    })

    // toggle 后 isExpanded 反映手动展开状态；再 toggle 回折叠
    it('Expand_ToggleManual_001', () => {
      const store = useSessionStore()
      expect(store.isExpanded('/p-a')).toBe(false)
      store.toggleExpand('/p-a')
      expect(store.isExpanded('/p-a')).toBe(true)
      store.toggleExpand('/p-a')
      expect(store.isExpanded('/p-a')).toBe(false)
    })

    // 有运行中 tab 也不自动展开（v3 移除 hasActive 自动展开）
    it('Expand_NoAutoExpandOnActive_001', () => {
      const store = useSessionStore()
      const tabId = store.createTab('/p-active')
      store.setTabPty(tabId, 'pty-1')  // status -> running
      expect(store.isExpanded('/p-active')).toBe(false)
    })

    // 不同项目展开状态互不影响
    it('Expand_PerProject_001', () => {
      const store = useSessionStore()
      store.toggleExpand('/p-a')
      expect(store.isExpanded('/p-a')).toBe(true)
      expect(store.isExpanded('/p-b')).toBe(false)
    })
  })

  // ==================== getHistoryFor（多项目历史 + 过滤存档） ====================
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

    // 存档会话从历史列表过滤掉（v3 §5.1）
    it('HistoryFor_FilterArchived_001', async () => {
      const { getSessions } = await import('@/api/tauri')
      ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
        { sessionId: 'sess-keep', name: 'Keep', projectPath: '/p-a', lastActiveAt: 1000 },
        { sessionId: 'sess-archived', name: 'Archived', projectPath: '/p-a', lastActiveAt: 2000 },
      ])
      const store = useSessionStore()
      await store.loadHistorySessions('/p-a')
      await store.archiveSession('/p-a', 'sess-archived')
      const ids = store.getHistoryFor('/p-a').map(s => s.sessionId)
      expect(ids).toContain('sess-keep')
      expect(ids).not.toContain('sess-archived')
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

    // isPinned 由 buildProjectGroups 用 store.isPinned 填充（UI 置顶标记用；排序读 pinnedProjects）
    it('Group_IsPinnedPopulated_001', async () => {
      const store = useSessionStore()
      await store.pinProject('/p-a')
      const groups = store.buildProjectGroups(
        [{ path: '/p-a', name: 'a' }, { path: '/p-b', name: 'b' }],
      )
      expect(groups.find(g => g.projectPath === '/p-a')!.isPinned).toBe(true)
      expect(groups.find(g => g.projectPath === '/p-b')!.isPinned).toBe(false)
    })
  })

  // ==================== sortProjectGroups（v3：置顶 + 字母序 + 孤儿置底） ====================
  describe('sortProjectGroups', () => {
    // 置顶项目排在非置顶之前
    it('Sort_PinnedFirst_001', () => {
      const store = useSessionStore()
      store.pinnedProjects = ['/p-b']
      const groups = [
        { projectPath: '/p-a', name: 'alpha', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
        { projectPath: '/p-b', name: 'beta', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
      ]
      const sorted = store.sortProjectGroups(groups)
      expect(sorted[0].projectPath).toBe('/p-b')
      expect(sorted[1].projectPath).toBe('/p-a')
    })

    // 非置顶项目按字母序排列
    it('Sort_Alphabetical_001', () => {
      const store = useSessionStore()
      store.pinnedProjects = []
      const groups = [
        { projectPath: '/p-zeta', name: 'zeta', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
        { projectPath: '/p-alpha', name: 'alpha', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
        { projectPath: '/p-mid', name: 'mid', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
      ]
      const sorted = store.sortProjectGroups(groups)
      expect(sorted.map(g => g.projectPath)).toEqual(['/p-alpha', '/p-mid', '/p-zeta'])
    })

    // 孤儿项目置底（即便孤儿 name 字母序更靠前）
    it('Sort_OrphanLast_001', () => {
      const store = useSessionStore()
      store.pinnedProjects = []
      const groups = [
        { projectPath: '/orphan', name: 'aaa', tabs: [], runningCount: 1, pendingCount: 0, hasActive: true, isOrphan: true },
        { projectPath: '/normal', name: 'zzz', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
      ]
      const sorted = store.sortProjectGroups(groups)
      // 孤儿即使 name 更靠前也排到非孤儿之后
      expect(sorted[sorted.length - 1].projectPath).toBe('/orphan')
      expect(sorted[0].projectPath).toBe('/normal')
    })

    // 综合：置顶（字母序）-> 非孤儿（字母序）-> 孤儿置底
    it('Sort_PinnedAlphaOrphan_001', () => {
      const store = useSessionStore()
      store.pinnedProjects = ['/p-pin-a', '/p-pin-b']
      const groups = [
        { projectPath: '/p-orphan', name: 'orphan', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: true },
        { projectPath: '/p-normal', name: 'normal', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
        { projectPath: '/p-pin-a', name: 'pinA', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
        { projectPath: '/p-pin-b', name: 'pinB', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
      ]
      const sorted = store.sortProjectGroups(groups)
      expect(sorted.map(g => g.projectPath)).toEqual(['/p-pin-a', '/p-pin-b', '/p-normal', '/p-orphan'])
    })

    // 置顶匹配使用 normalized 比较（大小写/斜杠不敏感）
    it('Sort_PinnedNormalized_001', () => {
      const store = useSessionStore()
      store.pinnedProjects = ['C:\\Users\\proj']
      const groups = [
        { projectPath: 'c:/users/proj', name: 'proj', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
        { projectPath: '/other', name: 'other', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
      ]
      const sorted = store.sortProjectGroups(groups)
      expect(sorted[0].projectPath).toBe('c:/users/proj')
    })

    // 签名已移除 currentCwd 参数：v3 排序不依赖 cwd
    it('Sort_NoCwdParam_001', () => {
      const store = useSessionStore()
      const groups = [
        { projectPath: '/p-a', name: 'a', tabs: [], runningCount: 0, pendingCount: 0, hasActive: false, isOrphan: false },
      ]
      // 不传第二参数，应正常返回
      const sorted = store.sortProjectGroups(groups)
      expect(sorted).toHaveLength(1)
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

  // ==================== pin / archive（持久化） ====================
  describe('pin / archive（持久化）', () => {
    // pin 项目后 isPinned 返回 true，并调用 updateProjectsState 发完整 pinnedProjects
    it('Pin_MarkPinned_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockClear()
      const store = useSessionStore()
      await store.pinProject('/p-a')
      expect(store.isPinned('/p-a')).toBe(true)
      expect(mockUpdate).toHaveBeenCalledTimes(1)
      const arg = mockUpdate.mock.calls[0][0] as { pinnedProjects: string[] }
      expect(arg.pinnedProjects).toContain('/p-a')
    })

    // 重复 pin 同一项目幂等（不再调用 updateProjectsState）
    it('Pin_Idempotent_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockClear()
      const store = useSessionStore()
      await store.pinProject('/p-a')
      mockUpdate.mockClear()
      await store.pinProject('/p-a')
      expect(mockUpdate).not.toHaveBeenCalled()
    })

    // unpin 移除置顶（normalized 比较）
    it('Unpin_Removes_001', async () => {
      const store = useSessionStore()
      await store.pinProject('/p-a')
      expect(store.isPinned('/p-a')).toBe(true)
      await store.unpinProject('/p-a')
      expect(store.isPinned('/p-a')).toBe(false)
    })

    // unpin 不存在的项目幂等（不调用 updateProjectsState）
    it('Unpin_Idempotent_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockClear()
      const store = useSessionStore()
      await store.unpinProject('/not-pinned')
      expect(mockUpdate).not.toHaveBeenCalled()
    })

    // isPinned 使用 normalized 比较（Windows 路径大小写/斜杠不敏感）
    it('Pinned_Normalized_001', async () => {
      const store = useSessionStore()
      await store.pinProject('C:\\Users\\proj')
      expect(store.isPinned('c:/users/proj')).toBe(true)
    })

    // archive 会话后 getArchivedSessions 包含该 sessionId
    it('Archive_Adds_001', async () => {
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-1')
      expect(store.getArchivedSessions('/p-a')).toContain('sess-1')
    })

    // archive 同一会话幂等（不重复调用 updateProjectsState）
    it('Archive_Idempotent_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-1')
      mockUpdate.mockClear()
      await store.archiveSession('/p-a', 'sess-1')
      expect(mockUpdate).not.toHaveBeenCalled()
    })

    // restore 移除存档
    it('Restore_Removes_001', async () => {
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-1')
      await store.restoreSession('/p-a', 'sess-1')
      expect(store.getArchivedSessions('/p-a')).not.toContain('sess-1')
    })

    // restore 未存档会话幂等
    it('Restore_Idempotent_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockClear()
      const store = useSessionStore()
      await store.restoreSession('/p-a', 'not-archived')
      expect(mockUpdate).not.toHaveBeenCalled()
    })

    // archive 发送完整 archivedSessions map（顶层替换语义：多个项目并存）
    it('Archive_SendsFullMap_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockClear()
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-a1')
      await store.archiveSession('/p-b', 'sess-b1')
      // 最后一次写入应包含两个项目的存档（v3-1 顶层替换，须发完整）
      const lastCall = mockUpdate.mock.calls[mockUpdate.mock.calls.length - 1][0] as {
        archivedSessions: Record<string, string[]>
      }
      expect(Object.keys(lastCall.archivedSessions).sort()).toEqual(['/p-a', '/p-b'])
      expect(lastCall.archivedSessions['/p-a']).toContain('sess-a1')
      expect(lastCall.archivedSessions['/p-b']).toContain('sess-b1')
    })

    // restore 最后一个会话后空数组自动清理 key
    it('Restore_EmptyKeyCleaned_001', async () => {
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-1')
      await store.restoreSession('/p-a', 'sess-1')
      expect(store.getArchivedSessions('/p-a')).toEqual([])
    })

    // loadProjectsState 从后端加载填充 pinnedProjects + archivedSessions
    it('LoadProjectsState_Fills_001', async () => {
      const { getProjectsState } = await import('@/api/tauri')
      ;(getProjectsState as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: ['/p-loaded'],
        archivedSessions: { '/p-loaded': ['sess-archived'] },
      })
      const store = useSessionStore()
      await store.loadProjectsState()
      expect(store.isPinned('/p-loaded')).toBe(true)
      expect(store.getArchivedSessions('/p-loaded')).toContain('sess-archived')
    })

    // archived lookup 使用 normalized 比较（跨重启路径漂移容忍）
    it('Archive_NormalizedLookup_001', async () => {
      const store = useSessionStore()
      await store.archiveSession('C:\\Users\\proj', 'sess-1')
      expect(store.getArchivedSessions('c:/users/proj')).toContain('sess-1')
    })

    // archive 复用已有键（normalized 匹配），避免重复条目
    it('Archive_ReuseKey_001', async () => {
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-1')
      // 用不同大小写再 archive 同项目另一会话，应进同一键
      await store.archiveSession('/P-A', 'sess-2')
      const archived = store.getArchivedSessions('/p-a')
      expect(archived).toContain('sess-1')
      expect(archived).toContain('sess-2')
    })
  })
})

// 辅助：loadHistorySessions 的薄封装，便于测试中复用 store 实例
async function store_loadHistory(path: string) {
  const store = useSessionStore()
  await store.loadHistorySessions(path)
}
