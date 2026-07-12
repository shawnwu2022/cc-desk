import { defineStore } from 'pinia'
import { ref, computed, reactive } from 'vue'
import {
  getProjectsState,
  getSessions,
  ptyKill,
  searchSessionMessages,
  updateProjectsState,
} from '@/api/tauri'
import type { SessionSearchResult } from '@/types'

// ==================== Tab-Centric 数据模型 ====================

/**
 * 终端 Tab — 跨越 PTY 生命周期的稳定 UI 单元
 *
 * - Tab 创建后一直存在，直到用户主动关闭
 * - PTY 退出后 status 变为 stopped，但 Tab 保留
 * - 有 sessionId 的 stopped Tab 可以 --resume 重启
 * - 无 sessionId 的 stopped Tab 重启时作为新会话
 */
export interface TerminalTab {
  tabId: string
  projectPath: string
  ptyId: string | null
  sessionId: string | null
  name: string
  status: 'starting' | 'running' | 'stopped'
  createdAt: number
  lastActiveAt: number
  working: boolean           // 正在工作中（用户发送消息后、响应返回前）
  model?: string
  pending: boolean           // 需要用户关注
  isResume: boolean          // 是否为恢复历史会话（创建时已有 sessionId）
}

/**
 * 历史会话（从 Claude 原生数据读取，未被 Tab 占用）
 */
export interface HistorySession {
  sessionId: string
  name: string
  projectPath: string
  lastActiveAt: number
}

/**
 * 全局树的项目分组
 */
export interface ProjectGroup {
  projectPath: string
  name: string
  tabs: TerminalTab[]
  runningCount: number
  pendingCount: number
  hasActive: boolean          // 有 running 或 pending tab
  isOrphan: boolean           // projectPath 不在 cachedProjects
  isPinned?: boolean          // 是否置顶（buildProjectGroups 填充，UI 展示用；排序读 pinnedProjects）
  matchedHistoryIds?: string[]  // 搜索命中的会话 ID（供 UI 临时展开 + 高亮）
}

/** 路径归一化：统一为小写正斜杠，用于跨平台/跨重启匹配置顶与存档 */
const normalizePath = (p: string) => p.replace(/\\/g, '/').toLowerCase()

// ==================== Store ====================

export const useSessionStore = defineStore('session', () => {
  // ---- State ----
  const tabs = reactive(new Map<string, TerminalTab>())
  const activeTabId = ref<string | null>(null)
  /** 项目级历史会话缓存 Map */
  const historyCacheMap = reactive(new Map<string, HistorySession[]>())
  /** 当前展示历史会话的项目路径 */
  const currentHistoryProject = ref<string>('')
  /** 正在加载的项目路径（去重用） */
  const inflightLoadProject = ref<string | null>(null)
  const searchQuery = ref<string>('')
  const isLoading = ref<boolean>(false)
  const isLoadingMore = ref<boolean>(false)
  const messageSearchResults = ref<SessionSearchResult[]>([])
  let messageSearchTimer: ReturnType<typeof setTimeout> | null = null

  /** 置顶项目（持久化到 projects.json，启动加载） */
  const pinnedProjects = ref<string[]>([])
  /** 会话存档：projectPath -> sessionId[]（持久化到 projects.json，启动加载） */
  const archivedSessions = reactive(new Map<string, string[]>())

  // ---- Computed ----

  const activeTab = computed<TerminalTab | null>(() => {
    if (!activeTabId.value) return null
    return tabs.get(activeTabId.value) ?? null
  })

  /** 当前项目的 Tab 列表 */
  const projectTabs = computed<TerminalTab[]>(() => {
    // 不在这里过滤，因为 projectPath 需要外部传入
    return [...tabs.values()]
  })

  /** 运行中的 Tab ID 列表 */
  const runningTabIds = computed<string[]>(() => {
    return [...tabs.values()]
      .filter(t => t.status === 'running')
      .map(t => t.tabId)
  })

  /** 已被 Tab 占用的 sessionId 集合 */
  const claimedSessionIds = computed<Set<string>>(() => {
    const ids = new Set<string>()
    for (const tab of tabs.values()) {
      if (tab.sessionId) ids.add(tab.sessionId)
    }
    return ids
  })

  /** 未被 Tab 占用的历史会话（去重 + 过滤） */
  const historySessions = computed<HistorySession[]>(() => {
    const cached = historyCacheMap.get(currentHistoryProject.value) ?? []
    const claimed = claimedSessionIds.value
    const seen = new Set<string>()
    return cached.filter(s => {
      if (claimed.has(s.sessionId) || seen.has(s.sessionId)) return false
      seen.add(s.sessionId)
      return true
    })
  })

  /**
   * 取指定项目的历史会话（去重 + 不含已被 tab 占用的 sessionId + 过滤存档）。
   * 替代单值 historySessions 作为全局树的数据源（对抗审查 A）。
   * v3 §5.1：在去重基础上过滤 archivedSessions.get(projectPath) 的 sessionId。
   */
  function getHistoryFor(projectPath: string): HistorySession[] {
    const cached = historyCacheMap.get(projectPath) ?? []
    const claimed = claimedSessionIds.value
    const archived = new Set(getArchivedSessions(projectPath))
    const seen = new Set<string>()
    return cached
      .filter(s => {
        if (claimed.has(s.sessionId) || seen.has(s.sessionId)) return false
        if (archived.has(s.sessionId)) return false
        seen.add(s.sessionId)
        return true
      })
      .sort((a, b) => b.lastActiveAt - a.lastActiveAt)
  }

  // ---- Tab 生命周期 ----

  /**
   * 创建新 Tab
   * @returns tabId
   */
  function createTab(projectPath: string, opts?: {
    sessionId?: string
    name?: string
  }): string {
    const tabId = crypto.randomUUID()
    const now = Date.now()

    tabs.set(tabId, {
      tabId,
      projectPath,
      ptyId: null,
      sessionId: opts?.sessionId ?? null,
      name: opts?.name ?? (opts?.sessionId ? opts.sessionId.slice(0, 8) : 'New Session'),
      status: 'stopped',
      createdAt: now,
      lastActiveAt: now,
      working: false,
      pending: false,
      isResume: !!opts?.sessionId,
    })

    return tabId
  }

  /**
   * 为 Tab 启动 PTY
   * 调用者需要在拿到 ptyId 后传入
   */
  function setTabPty(tabId: string, ptyId: string) {
    const tab = tabs.get(tabId)
    if (!tab) return
    tab.ptyId = ptyId
    tab.status = 'running'
    tab.lastActiveAt = Date.now()
  }

  /**
   * 处理 PTY 退出
   * 只更新状态，不删除 Tab
   */
  function handlePtyExit(ptyId: string) {
    for (const tab of tabs.values()) {
      if (tab.ptyId === ptyId) {
        tab.ptyId = null
        tab.status = 'stopped'
        tab.working = false
        tab.lastActiveAt = Date.now()
        break
      }
    }
  }

  /**
   * 关闭 Tab（用户主动操作）
   * 先删除 tab 让 UI 立即响应，再异步 kill PTY
   */
  async function closeTab(tabId: string) {
    const tab = tabs.get(tabId)
    if (!tab) return

    const projectPath = tab.projectPath
    const ptyId = tab.ptyId

    // 先删除 tab，UI 立即更新（历史列表通过 computed 自动显示释放的 sessionId）
    tabs.delete(tabId)

    if (activeTabId.value === tabId) {
      // 关闭当前活跃标签后，聚焦到同项目的相邻标签
      const remaining = getProjectTabs(projectPath)
      if (remaining.length > 0) {
        activeTabId.value = remaining[0].tabId
      } else {
        activeTabId.value = null
      }
    }

    // 异步 kill PTY（不阻塞 UI）
    if (ptyId) {
      ptyKill(ptyId).catch(() => {})
    }

    // 关闭后刷新历史会话：新会话的 JSONL 在运行期间已创建，force 拉取让被关闭的
    // 会话立即出现在历史列表，无需手动刷新（新建时刷新因时序过早未必能读到）
    await loadHistorySessions(projectPath, true)
  }

  /**
   * 关闭指定项目的所有 Tab
   */
  async function closeAllTabs(projectPath: string) {
    const projectTabList = getProjectTabs(projectPath)
    const ptyIds = projectTabList
      .map(t => t.ptyId)
      .filter((id): id is string => !!id)

    tabs.forEach((tab, tabId) => {
      if (tab.projectPath === projectPath) {
        tabs.delete(tabId)
      }
    })

    if (activeTabId.value && !tabs.has(activeTabId.value)) {
      activeTabId.value = null
    }

    for (const ptyId of ptyIds) {
      ptyKill(ptyId).catch(() => {})
    }

    // 关闭后刷新历史会话（同 closeTab）
    await loadHistorySessions(projectPath, true)
  }

  /**
   * 关闭除指定 Tab 外的所有同项目 Tab
   */
  async function closeOtherTabs(keepTabId: string) {
    const keepTab = tabs.get(keepTabId)
    if (!keepTab) return

    const projectPath = keepTab.projectPath
    const projectTabList = getProjectTabs(projectPath)
    const ptyIds: string[] = []

    for (const tab of projectTabList) {
      if (tab.tabId !== keepTabId) {
        if (tab.ptyId) ptyIds.push(tab.ptyId)
        tabs.delete(tab.tabId)
      }
    }

    if (activeTabId.value && !tabs.has(activeTabId.value)) {
      activeTabId.value = keepTabId
    }

    for (const ptyId of ptyIds) {
      ptyKill(ptyId).catch(() => {})
    }

    // 关闭后刷新历史会话（同 closeTab）
    await loadHistorySessions(projectPath, true)
  }

  /**
   * 更新 Tab 名称
   */
  function updateTabName(tabId: string, name: string) {
    const tab = tabs.get(tabId)
    if (tab) {
      tab.name = name
    }
  }

  /**
   * 更新 Tab 的 sessionId（匹配后赋值）
   */
  function setTabSessionId(tabId: string, sessionId: string, name?: string) {
    const tab = tabs.get(tabId)
    if (!tab) return
    tab.sessionId = sessionId
    if (name) tab.name = name
    tab.lastActiveAt = Date.now()
  }

  // ---- 活跃 Tab 管理 ----

  function setActiveTab(tabId: string | null) {
    if (tabId) {
      const tab = tabs.get(tabId)
      if (tab) tab.pending = false
    }
    activeTabId.value = tabId
  }

  function getRunningTabForProject(projectPath: string): TerminalTab | null {
    for (const tab of tabs.values()) {
      if (tab.projectPath === projectPath && tab.status === 'running') {
        return tab
      }
    }
    return null
  }

  function getTabByPtyId(ptyId: string): TerminalTab | null {
    for (const tab of tabs.values()) {
      if (tab.ptyId === ptyId) return tab
    }
    return null
  }

  function getTabBySessionId(sessionId: string): TerminalTab | null {
    for (const tab of tabs.values()) {
      if (tab.sessionId === sessionId) return tab
    }
    return null
  }

  // ---- Session ID 直接分配（由 hook 事件驱动） ----

  function assignSessionIdByPtyId(ptyId: string, sessionId: string, model?: string) {
    for (const tab of tabs.values()) {
      if (tab.ptyId === ptyId) {
        tab.sessionId = sessionId
        if (model) tab.model = model
        tab.lastActiveAt = Date.now()
        return true
      }
    }
    return false
  }

  const hasPendingTabs = computed(() => {
    for (const tab of tabs.values()) {
      if (tab.pending) return true
    }
    return false
  })

  // ---- 历史会话 ----

  const BATCH_SIZE = 20

  /**
   * 加载历史会话（项目级缓存 + 去重 + 不提前清空）
   */
  async function loadHistorySessions(projectPath: string, force = false) {
    // 切换当前展示项目（即使还在加载中也立即切换，让 computed 指向正确项目）
    currentHistoryProject.value = projectPath

    // 缓存命中且非强制刷新，直接返回
    if (!force && historyCacheMap.has(projectPath)) return
    // 去重：已在加载同一项目
    if (inflightLoadProject.value === projectPath) return

    inflightLoadProject.value = projectPath
    isLoading.value = true
    isLoadingMore.value = false

    try {
      // 请求 BATCH_SIZE + 1 条来判断是否有更多
      const firstBatch = await getSessions(projectPath, BATCH_SIZE + 1, 0)
      const hasMore = firstBatch.length > BATCH_SIZE
      const firstPage = hasMore ? firstBatch.slice(0, BATCH_SIZE) : firstBatch

      const mapped = firstPage
        .map(s => ({
          sessionId: s.sessionId,
          name: s.name,
          projectPath: s.projectPath,
          lastActiveAt: s.lastActiveAt,
        }))
        .sort((a, b) => b.lastActiveAt - a.lastActiveAt)

      // 数据就绪后才写入缓存（不提前清空）
      historyCacheMap.set(projectPath, mapped)
      isLoading.value = false

      if (hasMore) {
        isLoadingMore.value = true
        let offset = BATCH_SIZE
        let allSessions = [...mapped]
        while (true) {
          const batch = await getSessions(projectPath, BATCH_SIZE, offset)
          if (batch.length === 0) break
          const more = batch
            .map(s => ({
              sessionId: s.sessionId,
              name: s.name,
              projectPath: s.projectPath,
              lastActiveAt: s.lastActiveAt,
            }))
          allSessions = [...allSessions, ...more]
            .sort((a, b) => b.lastActiveAt - a.lastActiveAt)
          offset += batch.length
          if (batch.length < BATCH_SIZE) break
        }
        historyCacheMap.set(projectPath, allSessions)
        isLoadingMore.value = false
      }
    } catch (err) {
      console.error('[SessionStore] loadHistorySessions failed:', err)
      isLoading.value = false
      isLoadingMore.value = false
    } finally {
      inflightLoadProject.value = null
    }
  }

  /** 清除指定项目或全部历史缓存 */
  function invalidateHistoryCache(projectPath?: string) {
    if (projectPath) {
      historyCacheMap.delete(projectPath)
    } else {
      historyCacheMap.clear()
    }
  }

  // ---- 搜索 ----

  function setSearchQuery(query: string) {
    searchQuery.value = query
  }

  function debouncedSearchMessages(projectPath: string, query: string) {
    if (messageSearchTimer) clearTimeout(messageSearchTimer)
    if (!query || query.length < 2) {
      messageSearchResults.value = []
      return
    }
    messageSearchTimer = setTimeout(async () => {
      try {
        messageSearchResults.value = await searchSessionMessages(projectPath, query, 10)
      } catch (err) {
        console.error('[SessionStore] searchMessages failed:', err)
        messageSearchResults.value = []
      }
    }, 400)
  }

  // ---- 全局树：展开状态 ----

  /**
   * 显式展开/折叠覆盖：true=展开，false=折叠，缺省=折叠。
   * v3 §4.2：纯手动，无默认展开（移除 isCurrent/hasActive 自动展开）。
   */
  const expandOverride = reactive(new Map<string, boolean>())

  /** 切换展开/折叠（纯手动，无默认展开） */
  function toggleExpand(projectPath: string) {
    const cur = expandOverride.get(projectPath) ?? false
    expandOverride.set(projectPath, !cur)
  }

  /**
   * 判断项目是否展开（纯手动：只看 expandOverride，未 toggle 过则 false）。
   * v3 移除 isCurrent/hasActive 自动展开，展开状态完全由用户控制。
   */
  function isExpanded(projectPath: string): boolean {
    return expandOverride.get(projectPath) ?? false
  }

  // ---- 清理 ----

  async function cleanupAll() {
    const ids = [...tabs.values()]
      .filter(t => t.ptyId)
      .map(t => t.ptyId!)

    for (const id of ids) {
      try { await ptyKill(id) } catch {}
    }
    tabs.clear()
    historyCacheMap.clear()
    currentHistoryProject.value = ''
    inflightLoadProject.value = null
  }

  function getProjectTabs(projectPath: string): TerminalTab[] {
    return [...tabs.values()]
      .filter(t => t.projectPath === projectPath)
      .sort((a, b) => b.lastActiveAt - a.lastActiveAt)
  }

  /**
   * 构建项目分组（含孤儿）。
   * @param cachedProjects 来自 appStore.cachedProjects 的项目列表
   */
  function buildProjectGroups(
    cachedProjects: { path: string; name: string }[],
  ): ProjectGroup[] {
    const normalize = (p: string) => p.replace(/\\/g, '/').toLowerCase()
    const known = new Set(cachedProjects.map(p => normalize(p.path)))

    // 按 normalized path 聚合 tabs，避免 Windows 路径大小写/斜杠不一致时
    // 精确匹配（getProjectTabs）漏 tab：known 判非孤儿却贴不到 tab。
    const tabsByNorm = new Map<string, { tabs: TerminalTab[]; firstRaw: string }>()
    for (const tab of tabs.values()) {
      const n = normalize(tab.projectPath)
      const entry = tabsByNorm.get(n)
      if (entry) entry.tabs.push(tab)
      else tabsByNorm.set(n, { tabs: [tab], firstRaw: tab.projectPath })
    }
    // 同一项目内按 lastActiveAt 倒序，保持与 getProjectTabs 一致
    for (const entry of tabsByNorm.values()) {
      entry.tabs.sort((a, b) => b.lastActiveAt - a.lastActiveAt)
    }

    const groups: ProjectGroup[] = []
    for (const p of cachedProjects) {
      const projTabs = tabsByNorm.get(normalize(p.path))?.tabs ?? []
      groups.push(makeGroup(p.path, p.name, projTabs, false))
    }
    // 孤儿：tabs 中有但 cachedProjects 没有（Map 已按 normalized key 去重）
    for (const [n, entry] of tabsByNorm) {
      if (known.has(n)) continue
      const parts = entry.firstRaw.replace(/\\/g, '/').split('/')
      const name = parts[parts.length - 1] || entry.firstRaw
      groups.push(makeGroup(entry.firstRaw, name, entry.tabs, true))
    }
    return groups

    function makeGroup(projectPath: string, name: string, projTabs: TerminalTab[], isOrphan: boolean): ProjectGroup {
      let runningCount = 0, pendingCount = 0
      for (const t of projTabs) {
        if (t.status === 'running') runningCount++
        if (t.pending) pendingCount++
      }
      return {
        projectPath, name, tabs: projTabs,
        runningCount, pendingCount,
        hasActive: runningCount > 0 || pendingCount > 0,
        isOrphan,
        isPinned: isPinned(projectPath),
      }
    }
  }

  /**
   * 项目分组排序（v3 §4.6）：置顶项目优先 -> 非孤儿字母序 -> 孤儿置底。
   * 移除 v2 的当前项目置顶 / hasActive / lastActiveAt 档位。
   */
  function sortProjectGroups(groups: ProjectGroup[]): ProjectGroup[] {
    const pinnedSet = new Set(pinnedProjects.value.map(normalizePath))
    return [...groups].sort((a, b) => {
      const aPinned = pinnedSet.has(normalizePath(a.projectPath)) ? 1 : 0
      const bPinned = pinnedSet.has(normalizePath(b.projectPath)) ? 1 : 0
      if (aPinned !== bPinned) return bPinned - aPinned                // 置顶优先
      if (a.isOrphan !== b.isOrphan) return a.isOrphan ? 1 : -1        // 孤儿置底
      return a.name.toLowerCase().localeCompare(b.name.toLowerCase())  // 字母序
    })
  }

  /**
   * 搜索过滤（对抗审查 B 的范围限制）：
   * - 项目名：全量匹配
   * - 会话名：仅匹配已加载历史（getHistoryFor）+ 该组 tabs 的 name/sessionId
   * 命中会话的分组带 matchedHistoryIds（供 UI 临时展开 + 高亮）。
   */
  function filterProjectGroups(groups: ProjectGroup[], query: string): ProjectGroup[] {
    const q = query.trim().toLowerCase()
    if (!q) return groups
    return groups
      .map((g): ProjectGroup | null => {
        const matchProject = g.name.toLowerCase().includes(q)
        const tabHits = g.tabs
          .filter(t => t.name.toLowerCase().includes(q) || (t.sessionId?.toLowerCase().includes(q) ?? false))
          .map(t => t.sessionId)
          .filter((id): id is string => !!id)
        const historyHits = getHistoryFor(g.projectPath)
          .filter(s => s.name.toLowerCase().includes(q) || s.sessionId.toLowerCase().includes(q))
          .map(s => s.sessionId)
        const matchedHistoryIds = [...new Set([...tabHits, ...historyHits])]
        if (matchProject) return { ...g, matchedHistoryIds }   // 项目名命中：整个组保留
        if (matchedHistoryIds.length > 0) return { ...g, matchedHistoryIds }
        return null
      })
      .filter((g): g is ProjectGroup => g !== null)
  }

  // ---- 项目置顶 + 会话存档（持久化到 ~/.cc-box/projects.json）----

  /**
   * 启动加载：读取 pinnedProjects + archivedSessions（参考 loadAppConfig 范式）。
   * v3-1 后端 merge 为顶层替换，故写入时须发送完整 map。
   */
  async function loadProjectsState() {
    try {
      const state = await getProjectsState()
      pinnedProjects.value = state.pinnedProjects ?? []
      archivedSessions.clear()
      for (const [k, v] of Object.entries(state.archivedSessions ?? {})) {
        archivedSessions.set(k, v)
      }
    } catch (err) {
      console.error('[SessionStore] loadProjectsState failed:', err)
    }
  }

  /** 发送完整 pinnedProjects + archivedSessions（顶层替换语义，v3-1 merge 非深合并） */
  async function persistProjectsState() {
    const archivedObj: Record<string, string[]> = {}
    for (const [k, v] of archivedSessions.entries()) {
      archivedObj[k] = v
    }
    await updateProjectsState({
      pinnedProjects: [...pinnedProjects.value],
      archivedSessions: archivedObj,
    })
  }

  /** 该项目是否已置顶（normalized 比较，兼容 Windows 路径大小写/斜杠差异） */
  function isPinned(path: string): boolean {
    const n = normalizePath(path)
    return pinnedProjects.value.some(p => normalizePath(p) === n)
  }

  /** 置顶项目（已存在则幂等返回，不发持久化） */
  async function pinProject(path: string) {
    if (isPinned(path)) return
    pinnedProjects.value = [...pinnedProjects.value, path]
    await persistProjectsState()
  }

  /** 取消置顶（normalized 比较；未置顶则幂等返回） */
  async function unpinProject(path: string) {
    const n = normalizePath(path)
    const next = pinnedProjects.value.filter(p => normalizePath(p) !== n)
    if (next.length === pinnedProjects.value.length) return
    pinnedProjects.value = next
    await persistProjectsState()
  }

  /**
   * 取指定项目的存档会话 ID（normalized 查找，合并匹配键以容忍跨重启路径漂移）。
   */
  function getArchivedSessions(projectPath: string): string[] {
    const n = normalizePath(projectPath)
    const result: string[] = []
    for (const [key, val] of archivedSessions.entries()) {
      if (normalizePath(key) === n) result.push(...val)
    }
    return result
  }

  /** 存档会话（从历史列表隐藏；已存档则幂等返回） */
  async function archiveSession(projectPath: string, sessionId: string) {
    const n = normalizePath(projectPath)
    // 复用已有键（normalized 匹配）避免重复条目
    let key: string | null = null
    for (const k of archivedSessions.keys()) {
      if (normalizePath(k) === n) { key = k; break }
    }
    const useKey = key ?? projectPath
    const arr = archivedSessions.get(useKey) ?? []
    if (arr.includes(sessionId)) return
    arr.push(sessionId)
    archivedSessions.set(useKey, arr)
    await persistProjectsState()
  }

  /** 恢复存档会话（重新出现在历史列表；未存档则幂等返回；空数组自动清理键） */
  async function restoreSession(projectPath: string, sessionId: string) {
    const n = normalizePath(projectPath)
    let changed = false
    for (const [key, val] of archivedSessions.entries()) {
      if (normalizePath(key) === n) {
        const idx = val.indexOf(sessionId)
        if (idx >= 0) {
          val.splice(idx, 1)
          if (val.length === 0) archivedSessions.delete(key)
          else archivedSessions.set(key, val)
          changed = true
          break
        }
      }
    }
    if (changed) await persistProjectsState()
  }

  return {
    // State
    tabs,
    activeTabId,
    historySessions,
    searchQuery,
    isLoading,
    isLoadingMore,
    pinnedProjects,
    archivedSessions,

    // Computed
    activeTab,
    projectTabs,
    runningTabIds,
    claimedSessionIds,
    hasPendingTabs,

    // Tab lifecycle
    createTab,
    setTabPty,
    handlePtyExit,
    closeTab,
    closeAllTabs,
    closeOtherTabs,
    updateTabName,
    setTabSessionId,

    // Active tab
    setActiveTab,
    getRunningTabForProject,
    getTabByPtyId,
    getTabBySessionId,
    getProjectTabs,

    // Session ID assignment
    assignSessionIdByPtyId,

    // History
    loadHistorySessions,
    invalidateHistoryCache,

    // Search
    setSearchQuery,
    debouncedSearchMessages,
    messageSearchResults,

    // Cleanup
    cleanupAll,

    // 全局树：展开状态 + 多项目历史
    expandOverride,
    toggleExpand,
    isExpanded,
    getHistoryFor,

    // 全局树：项目分组
    buildProjectGroups,
    sortProjectGroups,
    filterProjectGroups,

    // 项目置顶 + 会话存档
    loadProjectsState,
    isPinned,
    pinProject,
    unpinProject,
    getArchivedSessions,
    archiveSession,
    restoreSession,
  }
})
