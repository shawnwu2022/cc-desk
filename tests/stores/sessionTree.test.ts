import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { computed } from 'vue'
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
  // T6+8：5 个增量 command 各返最新 ProjectsState；getProjectsState 启动加载。
  // 默认返空状态，各测试用 mockResolvedValueOnce 覆盖为操作后状态（始终 invoke + 返回值覆盖本地）。
  getProjectsState: vi.fn().mockResolvedValue({ pinnedProjects: [], archivedSessions: {}, displayNames: {} }),
  pinProject: vi.fn().mockResolvedValue({ pinnedProjects: [], archivedSessions: {}, displayNames: {} }),
  unpinProject: vi.fn().mockResolvedValue({ pinnedProjects: [], archivedSessions: {}, displayNames: {} }),
  archiveSession: vi.fn().mockResolvedValue({ pinnedProjects: [], archivedSessions: {}, displayNames: {} }),
  restoreSession: vi.fn().mockResolvedValue({ pinnedProjects: [], archivedSessions: {}, displayNames: {} }),
  setDisplayName: vi.fn().mockResolvedValue({ pinnedProjects: [], archivedSessions: {}, displayNames: {} }),
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
      const { getSessions, archiveSession } = await import('@/api/tauri')
      ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
        { sessionId: 'sess-keep', name: 'Keep', projectPath: '/p-a', lastActiveAt: 1000 },
        { sessionId: 'sess-archived', name: 'Archived', projectPath: '/p-a', lastActiveAt: 2000 },
      ])
      // archive 返回值覆盖本地（新行为：始终 invoke + applyReturnedState，opLock 串行）
      ;(archiveSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: { '/p-a': ['sess-archived'] }, displayNames: {},
      })
      const store = useSessionStore()
      await store.loadHistorySessions('/p-a')
      await store.archiveSession('/p-a', 'sess-archived')
      const ids = store.getHistoryFor('/p-a').map(s => s.sessionId)
      expect(ids).toContain('sess-keep')
      expect(ids).not.toContain('sess-archived')
    })

    // 性能 #3：getHistoryFor memo（per projectPath computed）-- 依赖不变返回同引用，
    // 避免模板 v-for 内反复调返回新数组导致 ProjectNode/SessionItem 无谓重渲染。
    it('HistoryFor_MemoSameRef_001', async () => {
      const { getSessions } = await import('@/api/tauri')
      ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
        { sessionId: 'a-1', name: 'A1', projectPath: '/p-a', lastActiveAt: 1000 },
      ])
      const store = useSessionStore()
      await store.loadHistorySessions('/p-a')
      const h1 = store.getHistoryFor('/p-a')
      const h2 = store.getHistoryFor('/p-a')
      expect(h1).toBe(h2) // 同引用（memo：依赖未变）
    })

    // 性能 #3：tab.working 翻转不触发 history 重算（memo 核心：依赖不含 working）
    it('HistoryFor_MemoIgnoresTabWorking_001', async () => {
      const { getSessions } = await import('@/api/tauri')
      ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
        { sessionId: 'a-1', name: 'A1', projectPath: '/p-a', lastActiveAt: 1000 },
      ])
      const store = useSessionStore()
      await store.loadHistorySessions('/p-a')
      store.createTab('/p-a', { sessionId: 'run-1' })
      const h1 = store.getHistoryFor('/p-a')
      // tab.working 翻转（hook 驱动）-- 不应触发 history 重算
      const tab = [...store.tabs.values()].find(t => t.sessionId === 'run-1')
      if (tab) tab.working = true
      expect(store.getHistoryFor('/p-a')).toBe(h1) // 同引用（working 不在依赖链）
    })

    // 性能 #3：依赖真变化（historyCacheMap 重载）-> 新引用（失效正确）
    it('HistoryFor_MemoInvalidatesOnReload_001', async () => {
      const { getSessions } = await import('@/api/tauri')
      ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
        { sessionId: 'a-1', name: 'A1', projectPath: '/p-a', lastActiveAt: 1000 },
      ])
      const store = useSessionStore()
      await store.loadHistorySessions('/p-a')
      const h1 = store.getHistoryFor('/p-a')
      ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
        { sessionId: 'a-1', name: 'A1', projectPath: '/p-a', lastActiveAt: 1000 },
        { sessionId: 'a-2', name: 'A2', projectPath: '/p-a', lastActiveAt: 2000 },
      ])
      await store.loadHistorySessions('/p-a', true) // force 重载 -> historyCacheMap 变 -> computed 失效
      const h2 = store.getHistoryFor('/p-a')
      expect(h2).not.toBe(h1) // 依赖变，新引用
      expect(h2.map(s => s.sessionId)).toEqual(['a-2', 'a-1'])
    })

    // 性能 #3：不同 projectPath 独立缓存（引用互异，互不影响）
    it('HistoryFor_MemoPerProjectIndependent_001', async () => {
      const { getSessions } = await import('@/api/tauri')
      ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
        { sessionId: 'a-1', name: 'A1', projectPath: '/p-a', lastActiveAt: 1000 },
      ])
      ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
        { sessionId: 'b-1', name: 'B1', projectPath: '/p-b', lastActiveAt: 2000 },
      ])
      const store = useSessionStore()
      await store.loadHistorySessions('/p-a')
      await store.loadHistorySessions('/p-b')
      const ha = store.getHistoryFor('/p-a')
      const hb = store.getHistoryFor('/p-b')
      expect(ha).not.toBe(hb) // 不同项目不同引用
      expect(ha.map(s => s.sessionId)).toEqual(['a-1'])
      expect(hb.map(s => s.sessionId)).toEqual(['b-1'])
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
      const { pinProject } = await import('@/api/tauri')
      ;(pinProject as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: ['/p-a'], archivedSessions: {}, displayNames: {},
      })
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

  // ==================== buildProjectGroups / filterProjectGroups（displayName + 三字段搜索） ====================
  describe('buildProjectGroups displayName', () => {
    // 已收项目：name = getDisplayName（别名优先 basename）
    it('Group_DisplayName_Cached_001', () => {
      const store = useSessionStore()
      store.displayNames.set('/p-a', '别名A')
      const groups = store.buildProjectGroups([{ path: '/p-a', name: 'p-a' }])
      expect(groups[0].name).toBe('别名A')
    })

    // 孤儿项目（tab 有、cachedProjects 无）：name 同样用 getDisplayName
    it('Group_DisplayName_Orphan_001', () => {
      const store = useSessionStore()
      store.displayNames.set('/p-orphan', '孤儿别名')
      const tabId = store.createTab('/p-orphan')
      store.setTabPty(tabId, 'pty-1')
      const groups = store.buildProjectGroups([]) // cachedProjects 空 -> 该 tab 为孤儿
      const orphan = groups.find(g => g.projectPath === '/p-orphan')
      expect(orphan).toBeTruthy()
      expect(orphan!.name).toBe('孤儿别名')
      expect(orphan!.isOrphan).toBe(true)
    })
  })

  describe('filterProjectGroups 搜索三字段', () => {
    // 搜别名命中（别名后搜「客户的活」能找到）
    it('Filter_SearchDisplayName_001', () => {
      const store = useSessionStore()
      store.displayNames.set('/p-a', '客户的活')
      const groups = store.buildProjectGroups([{ path: '/p-a', name: 'p-a' }])
      const filtered = store.filterProjectGroups(groups, '客户')
      expect(filtered.length).toBe(1)
      expect(filtered[0].projectPath).toBe('/p-a')
    })

    // alias 设置时搜 basename 仍命中（codex #8/#16）：path='/work/c1' 别名='客户的活'，
    // 搜 'c1' -> basename 'c1' 命中（path 也含 c1，二者独立验证三字段）
    it('Filter_SearchBasenameWithAlias_001', () => {
      const store = useSessionStore()
      store.displayNames.set('/work/c1', '客户的活')
      const groups = store.buildProjectGroups([{ path: '/work/c1', name: 'c1' }])
      const filtered = store.filterProjectGroups(groups, 'c1')
      expect(filtered.length).toBe(1)
      expect(filtered[0].projectPath).toBe('/work/c1')
    })

    // 搜 path 命中（displayName/basename 不含，path 含）
    it('Filter_SearchPath_001', () => {
      const store = useSessionStore()
      store.displayNames.set('/work/secret-dir', '别名')
      const groups = store.buildProjectGroups([{ path: '/work/secret-dir', name: 'secret-dir' }])
      const filtered = store.filterProjectGroups(groups, 'secret')
      expect(filtered.length).toBe(1)
    })

    // 别名不匹配 + 无会话命中 -> 过滤掉
    it('Filter_NoMatch_001', () => {
      const store = useSessionStore()
      store.displayNames.set('/p-a', '客户的活')
      const groups = store.buildProjectGroups([{ path: '/p-a', name: 'p-a' }])
      const filtered = store.filterProjectGroups(groups, 'zzz')
      expect(filtered.length).toBe(0)
    })
  })

  // ==================== pin / archive（持久化） ====================
  describe('pin / archive（持久化）', () => {
    // pin 项目：始终 invoke pinProject，返回值覆盖本地（不再发完整快照、不本地短路）
    it('Pin_MarkPinned_001', async () => {
      const { pinProject } = await import('@/api/tauri')
      const mockPin = pinProject as ReturnType<typeof vi.fn>
      mockPin.mockResolvedValueOnce({ pinnedProjects: ['/p-a'], archivedSessions: {}, displayNames: {} })
      const store = useSessionStore()
      await store.pinProject('/p-a')
      expect(mockPin).toHaveBeenCalledWith('/p-a')      // 始终发（无本地短路）
      expect(store.isPinned('/p-a')).toBe(true)         // 返回值覆盖本地
    })

    // 重复 pin 同一项目仍 invoke（幂等由后端判定，前端不短路）
    it('Pin_Idempotent_001', async () => {
      const { pinProject } = await import('@/api/tauri')
      const mockPin = pinProject as ReturnType<typeof vi.fn>
      mockPin.mockClear()
      const store = useSessionStore()
      await store.pinProject('/p-a')
      await store.pinProject('/p-a')
      expect(mockPin).toHaveBeenCalledTimes(2)           // 始终发，不本地短路
    })

    // unpin 移除置顶：始终 invoke，返回值覆盖本地
    it('Unpin_Removes_001', async () => {
      const { pinProject, unpinProject } = await import('@/api/tauri')
      ;(pinProject as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: ['/p-a'], archivedSessions: {}, displayNames: {},
      })
      const store = useSessionStore()
      await store.pinProject('/p-a')
      expect(store.isPinned('/p-a')).toBe(true)
      await store.unpinProject('/p-a')                    // 默认 mock 返空 pinned
      expect(unpinProject as ReturnType<typeof vi.fn>).toHaveBeenCalledWith('/p-a')
      expect(store.isPinned('/p-a')).toBe(false)
    })

    // unpin 不存在的项目仍 invoke（幂等由后端判定，前端不短路）
    it('Unpin_Idempotent_001', async () => {
      const { unpinProject } = await import('@/api/tauri')
      const mockUnpin = unpinProject as ReturnType<typeof vi.fn>
      mockUnpin.mockClear()
      const store = useSessionStore()
      await store.unpinProject('/not-pinned')
      expect(mockUnpin).toHaveBeenCalledWith('/not-pinned')   // 始终发
      expect(mockUnpin).toHaveBeenCalledTimes(1)
    })

    // isPinned 使用 normalized 比较（后端 canonical 返回 + 前端 normalize 查询）
    it('Pinned_Normalized_001', async () => {
      const { pinProject } = await import('@/api/tauri')
      ;(pinProject as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: ['c:/users/proj'], archivedSessions: {}, displayNames: {},
      })
      const store = useSessionStore()
      await store.pinProject('C:\\Users\\proj')
      expect(store.isPinned('c:/users/proj')).toBe(true)
    })

    // archive 会话：始终 invoke，返回值覆盖本地
    it('Archive_Adds_001', async () => {
      const { archiveSession } = await import('@/api/tauri')
      ;(archiveSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: { '/p-a': ['sess-1'] }, displayNames: {},
      })
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-1')
      expect(archiveSession as ReturnType<typeof vi.fn>).toHaveBeenCalledWith('/p-a', 'sess-1')
      expect(store.getArchivedSessions('/p-a')).toContain('sess-1')
    })

    // archive 同一会话仍 invoke（幂等由后端判定，前端不短路）
    it('Archive_Idempotent_001', async () => {
      const { archiveSession } = await import('@/api/tauri')
      const mockArchive = archiveSession as ReturnType<typeof vi.fn>
      mockArchive.mockClear()
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-1')
      await store.archiveSession('/p-a', 'sess-1')
      expect(mockArchive).toHaveBeenCalledTimes(2)
    })

    // restore 移除存档：始终 invoke，返回值覆盖本地
    it('Restore_Removes_001', async () => {
      const { archiveSession, restoreSession } = await import('@/api/tauri')
      ;(archiveSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: { '/p-a': ['sess-1'] }, displayNames: {},
      })
      ;(restoreSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: {}, displayNames: {},
      })
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-1')
      await store.restoreSession('/p-a', 'sess-1')
      expect(restoreSession as ReturnType<typeof vi.fn>).toHaveBeenCalledWith('/p-a', 'sess-1')
      expect(store.getArchivedSessions('/p-a')).not.toContain('sess-1')
    })

    // restore 未存档会话仍 invoke（幂等由后端判定，前端不短路）
    it('Restore_Idempotent_001', async () => {
      const { restoreSession } = await import('@/api/tauri')
      const mockRestore = restoreSession as ReturnType<typeof vi.fn>
      mockRestore.mockClear()
      const store = useSessionStore()
      await store.restoreSession('/p-a', 'not-archived')
      expect(mockRestore).toHaveBeenCalledWith('/p-a', 'not-archived')
      expect(mockRestore).toHaveBeenCalledTimes(1)
    })

    // archive 多项目：后端串行归并，每次返回值覆盖本地 -> 多项目并存（顶层替换由后端 merge 完成）
    it('Archive_SendsFullMap_001', async () => {
      const { archiveSession } = await import('@/api/tauri')
      const mockArchive = archiveSession as ReturnType<typeof vi.fn>
      mockArchive.mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: { '/p-a': ['sess-a1'] }, displayNames: {},
      })
      mockArchive.mockResolvedValueOnce({
        pinnedProjects: [],
        archivedSessions: { '/p-a': ['sess-a1'], '/p-b': ['sess-b1'] },
        displayNames: {},
      })
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-a1')
      await store.archiveSession('/p-b', 'sess-b1')
      // 后端 merge 后最后一次返回值含两个项目（顶层替换语义由后端锁内完成）
      expect(store.getArchivedSessions('/p-a')).toContain('sess-a1')
      expect(store.getArchivedSessions('/p-b')).toContain('sess-b1')
    })

    // restore 最后一个会话后空 map -> 本地同步清空
    it('Restore_EmptyKeyCleaned_001', async () => {
      const { archiveSession, restoreSession } = await import('@/api/tauri')
      ;(archiveSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: { '/p-a': ['sess-1'] }, displayNames: {},
      })
      ;(restoreSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: {}, displayNames: {},
      })
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
      const { archiveSession } = await import('@/api/tauri')
      ;(archiveSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: { 'c:/users/proj': ['sess-1'] }, displayNames: {},
      })
      const store = useSessionStore()
      await store.archiveSession('C:\\Users\\proj', 'sess-1')
      expect(store.getArchivedSessions('c:/users/proj')).toContain('sess-1')
    })

    // archive 复用已有键（normalized 匹配）：后端归一归并，返回值含合并结果
    it('Archive_ReuseKey_001', async () => {
      const { archiveSession } = await import('@/api/tauri')
      const mockArchive = archiveSession as ReturnType<typeof vi.fn>
      mockArchive.mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: { '/p-a': ['sess-1'] }, displayNames: {},
      })
      mockArchive.mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: { '/p-a': ['sess-1', 'sess-2'] }, displayNames: {},
      })
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-1')
      // 用不同大小写再 archive 同项目另一会话，后端 normalized 归并进同一键
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
      const { getSessions, archiveSession } = await import('@/api/tauri')
      ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
        { sessionId: 'sess-named', name: '修复登录bug', projectPath: '/p-a', lastActiveAt: 5000 },
      ])
      ;(archiveSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: { '/p-a': ['sess-named'] }, displayNames: {},
      })
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
      const { archiveSession } = await import('@/api/tauri')
      ;(archiveSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: { '/p-a': ['abcdef1234567890'] }, displayNames: {},
      })
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

  // ==================== command 失败回滚（P1.3：始终 invoke 后端；reject 时本地未 apply） ====================
  describe('persist 失败回滚', () => {
    // pin command reject -> 抛错 + 本地未 apply（返回值未到达）
    it('Pin_PersistFail_NoLocalChange_001', async () => {
      const { pinProject } = await import('@/api/tauri')
      ;(pinProject as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('disk full'))
      const store = useSessionStore()
      await expect(store.pinProject('/p-a')).rejects.toThrow('disk full')
      expect(store.isPinned('/p-a')).toBe(false)
    })

    // unpin command reject -> 抛错 + 本地保持置顶（前一次 pin 返回值已 apply，unpin 返回值未到达）
    it('Unpin_PersistFail_KeepPinned_001', async () => {
      const { pinProject, unpinProject } = await import('@/api/tauri')
      ;(pinProject as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: ['/p-a'], archivedSessions: {}, displayNames: {},
      })
      ;(unpinProject as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('io error'))
      const store = useSessionStore()
      await store.pinProject('/p-a')  // 先成功置顶（返回值已 apply）
      await expect(store.unpinProject('/p-a')).rejects.toThrow('io error')
      expect(store.isPinned('/p-a')).toBe(true)
    })

    // archive command reject -> 抛错 + 本地存档不变（未 apply）
    it('Archive_PersistFail_NoLocalChange_001', async () => {
      const { archiveSession } = await import('@/api/tauri')
      ;(archiveSession as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('io error'))
      const store = useSessionStore()
      await expect(store.archiveSession('/p-a', 'sess-1')).rejects.toThrow('io error')
      expect(store.getArchivedSessions('/p-a')).toEqual([])
    })

    // restore command reject -> 抛错 + 本地仍保持存档（前一次 archive 已 apply）
    it('Restore_PersistFail_KeepArchived_001', async () => {
      const { archiveSession, restoreSession } = await import('@/api/tauri')
      ;(archiveSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: { '/p-a': ['sess-1'] }, displayNames: {},
      })
      ;(restoreSession as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('io error'))
      const store = useSessionStore()
      await store.archiveSession('/p-a', 'sess-1')  // 先成功存档
      await expect(store.restoreSession('/p-a', 'sess-1')).rejects.toThrow('io error')
      expect(store.getArchivedSessions('/p-a')).toContain('sess-1')
    })
  })

  // ==================== 启动加载门禁 + 并发操作锁（P1.2/P1.3） ====================
  describe('启动加载门禁 + 并发操作锁', () => {
    // 加载未完成时并发 pin：门禁使其等待加载完成，不得用空内存覆写磁盘已有置顶
    it('Pin_WaitsForLoad_NoOverwrite_001', async () => {
      const { getProjectsState, pinProject } = await import('@/api/tauri')
      const mockGet = getProjectsState as ReturnType<typeof vi.fn>
      // 模拟加载延迟：getProjectsState 不立即返回，磁盘已有置顶 '/p-pre'
      let resolveGet!: (v: { pinnedProjects: string[]; archivedSessions: Record<string, string[]> }) => void
      mockGet.mockReturnValueOnce(new Promise(r => { resolveGet = r }))
      // pin command 返回磁盘 '/p-pre' + 新 '/p-a'（后端锁内 merge）
      ;(pinProject as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: ['/p-pre', '/p-a'], archivedSessions: {}, displayNames: {},
      })
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
      // pin 在加载完成后执行：返回值含 '/p-pre' + '/p-a'（未用空内存覆写磁盘旧置顶）
      expect(store.isPinned('/p-pre')).toBe(true)
      expect(store.isPinned('/p-a')).toBe(true)
      expect(store.projectsStateLoaded).toBe(true)
    })

    // 加载失败后 pin 抛错不操作不发 command（门禁阻断，本地保持空）
    it('Pin_LoadFail_BlocksOp_001', async () => {
      const { getProjectsState, pinProject } = await import('@/api/tauri')
      ;(getProjectsState as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('read fail'))
      const mockPin = pinProject as ReturnType<typeof vi.fn>
      mockPin.mockClear()
      const store = useSessionStore()
      // 加载失败：ensureProjectsStateLoaded 抛错，pin command 未发
      await expect(store.pinProject('/p-a')).rejects.toThrow()
      expect(store.isPinned('/p-a')).toBe(false)
      expect(store.projectsStateLoaded).toBe(false)
      expect(mockPin).not.toHaveBeenCalled()
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

    // 并发 pin + archive（pin 先入锁）：操作锁串行化，返回值覆盖本地 -> 同时含两者
    it('Concurrent_PinArchive_NoLostUpdate_001', async () => {
      const { pinProject, archiveSession } = await import('@/api/tauri')
      ;(pinProject as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: ['/p-a'], archivedSessions: {}, displayNames: {},
      })
      // archive 后发：后端锁内看到 pin 结果，返回值含 pinned + archived
      ;(archiveSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: ['/p-a'], archivedSessions: { '/p-a': ['sess-1'] }, displayNames: {},
      })
      const store = useSessionStore()
      await Promise.all([
        store.pinProject('/p-a'),
        store.archiveSession('/p-a', 'sess-1'),
      ])
      // 本地最终同时含置顶与存档（最后一次 apply = archive 返回值，不丢任一项）
      expect(store.isPinned('/p-a')).toBe(true)
      expect(store.getArchivedSessions('/p-a')).toContain('sess-1')
    })

    // 并发 archive + pin（archive 先入锁）：反向顺序同样不丢更新
    it('Concurrent_ArchivePin_NoLostUpdate_001', async () => {
      const { archiveSession, pinProject } = await import('@/api/tauri')
      ;(archiveSession as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: { '/p-a': ['sess-1'] }, displayNames: {},
      })
      // pin 后发：后端锁内看到 archive 结果，返回值含 archived + pinned
      ;(pinProject as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: ['/p-a'], archivedSessions: { '/p-a': ['sess-1'] }, displayNames: {},
      })
      const store = useSessionStore()
      await Promise.all([
        store.archiveSession('/p-a', 'sess-1'),
        store.pinProject('/p-a'),
      ])
      expect(store.isPinned('/p-a')).toBe(true)
      expect(store.getArchivedSessions('/p-a')).toContain('sess-1')
    })

    // ensureProjectsStateLoaded 复用同一 loadPromise：并发 pin 不重复发 getProjectsState
    it('EnsureLoaded_ReusesLoadPromise_001', async () => {
      const { getProjectsState, pinProject } = await import('@/api/tauri')
      const mockGet = getProjectsState as ReturnType<typeof vi.fn>
      mockGet.mockClear()
      const mockPin = pinProject as ReturnType<typeof vi.fn>
      // 两次 pin 串行：第一次返 ['/p-a']，第二次返 ['/p-a','/p-b']（后端锁内累加）
      mockPin.mockResolvedValueOnce({ pinnedProjects: ['/p-a'], archivedSessions: {}, displayNames: {} })
      mockPin.mockResolvedValueOnce({ pinnedProjects: ['/p-a', '/p-b'], archivedSessions: {}, displayNames: {} })
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

    // action 的后端响应晚于 reload 时，reload 必须等待 action 完整应用后再读取，不能被旧 action 响应回滚。
    it('Reload_WaitsForActionRequestAndApply_001', async () => {
      const { getProjectsState, pinProject } = await import('@/api/tauri')
      const mockGet = getProjectsState as ReturnType<typeof vi.fn>
      const mockPin = pinProject as ReturnType<typeof vi.fn>
      const store = useSessionStore()
      await store.loadProjectsState()
      mockGet.mockClear()
      mockPin.mockClear()

      let resolvePin!: (value: { pinnedProjects: string[]; archivedSessions: Record<string, string[]>; displayNames: Record<string, string> }) => void
      mockPin.mockReturnValueOnce(new Promise(resolve => { resolvePin = resolve }))
      mockGet.mockResolvedValueOnce({
        pinnedProjects: ['/p-a', '/p-external'], archivedSessions: {}, displayNames: {},
      })

      const action = store.pinProject('/p-a')
      await vi.waitFor(() => expect(mockPin).toHaveBeenCalledTimes(1))
      const reload = store.reloadProjectsState()
      resolvePin({ pinnedProjects: ['/p-a'], archivedSessions: {}, displayNames: {} })
      await Promise.all([action, reload])

      expect(store.isPinned('/p-a')).toBe(true)
      expect(store.isPinned('/p-external')).toBe(true)
    })

    // 两次 reload 必须串行完整请求；旧读取先应用后，较新的读取仍应应用，不能仅因本地 seq 变化被丢弃。
    it('Reload_OverlappingRequestsApplyNewestRead_001', async () => {
      const { getProjectsState } = await import('@/api/tauri')
      const mockGet = getProjectsState as ReturnType<typeof vi.fn>
      const store = useSessionStore()
      await store.loadProjectsState()
      mockGet.mockClear()

      let resolveOld!: (value: { pinnedProjects: string[]; archivedSessions: Record<string, string[]>; displayNames: Record<string, string> }) => void
      let resolveNew!: (value: { pinnedProjects: string[]; archivedSessions: Record<string, string[]>; displayNames: Record<string, string> }) => void
      mockGet
        .mockReturnValueOnce(new Promise(resolve => { resolveOld = resolve }))
        .mockReturnValueOnce(new Promise(resolve => { resolveNew = resolve }))

      const first = store.reloadProjectsState()
      const second = store.reloadProjectsState()
      resolveOld({ pinnedProjects: ['/old'], archivedSessions: {}, displayNames: {} })
      await vi.waitFor(() => expect(mockGet).toHaveBeenCalledTimes(2))
      resolveNew({ pinnedProjects: ['/new'], archivedSessions: {}, displayNames: {} })
      await Promise.all([first, second])

      expect(store.isPinned('/new')).toBe(true)
      expect(store.isPinned('/old')).toBe(false)
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

    // setDisplayName：始终 invoke setDisplayName，返回值覆盖本地（displayNames map）
    it('SetDisplayName_PersistAndLocal_001', async () => {
      const { setDisplayName } = await import('@/api/tauri')
      const mock = setDisplayName as ReturnType<typeof vi.fn>
      mock.mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: {}, displayNames: { '/p-a': '主项目' },
      })
      const store = useSessionStore()
      await store.setDisplayName('/p-a', '主项目')
      expect(mock).toHaveBeenCalledWith('/p-a', '主项目')
      expect(store.getDisplayName('/p-a')).toBe('主项目')
    })

    // 并发 rename + pin 不丢：setDisplayName 与 pinProject 共享 withLock，串行化
    // 后端锁内两次操作归并，最后一次返回值（pin）含 alias + pinned
    it('SetDisplayName_ConcurrentWithPin_NoLoss_001', async () => {
      const { setDisplayName, pinProject } = await import('@/api/tauri')
      ;(setDisplayName as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: {}, displayNames: { '/p-a': '别名A' },
      })
      ;(pinProject as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: ['/p-a'], archivedSessions: {}, displayNames: { '/p-a': '别名A' },
      })
      const store = useSessionStore()
      await Promise.all([
        store.setDisplayName('/p-a', '别名A'),
        store.pinProject('/p-a'),
      ])
      expect(store.getDisplayName('/p-a')).toBe('别名A')
      expect(store.isPinned('/p-a')).toBe(true)
    })

    // setDisplayName command reject -> 抛错 + 本地未 apply（displayNames 不变）
    it('SetDisplayName_PersistFail_Rollback_001', async () => {
      const { setDisplayName } = await import('@/api/tauri')
      ;(setDisplayName as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('alias too long'))
      const store = useSessionStore()
      await expect(store.setDisplayName('/p-a', '别名')).rejects.toThrow('alias too long')
      expect(store.getDisplayName('/p-a')).toBe('p-a') // 本地未 apply
    })

    // 空/空白清除：setDisplayName('/p-a','') -> invoke，返回 displayNames 不含该 key（本地清）
    it('SetDisplayName_EmptyClears_001', async () => {
      const { setDisplayName } = await import('@/api/tauri')
      ;(setDisplayName as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: {}, displayNames: {},
      })
      const store = useSessionStore()
      store.displayNames.set('/p-a', '旧别名')
      await store.setDisplayName('/p-a', '')
      expect([...store.displayNames.keys()]).not.toContain('/p-a')
      expect(store.getDisplayName('/p-a')).toBe('p-a')
    })
    it('SetDisplayName_WhitespaceClears_001', async () => {
      const { setDisplayName } = await import('@/api/tauri')
      ;(setDisplayName as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: {}, displayNames: {},
      })
      const store = useSessionStore()
      store.displayNames.set('/p-a', '旧别名')
      await store.setDisplayName('/p-a', '   ')
      expect([...store.displayNames.keys()]).not.toContain('/p-a')
    })

    // 规范化等价 key（codex #8 断言值非只数量）：后端 canonical 返回单一 key，本地 apply 后值=新
    it('SetDisplayName_NormalizedEqKey_001', async () => {
      const { setDisplayName } = await import('@/api/tauri')
      ;(setDisplayName as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        pinnedProjects: [], archivedSessions: {}, displayNames: { 'e:/repo': '新' },
      })
      const store = useSessionStore()
      store.displayNames.set('e:/repo', '旧')
      await store.setDisplayName('E:\\Repo', '新')
      const keys = [...store.displayNames.keys()]
      expect(keys.length).toBe(1)                       // 不累积等价 key
      expect(store.getDisplayName('E:\\Repo')).toBe('新')  // 断言值=新（非只数量）
      expect(store.getDisplayName('e:/repo')).toBe('新')
    })

    // 校验失败不 invoke：超长别名前置校验抛错且不调 setDisplayName
    it('SetDisplayName_TooLong_NoPersist_001', async () => {
      const { setDisplayName } = await import('@/api/tauri')
      const mock = setDisplayName as ReturnType<typeof vi.fn>
      mock.mockClear()
      const store = useSessionStore()
      await expect(store.setDisplayName('/p-a', 'a'.repeat(33))).rejects.toThrow()
      expect(mock).not.toHaveBeenCalled()               // 前端 validateDisplayName 前置
    })

    // 校验失败（控制字符）不 invoke
    it('SetDisplayName_ControlChar_NoPersist_001', async () => {
      const { setDisplayName } = await import('@/api/tauri')
      const mock = setDisplayName as ReturnType<typeof vi.fn>
      mock.mockClear()
      const store = useSessionStore()
      await expect(store.setDisplayName('/p-a', 'a\nb')).rejects.toThrow()
      expect(mock).not.toHaveBeenCalled()               // 前端 validateDisplayName 前置
    })

    // load：后端 read_projects_state_locked 已 canonical，前端 applyReturnedState 原样入 Map；
    // 查询时 getDisplayName 对 query 做 normalizePath 命中规范 key
    it('LoadProjectsState_NormalizedKeys_001', async () => {
      const { getProjectsState } = await import('@/api/tauri')
      const mockGet = getProjectsState as ReturnType<typeof vi.fn>
      mockGet.mockResolvedValueOnce({
        pinnedProjects: [],
        archivedSessions: {},
        displayNames: { 'e:/repo': '别名A', '/p-b': '别名B' },  // 后端已 canonical
      })
      const store = useSessionStore()
      await store.loadProjectsState()
      // 查询端 normalizePath 命中（E:\REPO -> e:/repo）
      expect(store.getDisplayName('e:/repo')).toBe('别名A')
      expect(store.getDisplayName('E:\\REPO')).toBe('别名A')
      expect(store.getDisplayName('/p-b')).toBe('别名B')
    })

    // load 规范化冲突由后端解决：后端 dedup 后只返单一 canonical key，前端原样 apply（不再前端去重）
    it('LoadProjectsState_DuplicateNormalizedKey_001', async () => {
      const { getProjectsState } = await import('@/api/tauri')
      const mockGet = getProjectsState as ReturnType<typeof vi.fn>
      mockGet.mockResolvedValueOnce({
        pinnedProjects: [],
        archivedSessions: {},
        displayNames: { 'e:/repo': 'B' },   // 后端 dedup 后单一 canonical key
      })
      const store = useSessionStore()
      await store.loadProjectsState()
      const keys = [...store.displayNames.keys()]
      expect(keys.length).toBe(1)                          // 不累积等价 key
      expect(store.getDisplayName('e:/repo')).toBe('B')
      expect(store.getDisplayName('E:\\REPO')).toBe('B')   // 查询端 normalize 命中
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

// ==================== v6 codex batch2：缓存 key 规范化 + generation + 分页失败 ====================
describe('session store - codex batch2 缓存/状态一致性', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  // 历史缓存规范化 key：原始路径加载后，用规范化/等价路径访问命中同一缓存（不再重复拉取）
  it('HistoryCache_NormalizedKey_Hit_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    const mock = getSessions as ReturnType<typeof vi.fn>
    mock.mockResolvedValue([{ sessionId: 's1', name: 'S1', projectPath: 'E:\\Foo', lastActiveAt: 1 }])
    const store = useSessionStore()
    // 原始反斜杠路径加载
    await store.loadHistoryFor('E:\\Foo')
    expect(mock).toHaveBeenCalledTimes(1)
    // 等价正斜杠小写路径命中缓存（不再发请求）
    const res = await store.loadHistoryFor('e:/foo')
    expect(mock).toHaveBeenCalledTimes(1)
    expect(res.ok).toBe(true)
  })

  // getHistoryFor 规范化查找：原始路径加载，等价路径取到同一批历史
  it('HistoryCache_GetHistoryFor_NormalizedLookup_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    const mock = getSessions as ReturnType<typeof vi.fn>
    mock.mockResolvedValue([{ sessionId: 's1', name: 'S1', projectPath: 'E:\\Foo', lastActiveAt: 1 }])
    const store = useSessionStore()
    await store.loadHistoryFor('E:\\Foo')
    // 等价路径 getHistoryFor 命中
    const hist = store.getHistoryFor('e:/foo/')
    expect(hist.length).toBe(1)
    expect(hist[0].sessionId).toBe('s1')
    expect(mock).toHaveBeenCalledTimes(1)
  })

  // invalidateHistoryCache 规范化删除：等价路径 invalidate 后缓存清空
  it('HistoryCache_Invalidate_Normalized_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    const mock = getSessions as ReturnType<typeof vi.fn>
    mock.mockResolvedValue([{ sessionId: 's1', name: 'S1', projectPath: 'E:\\Foo', lastActiveAt: 1 }])
    const store = useSessionStore()
    await store.loadHistoryFor('E:\\Foo')
    expect(store.getHistoryFor('e:/foo').length).toBe(1)
    // 用等价路径 invalidate
    store.invalidateHistoryCache('e:/foo')
    // 缓存清空 -> 下次 force 或非 force 拉取重新请求
    mock.mockClear()
    await store.loadHistoryFor('E:\\Foo')
    expect(mock).toHaveBeenCalledTimes(1)
  })

  // generation：normal A 在途 + force B 排队，A resolve 不应把 loading 提前置 false（B 仍在拉）
  it('FetchHistory_Generation_OldNoFlipLoading_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    const resolvers: ((v: unknown[]) => void)[] = []
    const mock = getSessions as ReturnType<typeof vi.fn>
    // 每次调用登记一个 resolver，按顺序对应 A 的首页 / B 的追加刷新
    mock.mockImplementation(() => new Promise(r => { resolvers.push(r as (v: unknown[]) => void) }))
    const flush = () => new Promise<void>(r => setTimeout(r, 0))
    const store = useSessionStore()
    const pA = store.loadHistoryFor('/p-a')          // normal A（触发 getSessions #0）
    const pB = store.loadHistoryFor('/p-a', true)    // force B（排队等 A）
    await flush()
    expect(resolvers.length).toBeGreaterThanOrEqual(1)
    // A 首页 0 条（无分页）-> A settle；但 gen 已被 B 接管 -> A 不应把 loading 置 false
    resolvers[0]([])
    await flush()
    // B 的 queued startFetch 触发 getSessions #1，仍在途 -> loading 保持 true
    expect(store.historyLoadState.get('/p-a')?.loading).toBe(true)
    expect(resolvers.length).toBeGreaterThanOrEqual(2)
    resolvers[1]([])
    await Promise.all([pA, pB])
    expect(store.historyLoadState.get('/p-a')?.loading).toBe(false)
  })

  // 分页失败：首页成功有更多 -> 进入分页 -> 分页 reject -> isLoadingMore 复位 + 缓存清空（partial 不当完整）
  it('FetchHistory_PaginationFail_ClearPartialAndResetLoadingMore_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    const mock = getSessions as ReturnType<typeof vi.fn>
    const BATCH = 20
    // 首页返回 BATCH+1（触发 hasMore）；分页第二次 reject
    const firstBatch = Array.from({ length: BATCH + 1 }, (_, i) => ({ sessionId: `s${i}`, name: `S${i}`, projectPath: '/big', lastActiveAt: i }))
    mock.mockResolvedValueOnce(firstBatch)
    mock.mockRejectedValueOnce(new Error('pagination io'))
    const store = useSessionStore()
    const res = await store.loadHistoryFor('/big')
    expect(res.ok).toBe(false)                              // 分页失败向上传播
    expect(store.isLoadingMore).toBe(false)                 // finally 复位
    expect(store.getHistoryFor('/big').length).toBe(0)      // partial 缓存已清，不当完整
  })

  // 首页失败：不写缓存，error 置位
  it('FetchHistory_FirstPageFail_NoCache_ErrorSet_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    const mock = getSessions as ReturnType<typeof vi.fn>
    mock.mockRejectedValue(new Error('first page io'))
    const store = useSessionStore()
    const res = await store.loadHistoryFor('/p-a')
    expect(res.ok).toBe(false)
    expect(store.historyLoadState.get('/p-a')?.error).toBeTruthy()
    expect(store.getHistoryFor('/p-a').length).toBe(0)
  })

  // per-project loading：不同项目独立 loading 状态（互不干扰）
  it('FetchHistory_PerProjectLoading_Independent_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    let resolveA!: (v: unknown[]) => void
    let resolveB!: (v: unknown[]) => void
    const mock = getSessions as ReturnType<typeof vi.fn>
    mock.mockImplementationOnce(() => new Promise(r => { resolveA = r as (v: unknown[]) => void }))
    mock.mockImplementationOnce(() => new Promise(r => { resolveB = r as (v: unknown[]) => void }))
    const store = useSessionStore()
    const pA = store.loadHistoryFor('/p-a')
    const pB = store.loadHistoryFor('/p-b')
    expect(store.historyLoadState.get('/p-a')?.loading).toBe(true)
    expect(store.historyLoadState.get('/p-b')?.loading).toBe(true)
    resolveA([])
    await pA
    // A 完成、B 仍在拉 -> A loading=false, B loading=true
    expect(store.historyLoadState.get('/p-a')?.loading).toBe(false)
    expect(store.historyLoadState.get('/p-b')?.loading).toBe(true)
    resolveB([])
    await pB
    expect(store.historyLoadState.get('/p-b')?.loading).toBe(false)
  })

  // loadHistorySessions 切 currentHistoryProject 为规范化 key
  it('LoadHistorySessions_CurrentNormalized_001', async () => {
    const { getSessions } = await import('@/api/tauri')
    ;(getSessions as ReturnType<typeof vi.fn>).mockResolvedValue([])
    const store = useSessionStore()
    await store.loadHistorySessions('E:\\Foo\\')
    // currentHistoryProject 存规范化（Windows：lower + 去尾斜杠 + 斜杠规范）
    expect(store.currentHistoryProject).toBe('e:/foo')
  })

  // projects.json 加载：后端 read_projects_state_locked 已 canonical（等价路径去重），前端原样 apply
  it('LoadProjectsState_PinnedNormalizeDedupe_001', async () => {
    const { getProjectsState } = await import('@/api/tauri')
    ;(getProjectsState as ReturnType<typeof vi.fn>).mockResolvedValue({
      pinnedProjects: ['e:/foo', '/p-a'],   // 后端 canonical 已去重等价路径
      archivedSessions: {}, displayNames: {},
    })
    const store = useSessionStore()
    await store.loadProjectsState()
    const pinned = [...store.pinnedProjects]
    expect(pinned.filter(p => p === 'e:/foo')).toHaveLength(1)
    expect(pinned).toHaveLength(2)  // e:/foo + /p-a
  })

  // projects.json 加载：后端 canonical 已按规范化路径归并 sessionId 去重，前端原样 apply；查询端 normalize 命中
  it('LoadProjectsState_ArchivedMergeByNormalized_001', async () => {
    const { getProjectsState } = await import('@/api/tauri')
    ;(getProjectsState as ReturnType<typeof vi.fn>).mockResolvedValue({
      pinnedProjects: [],
      archivedSessions: { 'e:/foo': ['s1', 's2', 's3'] },  // 后端 canonical 已归并去重
      displayNames: {},
    })
    const store = useSessionStore()
    await store.loadProjectsState()
    expect([...store.archivedSessions.keys()].filter(k => k === 'e:/foo')).toHaveLength(1)
    const ids = store.getArchivedSessions('E:\\Foo')   // 查询端 normalizePath 命中 e:/foo
    expect(ids.sort()).toEqual(['s1', 's2', 's3'])
  })
})
