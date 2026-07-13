import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { computed } from 'vue'
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

// 平台确定性（codex 重要#3）：强制 Windows，使大小写不敏感测试在任意宿主全绿
vi.mock('@/utils/platform', () => ({
  detectPlatform: () => 'windows',
  platform: 'windows',
  isMac: false,
  isWindows: true,
  ctrl: 'Ctrl',
  alt: 'Alt',
  cmd: 'Ctrl',
  getClaudePlatformKey: () => 'win32-x64',
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

  // ==================== getArchivedSessionInfos（name/lastActiveAt 查 historyCacheMap） ====================
  describe('getArchivedSessionInfos', () => {
    // historyCacheMap 命中：返回会话名与 lastActiveAt（区分存档会话）
    it('ArchivedInfos_HitReturnsName_001', async () => {
      const { getSessions } = await import('@/api/tauri')
      ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
        { sessionId: 'sess-named', name: '修复登录bug', projectPath: '/p-a', lastActiveAt: 5000 },
      ])
      const store = useSessionStore()
      await store.loadHistorySessions('/p-a')
      await store.archiveSession('/p-a', 'sess-named')
      const infos = store.getArchivedSessionInfos('/p-a')
      expect(infos).toHaveLength(1)
      expect(infos[0].sessionId).toBe('sess-named')
      expect(infos[0].name).toBe('修复登录bug')
      expect(infos[0].lastActiveAt).toBe(5000)
    })

    // historyCacheMap 未加载：name 回退 ID 截断（前 8 位）、lastActiveAt 回退 0
    it('ArchivedInfos_MissFallbackId_001', async () => {
      const store = useSessionStore()
      // 未 loadHistorySessions -> historyCacheMap 无该项目
      await store.archiveSession('/p-a', 'abcdef1234567890')
      const infos = store.getArchivedSessionInfos('/p-a')
      expect(infos).toHaveLength(1)
      expect(infos[0].sessionId).toBe('abcdef1234567890')
      expect(infos[0].name).toBe('abcdef12')  // ID.slice(0,8) 截断
      expect(infos[0].lastActiveAt).toBe(0)
    })
  })

  // ==================== 持久化失败回滚（P1.3：先 persist 新状态再改本地，失败不改本地 + 抛错） ====================
  describe('persist 失败回滚', () => {
    // pin 持久化失败时本地 pinnedProjects 不变且抛错
    it('Pin_PersistFail_NoLocalChange_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockRejectedValueOnce(new Error('disk full'))
      const store = useSessionStore()
      await expect(store.pinProject('/p-a')).rejects.toThrow('disk full')
      expect(store.isPinned('/p-a')).toBe(false)
    })

    // unpin 持久化失败时本地仍保持置顶
    it('Unpin_PersistFail_KeepPinned_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      const store = useSessionStore()
      await store.pinProject('/p-a')  // 先成功置顶
      mockUpdate.mockRejectedValueOnce(new Error('io error'))
      await expect(store.unpinProject('/p-a')).rejects.toThrow('io error')
      expect(store.isPinned('/p-a')).toBe(true)
    })

    // archive 持久化失败时本地存档不变且抛错
    it('Archive_PersistFail_NoLocalChange_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockRejectedValueOnce(new Error('io error'))
      const store = useSessionStore()
      await expect(store.archiveSession('/p-a', 'sess-1')).rejects.toThrow('io error')
      expect(store.getArchivedSessions('/p-a')).toEqual([])
    })

    // restore 持久化失败时本地仍保持存档
    it('Restore_PersistFail_KeepArchived_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-1')  // 先成功存档
      mockUpdate.mockRejectedValueOnce(new Error('io error'))
      await expect(store.restoreSession('/p-a', 'sess-1')).rejects.toThrow('io error')
      expect(store.getArchivedSessions('/p-a')).toContain('sess-1')
    })
  })

  // ==================== 启动加载门禁 + 并发操作锁（P1.2/P1.3） ====================
  describe('启动加载门禁 + 并发操作锁', () => {
    // 加载未完成时并发 pin：门禁使其等待加载完成，不得用空内存覆写磁盘已有置顶
    it('Pin_WaitsForLoad_NoOverwrite_001', async () => {
      const { getProjectsState } = await import('@/api/tauri')
      const mockGet = getProjectsState as ReturnType<typeof vi.fn>
      // 模拟加载延迟：getProjectsState 不立即返回，磁盘已有置顶 '/p-pre'
      let resolveGet!: (v: { pinnedProjects: string[]; archivedSessions: Record<string, string[]> }) => void
      mockGet.mockReturnValueOnce(new Promise(r => { resolveGet = r }))
      const store = useSessionStore()
      // 启动 fire-and-forget 加载（未完成）
      store.loadProjectsState()
      // 加载未完成时并发 pin：门禁应使其等待，不得用空内存覆写磁盘 '/p-pre'
      const pinP = store.pinProject('/p-a')
      await new Promise(r => setTimeout(r, 0))
      expect(store.isPinned('/p-a')).toBe(false)   // 尚未写入
      expect(store.isPinned('/p-pre')).toBe(false) // 加载未完成，本地仍空
      // 完成加载：磁盘 '/p-pre' 读入本地
      resolveGet({ pinnedProjects: ['/p-pre'], archivedSessions: {} })
      await pinP
      // pin 在加载完成后执行：保留 '/p-pre' 并追加 '/p-a'（未用空内存覆写）
      expect(store.isPinned('/p-pre')).toBe(true)
      expect(store.isPinned('/p-a')).toBe(true)
      expect(store.projectsStateLoaded).toBe(true)
    })

    // 加载失败后 pin 抛错不操作不持久化（门禁阻断，本地保持空）
    it('Pin_LoadFail_BlocksOp_001', async () => {
      const { getProjectsState, updateProjectsState } = await import('@/api/tauri')
      ;(getProjectsState as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('read fail'))
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockClear()
      const store = useSessionStore()
      // 加载失败：ensureProjectsStateLoaded 抛错，pin 不操作不持久化
      await expect(store.pinProject('/p-a')).rejects.toThrow()
      expect(store.isPinned('/p-a')).toBe(false)
      expect(store.projectsStateLoaded).toBe(false)
      expect(mockUpdate).not.toHaveBeenCalled()
    })

    // P2：加载失败后 loadPromise 重置为 null，第二次 ensureProjectsStateLoaded 触发重试（第二次成功）
    it('LoadFail_Retry_001', async () => {
      const { getProjectsState } = await import('@/api/tauri')
      const mockGet = getProjectsState as ReturnType<typeof vi.fn>
      // 第一次加载失败
      mockGet.mockRejectedValueOnce(new Error('read fail'))
      // 第二次加载成功
      mockGet.mockResolvedValueOnce({
        pinnedProjects: ['/p-retry'],
        archivedSessions: { '/p-retry': ['sess-arch'] },
      })
      const store = useSessionStore()
      // 第一次 ensureProjectsStateLoaded 抛错（加载失败，门禁阻断）
      await expect(store.ensureProjectsStateLoaded()).rejects.toThrow()
      expect(store.projectsStateLoaded).toBe(false)
      expect(store.projectsStateError).toBe(true)
      // 第二次 ensureProjectsStateLoaded 触发重试（loadPromise 已重置为 null）
      await store.ensureProjectsStateLoaded()
      expect(store.projectsStateLoaded).toBe(true)
      expect(store.projectsStateError).toBe(false)
      expect(store.isPinned('/p-retry')).toBe(true)
      expect(store.getArchivedSessions('/p-retry')).toContain('sess-arch')
    })

    // 并发 pin + archive（pin 先入锁）：操作锁串行化，磁盘最终同时含两者（不丢更新）
    it('Concurrent_PinArchive_NoLostUpdate_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockClear()
      const store = useSessionStore()
      await Promise.all([
        store.pinProject('/p-a'),
        store.archiveSession('/p-a', 'sess-1'),
      ])
      // 本地最终同时含置顶与存档
      expect(store.isPinned('/p-a')).toBe(true)
      expect(store.getArchivedSessions('/p-a')).toContain('sess-1')
      // 最后一次 persist 须含两项（顶层替换语义下后操作基于前操作结果，不丢任一项）
      const lastCall = mockUpdate.mock.calls[mockUpdate.mock.calls.length - 1][0] as {
        pinnedProjects: string[]
        archivedSessions: Record<string, string[]>
      }
      expect(lastCall.pinnedProjects).toContain('/p-a')
      expect(lastCall.archivedSessions['/p-a']).toContain('sess-1')
    })

    // 并发 archive + pin（archive 先入锁）：反向顺序同样不丢更新
    it('Concurrent_ArchivePin_NoLostUpdate_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockClear()
      const store = useSessionStore()
      await Promise.all([
        store.archiveSession('/p-a', 'sess-1'),
        store.pinProject('/p-a'),
      ])
      expect(store.isPinned('/p-a')).toBe(true)
      expect(store.getArchivedSessions('/p-a')).toContain('sess-1')
      const lastCall = mockUpdate.mock.calls[mockUpdate.mock.calls.length - 1][0] as {
        pinnedProjects: string[]
        archivedSessions: Record<string, string[]>
      }
      expect(lastCall.pinnedProjects).toContain('/p-a')
      expect(lastCall.archivedSessions['/p-a']).toContain('sess-1')
    })

    // ensureProjectsStateLoaded 复用同一 loadPromise：并发 pin 不重复发 getProjectsState
    it('EnsureLoaded_ReusesLoadPromise_001', async () => {
      const { getProjectsState } = await import('@/api/tauri')
      const mockGet = getProjectsState as ReturnType<typeof vi.fn>
      mockGet.mockClear()
      const store = useSessionStore()
      await Promise.all([
        store.pinProject('/p-a'),
        store.pinProject('/p-b'),
      ])
      // 两个并发 pin 共用首次加载：getProjectsState 只调一次
      expect(mockGet).toHaveBeenCalledTimes(1)
      expect(store.isPinned('/p-a')).toBe(true)
      expect(store.isPinned('/p-b')).toBe(true)
    })
  })

  // ==================== displayNames / getDisplayName / setDisplayName ====================
  describe('displayNames', () => {
    // getDisplayName：有别名返别名，无则 basename 回退（去尾斜杠 + 反斜杠规范）
    it('GetDisplayName_AliasOrBasename_001', () => {
      const store = useSessionStore()
      store.displayNames.set('/p-a', '主项目')
      expect(store.getDisplayName('/p-a')).toBe('主项目')
      expect(store.getDisplayName('/p-b')).toBe('p-b')        // basename 回退
      expect(store.getDisplayName('/work/app/')).toBe('app')  // 去尾斜杠 basename
      expect(store.getDisplayName('C:\\repo\\x\\')).toBe('x') // 反斜杠 + 尾斜杠
    })

    // getDisplayName 响应式（codex #10）：displayNames Map 变化 -> 依赖 getDisplayName 的 computed 重算
    // 证明 native title watcher 的依赖（watch(getDisplayName(cwd))）会触发
    it('GetDisplayName_Reactive_001', () => {
      const store = useSessionStore()
      const cwd = '/p-reactive'
      const c = computed(() => store.getDisplayName(cwd))
      expect(c.value).toBe('p-reactive')
      store.displayNames.set('/p-reactive', '别名')
      expect(c.value).toBe('别名')  // reactive Map -> computed 重算
      store.displayNames.delete('/p-reactive')
      expect(c.value).toBe('p-reactive')  // 删别名 -> 回退 basename
    })

    // setDisplayName：成功持久化并写入本地 Map（含规范化 key）+ 发完整三份
    it('SetDisplayName_PersistAndLocal_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockClear()
      const store = useSessionStore()
      await store.setDisplayName('/p-a', '主项目')
      expect(store.getDisplayName('/p-a')).toBe('主项目')
      expect(mockUpdate).toHaveBeenCalledTimes(1)
      const payload = mockUpdate.mock.calls[0][0] as Record<string, unknown>
      expect(payload.pinnedProjects).toEqual([])
      expect(payload.archivedSessions).toEqual({})
      expect(payload.displayNames).toEqual({ '/p-a': '主项目' })
    })

    // 并发 rename + pin 不丢：setDisplayName 与 pinProject 共享 withLock，串行化
    // 最终磁盘（updateProjectsState 最后一次调用）含 alias + pinned
    it('SetDisplayName_ConcurrentWithPin_NoLoss_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockClear()
      const store = useSessionStore()
      await Promise.all([
        store.setDisplayName('/p-a', '别名A'),
        store.pinProject('/p-a'),
      ])
      expect(store.getDisplayName('/p-a')).toBe('别名A')
      expect(store.isPinned('/p-a')).toBe(true)
      // 串行化保证：最后一次写入时本地已含另一项 -> 不丢
      const payloads = mockUpdate.mock.calls.map(c => c[0] as Record<string, unknown>)
      const last = payloads[payloads.length - 1]
      expect(last.displayNames).toEqual({ '/p-a': '别名A' })
      expect(last.pinnedProjects).toEqual(['/p-a'])
    })

    // persist-first 失败回滚：updateProjectsState reject -> 本地 displayNames 不变 + 抛错
    it('SetDisplayName_PersistFail_Rollback_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockRejectedValueOnce(new Error('disk full'))
      const store = useSessionStore()
      await expect(store.setDisplayName('/p-a', '别名')).rejects.toThrow('disk full')
      expect(store.getDisplayName('/p-a')).toBe('p-a') // 本地未改
    })

    // 空/空白清除：setDisplayName('/p-a','') -> 删 key（恢复 basename）
    it('SetDisplayName_EmptyClears_001', async () => {
      const store = useSessionStore()
      store.displayNames.set('/p-a', '旧别名')
      await store.setDisplayName('/p-a', '')
      expect([...store.displayNames.keys()]).not.toContain('/p-a')
      expect(store.getDisplayName('/p-a')).toBe('p-a')
    })
    it('SetDisplayName_WhitespaceClears_001', async () => {
      const store = useSessionStore()
      store.displayNames.set('/p-a', '旧别名')
      await store.setDisplayName('/p-a', '   ')
      expect([...store.displayNames.keys()]).not.toContain('/p-a')
    })

    // 规范化等价 key（codex #8 断言值非只数量）：set 前删等价旧 key。
    // Windows 平台下 E:\Repo 与 e:/repo 规范化等价 -> 旧 key 删，新规范化 key 写入，值=新
    it('SetDisplayName_NormalizedEqKey_001', async () => {
      const store = useSessionStore()
      store.displayNames.set('e:/repo', '旧')
      await store.setDisplayName('E:\\Repo', '新')
      const keys = [...store.displayNames.keys()]
      expect(keys.length).toBe(1)                       // 不累积等价 key
      expect(store.getDisplayName('E:\\Repo')).toBe('新')  // 断言值=新（非只数量）
      expect(store.getDisplayName('e:/repo')).toBe('新')
    })

    // 校验失败不 persist：超长别名抛错且不调 updateProjectsState
    it('SetDisplayName_TooLong_NoPersist_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockClear()
      const store = useSessionStore()
      await expect(store.setDisplayName('/p-a', 'a'.repeat(33))).rejects.toThrow()
      expect(mockUpdate).not.toHaveBeenCalled()
    })

    // 校验失败（控制字符）不 persist
    it('SetDisplayName_ControlChar_NoPersist_001', async () => {
      const { updateProjectsState } = await import('@/api/tauri')
      const mockUpdate = updateProjectsState as ReturnType<typeof vi.fn>
      mockUpdate.mockClear()
      const store = useSessionStore()
      await expect(store.setDisplayName('/p-a', 'a\nb')).rejects.toThrow()
      expect(mockUpdate).not.toHaveBeenCalled()
    })

    // load 规范化 key：getProjectsState 返回混合斜杠/大小写 key -> 规范化入 Map
    it('LoadProjectsState_NormalizedKeys_001', async () => {
      const { getProjectsState } = await import('@/api/tauri')
      const mockGet = getProjectsState as ReturnType<typeof vi.fn>
      mockGet.mockResolvedValueOnce({
        pinnedProjects: [],
        archivedSessions: {},
        displayNames: { 'E:\\Repo': '别名A', '/p-b': '别名B' },
      })
      const store = useSessionStore()
      await store.loadProjectsState()
      // Windows 规范化后查询命中（E:\Repo -> e:/repo）
      expect(store.getDisplayName('e:/repo')).toBe('别名A')
      expect(store.getDisplayName('E:\\REPO')).toBe('别名A')
      expect(store.getDisplayName('/p-b')).toBe('别名B')
    })

    // load 规范化冲突（codex #8 断言后者值）：同规范化 key 多条目 -> 清空旧 Map 再填，后者覆盖
    it('LoadProjectsState_DuplicateNormalizedKey_001', async () => {
      const { getProjectsState } = await import('@/api/tauri')
      const mockGet = getProjectsState as ReturnType<typeof vi.fn>
      mockGet.mockResolvedValueOnce({
        pinnedProjects: [],
        archivedSessions: {},
        displayNames: { 'E:\\Repo': 'A', 'e:/repo': 'B' }, // 规范化等价 -> 后者 B 覆盖
      })
      const store = useSessionStore()
      await store.loadProjectsState()
      const keys = [...store.displayNames.keys()]
      expect(keys.length).toBe(1)                          // 不累积等价 key
      expect(store.getDisplayName('e:/repo')).toBe('B')    // 断言后者值 B（非只数量）
    })

    // load 清空旧 Map：先 set 一条本地，load 后旧本地条目被清（以磁盘为准）
    it('LoadProjectsState_ClearsStaleLocal_001', async () => {
      const { getProjectsState } = await import('@/api/tauri')
      const mockGet = getProjectsState as ReturnType<typeof vi.fn>
      mockGet.mockResolvedValueOnce({
        pinnedProjects: [],
        archivedSessions: {},
        displayNames: { '/p-new': '新' },
      })
      const store = useSessionStore()
      store.displayNames.set('/p-stale', '旧本地')
      await store.loadProjectsState()
      expect([...store.displayNames.keys()]).not.toContain('/p-stale')
      expect(store.getDisplayName('/p-new')).toBe('新')
    })
  })
})

// 辅助：loadHistorySessions 的薄封装，便于测试中复用 store 实例
async function store_loadHistory(path: string) {
  const store = useSessionStore()
  await store.loadHistorySessions(path)
}

// ==================== v5-T4：fetchHistory 两层 force 状态机 ====================
describe('session store - fetchHistory 两层 force', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  // 普通 + inflight 复用：同项目并发 loadHistoryFor -> getSessions 调 1 次
  it('FetchHistory_ReuseInflight_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    const mock = getSessions as ReturnType<typeof vi.fn>
    mock.mockResolvedValue([])
    const store = useSessionStore()
    await Promise.all([store.loadHistoryFor('/p-a'), store.loadHistoryFor('/p-a')])
    expect(mock).toHaveBeenCalledTimes(1)
  })

  // force + inflight：等当前后追加一次刷新 -> getSessions 调 2 次
  it('FetchHistory_ForceAfterInflight_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    const mock = getSessions as ReturnType<typeof vi.fn>
    mock.mockResolvedValueOnce([{ sessionId: 's1', name: 'S1', projectPath: '/p-a', lastActiveAt: 1 }])
    mock.mockResolvedValueOnce([{ sessionId: 's2', name: 'S2', projectPath: '/p-a', lastActiveAt: 2 }])
    const store = useSessionStore()
    await Promise.all([
      store.loadHistoryFor('/p-a'),          // 普通发起 inflight
      store.loadHistoryFor('/p-a', true),     // force 等当前后追加
    ])
    expect(mock).toHaveBeenCalledTimes(2)
  })

  // 多 force 并发合并：3 个 force -> getSessions 调最多 2 次
  it('FetchHistory_MultiForceMerge_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    const mock = getSessions as ReturnType<typeof vi.fn>
    mock.mockResolvedValue([])
    const store = useSessionStore()
    await Promise.all([
      store.loadHistoryFor('/p-a', true),
      store.loadHistoryFor('/p-a', true),
      store.loadHistoryFor('/p-a', true),
    ])
    expect(mock.mock.calls.length).toBeLessThanOrEqual(2)
  })

  // force 无 inflight：直接发 -> getSessions 调 1 次
  it('FetchHistory_ForceNoInflight_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    const mock = getSessions as ReturnType<typeof vi.fn>
    mock.mockResolvedValue([])
    const store = useSessionStore()
    await store.loadHistoryFor('/p-a', true)
    expect(mock).toHaveBeenCalledTimes(1)
  })

  // loadHistoryFor 不改 currentHistoryProject（无副作用）
  it('LoadHistoryFor_NoCurrentChange_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValue([])
    const store = useSessionStore()
    const before = store.currentHistoryProject
    await store.loadHistoryFor('/p-a')
    expect(store.currentHistoryProject).toBe(before)
  })

  // loadHistorySessions 仍切 currentHistoryProject
  it('LoadHistorySessions_ChangesCurrent_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValue([])
    const store = useSessionStore()
    await store.loadHistorySessions('/p-a')
    expect(store.currentHistoryProject).toBe('/p-a')
  })

  // historyLoadState 反映 per 项目 loading
  it('FetchHistory_LoadStatePerProject_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    let resolveFn!: (v: unknown[]) => void
    ;(getSessions as ReturnType<typeof vi.fn>).mockReturnValue(new Promise(r => { resolveFn = r as (v: unknown[]) => void }))
    const store = useSessionStore()
    const p = store.loadHistoryFor('/p-a')
    expect(store.historyLoadState.get('/p-a')?.loading).toBe(true)
    resolveFn!([])
    await p
    expect(store.historyLoadState.get('/p-a')?.loading).toBe(false)
  })
})

// ==================== v5-T4：buildProjectGroups 过滤 hidden ====================
describe('session store - buildProjectGroups 过滤 hidden', () => {
  beforeEach(() => setActivePinia(createPinia()))

  // hidden 项目不在分组
  it('Group_FilterHidden_001', () => {
    const store = useSessionStore()
    const groups = store.buildProjectGroups(
      [{ path: '/p-a', name: 'a' }, { path: '/p-b', name: 'b' }],
      new Set(['/p-a']),
    )
    expect(groups.map(g => g.projectPath)).toEqual(['/p-b'])
  })

  // hidden 项目的 running tab 也不作孤儿出现
  it('Group_FilterHiddenTab_001', () => {
    const store = useSessionStore()
    const tabId = store.createTab('/p-hidden')
    store.setTabPty(tabId, 'pty-1') // running tab
    const groups = store.buildProjectGroups(
      [{ path: '/p-vis', name: 'vis' }],
      new Set(['/p-hidden']),
    )
    expect(groups.map(g => g.projectPath)).toEqual(['/p-vis']) // 不含 /p-hidden 孤儿
  })

  // 不传 hidden（undefined）= 不过滤（向后兼容现有调用/测试）
  it('Group_NoHiddenParam_NoFilter_001', () => {
    const store = useSessionStore()
    const groups = store.buildProjectGroups([{ path: '/p-a', name: 'a' }])
    expect(groups.map(g => g.projectPath)).toEqual(['/p-a'])
  })

  // 规范化比较：hidden 用正斜杠小写仍隐藏反斜杠大写项目
  it('Group_HiddenNormalized_001', () => {
    const store = useSessionStore()
    const groups = store.buildProjectGroups(
      [{ path: 'E:\\Source\\Foo', name: 'Foo' }],
      new Set(['e:/source/foo']),
    )
    expect(groups).toHaveLength(0)
  })
})

// ==================== v5-T4：removeTab 不刷历史 ====================
describe('session store - removeTab 不刷历史', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  // removeTab 删 tab 不触发 loadHistorySessions（getSessions 未调）
  it('RemoveTab_NoHistoryRefresh_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    const mock = getSessions as ReturnType<typeof vi.fn>
    mock.mockResolvedValue([])
    const store = useSessionStore()
    const tabId = store.createTab('/p-a')
    store.removeTab(tabId)
    expect(store.tabs.has(tabId)).toBe(false)
    expect(mock).not.toHaveBeenCalled()
  })
})
