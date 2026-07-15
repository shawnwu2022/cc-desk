import { defineStore } from 'pinia'
import { ref, computed, reactive } from 'vue'
import {
  getProjectsState,
  getSessions,
  ptyKill,
  searchSessionMessages,
  pinProject as pinProjectApi,
  unpinProject as unpinProjectApi,
  archiveSession as archiveSessionApi,
  restoreSession as restoreSessionApi,
  setDisplayName as setDisplayNameApi,
} from '@/api/tauri'
import { normalizePath, sameProjectPath } from '@/utils/path'
import { validateDisplayName, projectBasename, matchProjectQuery } from '@/utils/displayName'
import { createProjectsStateSync } from '@/utils/projectsStateSync'
import type { SessionSearchResult } from '@/types'
import type { ProjectsState } from '@/types/app'

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

// ==================== Store ====================

export const useSessionStore = defineStore('session', () => {
  // ---- State ----
  const tabs = reactive(new Map<string, TerminalTab>())
  const activeTabId = ref<string | null>(null)
  /** 项目级历史会话缓存 Map */
  const historyCacheMap = reactive(new Map<string, HistorySession[]>())
  /** 当前展示历史会话的项目路径 */
  const currentHistoryProject = ref<string>('')
  /** per 项目历史加载状态（v5-T4 替代旧单值 inflightLoadProject） */
  const historyLoadState = reactive(new Map<string, { loading: boolean; error: string | null }>())
  /** 当前请求（每规范化路径最多 1） */
  const inflight = new Map<string, Promise<HistorySession[]>>()
  /** 排队 force（每规范化路径最多 1）：force 遇 inflight 时排队，等当前结束后追加刷新 */
  const queuedForce = new Map<string, Promise<HistorySession[]>>()
  /** 请求代次（每规范化路径递增）：仅最后有效请求可改 loading/error，避免旧请求 settle 把新请求的 loading 提前置 false */
  const fetchGeneration = new Map<string, number>()
  const searchQuery = ref<string>('')
  const isLoadingMore = ref<boolean>(false)
  const messageSearchResults = ref<SessionSearchResult[]>([])
  let messageSearchTimer: ReturnType<typeof setTimeout> | null = null

  /** 置顶项目（持久化到 projects.json，启动加载） */
  const pinnedProjects = ref<string[]>([])
  /** 会话存档：projectPath -> sessionId[]（持久化到 projects.json，启动加载） */
  const archivedSessions = reactive(new Map<string, string[]>())
  /** 项目别名：normalizedPath -> 别名（持久化到 projects.json displayNames，启动加载） */
  const displayNames = reactive(new Map<string, string>())
  /** projects.json 是否已加载完成（P1.2 门禁：pin/archive 前须确保加载，否则用空内存覆写旧文件） */
  const projectsStateLoaded = ref(false)
  /** projects.json 加载是否失败（P1 v-if 门禁：失败时树显示失败提示 + 重试，不读空状态） */
  const projectsStateError = ref(false)
  /** loadProjectsState 的进行中 Promise（供 ensureProjectsStateLoaded 复用，避免重复加载） */
  let loadPromise: Promise<void> | null = null
  /** pin/unpin/archive/restore 操作锁（P1.3 串行化，避免并发各自基于旧内存算 next 丢更新） */
  let opLock: Promise<void> = Promise.resolve()

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
    const cached = historyCacheMap.get(normalizePath(currentHistoryProject.value)) ?? []
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
    const cached = historyCacheMap.get(normalizePath(projectPath)) ?? []
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
      if (sameProjectPath(tab.projectPath, projectPath)) {
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
   * 删除 Tab 但不 kill PTY、不刷历史（供 startTab 失败时清理未启动 tab）。
   * 与 closeTab 的区别：closeTab 是用户主动关闭（kill PTY + 刷历史），
   * removeTab 是内部清理（tab 尚未启动或 PTY 已失败，无需 kill/刷新）。
   */
  function removeTab(tabId: string) {
    const tab = tabs.get(tabId)
    if (!tab) return
    const projectPath = tab.projectPath
    tabs.delete(tabId)
    if (activeTabId.value === tabId) {
      // 聚焦到同项目的相邻 tab；无则置空
      const remaining = getProjectTabs(projectPath)
      activeTabId.value = remaining.length > 0 ? remaining[0].tabId : null
    }
  }
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
      if (sameProjectPath(tab.projectPath, projectPath) && tab.status === 'running') {
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

  /** isLoading 由当前展示项目的 loadState 派生（替代旧单值 inflightLoadProject） */
  const isLoading = computed(() => {
    const n = normalizePath(currentHistoryProject.value)
    return historyLoadState.get(n)?.loading ?? false
  })

  function setLoadState(n: string, loading: boolean, error: string | null) {
    historyLoadState.set(n, { loading, error })
  }

  /**
   * 实际拉取 + 分页 + 写缓存（不处理并发/force）。
   * 缓存 key 用规范化路径（与 getHistoryFor/invalidate 一致）；调用后端 getSessions 保留原始路径
   * （后端按原始路径查 JSONL）。分页 try/finally 保证 isLoadingMore 复位；分页失败清缓存避免 partial 当完整。
   */
  async function runFetch(projectPath: string): Promise<HistorySession[]> {
    const n = normalizePath(projectPath)
    // 请求 BATCH_SIZE + 1 条来判断是否有更多（后端用原始路径）
    const firstBatch = await getSessions(projectPath, BATCH_SIZE + 1, 0)
    const hasMore = firstBatch.length > BATCH_SIZE
    const firstPage = hasMore ? firstBatch.slice(0, BATCH_SIZE) : firstBatch

    const mapped = firstPage
      .map(s => ({ sessionId: s.sessionId, name: s.name, projectPath: s.projectPath, lastActiveAt: s.lastActiveAt }))
      .sort((a, b) => b.lastActiveAt - a.lastActiveAt)

    // 数据就绪后才写入缓存（规范化 key；不提前清空）
    historyCacheMap.set(n, mapped)
    if (!hasMore) return historyCacheMap.get(n) ?? []

    // 有更多：分页拉取剩余。try/finally 保证 isLoadingMore 必复位；
    // 失败时清缓存（partial 不当完整）并向上抛错，由 fetchHistory 据代次置 error。
    isLoadingMore.value = true
    try {
      let offset = BATCH_SIZE
      let all = [...mapped]
      while (true) {
        const batch = await getSessions(projectPath, BATCH_SIZE, offset)
        if (batch.length === 0) break
        const more = batch
          .map(s => ({ sessionId: s.sessionId, name: s.name, projectPath: s.projectPath, lastActiveAt: s.lastActiveAt }))
        all = [...all, ...more].sort((a, b) => b.lastActiveAt - a.lastActiveAt)
        offset += batch.length
        if (batch.length < BATCH_SIZE) break
      }
      historyCacheMap.set(n, all)
      return historyCacheMap.get(n) ?? []
    } catch (e) {
      // 分页失败：清缓存（避免 partial 被后续命中当完整），错误向上传播
      historyCacheMap.delete(n)
      throw e
    } finally {
      isLoadingMore.value = false
    }
  }

  /** 启动一次新拉取并登记为 inflight（CAS 自清：旧不覆盖新） */
  function startFetch(n: string, projectPath: string): Promise<HistorySession[]> {
    const p = runFetch(projectPath)
    inflight.set(n, p)
    // CAS 自清：p 结束后，仅当 inflight 仍指向 p 时清除（排队 force 已接管 inflight 时不清）。
    // 用 .then(clear, clear) 替代 .finally：避免 reject 时 unhandled rejection（.finally 返回新 promise 未消费）。
    const clear = () => { if (inflight.get(n) === p) inflight.delete(n) }
    p.then(clear, clear)
    return p
  }

  /**
   * 历史加载核心（v5-T4 两层 force 状态机 + v6 代次保护）。
   * - 普通：缓存命中秒回；否则复用排队 force（更新）> 当前 inflight > 新建
   * - force + 无 inflight：直接新建
   * - force + 有 inflight：排队一次（多 force 合并），等当前结束后追加刷新
   * - 每规范化路径最多 1 当前 + 1 排队 force -> 无两并发 -> 旧不覆盖新天然成立
   * - 代次（generation）：仅最后有效请求可置 loading/error，避免旧请求 settle 把新请求的 loading 提前置 false
   *   （场景：normal A 在途，force B 排队；A resolve 不应把 loading 置 false，因 B 仍在拉）。
   */
  async function fetchHistory(projectPath: string, force = false): Promise<{ ok: true; sessions: HistorySession[] } | { ok: false; error: string }> {
    const n = normalizePath(projectPath)
    // 缓存命中秒回（非 force；规范化 key；保留 v3 语义，避免 watch/toggle 触发冗余拉取）
    if (!force && historyCacheMap.has(n)) {
      return { ok: true, sessions: historyCacheMap.get(n)! }
    }
    const myGen = (fetchGeneration.get(n) ?? 0) + 1
    fetchGeneration.set(n, myGen)
    const isLatest = () => fetchGeneration.get(n) === myGen
    if (isLatest()) setLoadState(n, true, null)
    try {
      let p: Promise<HistorySession[]>
      if (!force) {
        const q = queuedForce.get(n)
        const c = inflight.get(n)
        if (q) p = q                       // 复用排队 force（更新）
        else if (c) p = c                  // 复用当前 inflight
        else p = startFetch(n, projectPath) // 新建
      } else {
        const c = inflight.get(n)
        if (!c) {
          p = startFetch(n, projectPath)   // 无当前：直接发
        } else {
          // 有当前：排队一次（多 force 合并）
          let q = queuedForce.get(n)
          if (!q) {
            q = (async () => {
              await c.catch(() => {})            // 等当前结束（忽略其错误，force 仍刷新）
              queuedForce.delete(n)              // 清排队标记
              return startFetch(n, projectPath)  // 追加刷新（startFetch 设 inflight）
            })()
            queuedForce.set(n, q)
          }
          p = q
        }
      }
      const sessions = await p
      if (isLatest()) setLoadState(n, false, null)
      return { ok: true, sessions }
    } catch (e) {
      if (isLatest()) setLoadState(n, false, String(e))
      return { ok: false, error: String(e) }
    }
  }

  /** 管理页用：加载历史，不改 currentHistoryProject（无副作用） */
  async function loadHistoryFor(projectPath: string, force = false) {
    return fetchHistory(projectPath, force)
  }

  /**
   * 侧栏用：加载历史 + 切 currentHistoryProject（存规范化 key，与 historyCacheMap key 一致）。
   */
  async function loadHistorySessions(projectPath: string, force = false) {
    // 切换当前展示项目（即使还在加载中也立即切换，让 computed 指向正确项目）
    currentHistoryProject.value = normalizePath(projectPath)
    return fetchHistory(projectPath, force)
  }

  /** 清除指定项目或全部历史缓存（指定项目时按规范化 key 删，与缓存 key 一致） */
  function invalidateHistoryCache(projectPath?: string) {
    if (projectPath) {
      historyCacheMap.delete(normalizePath(projectPath))
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
    historyLoadState.clear()
    inflight.clear()
    queuedForce.clear()
    fetchGeneration.clear()
    currentHistoryProject.value = ''
  }

  function getProjectTabs(projectPath: string): TerminalTab[] {
    return [...tabs.values()]
      .filter(t => sameProjectPath(t.projectPath, projectPath))
      .sort((a, b) => b.lastActiveAt - a.lastActiveAt)
  }

  /**
   * 构建项目分组（含孤儿）。
   * @param cachedProjects 来自 appStore.cachedProjects 的项目列表
   * @param hidden 隐藏项目集合（normalized 比较；隐藏项目及其孤儿 tab 不入树）
   */
  function buildProjectGroups(
    cachedProjects: { path: string; name: string }[],
    hidden?: Set<string>,
  ): ProjectGroup[] {
    const hiddenSet = hidden ? new Set([...hidden].map(h => normalizePath(h))) : null
    // 过滤 hidden 项目（含规范化比较）
    const visibleProjects = hiddenSet
      ? cachedProjects.filter(p => !hiddenSet.has(normalizePath(p.path)))
      : cachedProjects
    const known = new Set(visibleProjects.map(p => normalizePath(p.path)))

    // 按 normalized path 聚合 tabs，避免 Windows 路径大小写/斜杠不一致时
    // 精确匹配（getProjectTabs）漏 tab：known 判非孤儿却贴不到 tab。
    const tabsByNorm = new Map<string, { tabs: TerminalTab[]; firstRaw: string }>()
    for (const tab of tabs.values()) {
      const n = normalizePath(tab.projectPath)
      // hidden 项目的 tab 不入树（不作孤儿出现）
      if (hiddenSet && hiddenSet.has(n)) continue
      const entry = tabsByNorm.get(n)
      if (entry) entry.tabs.push(tab)
      else tabsByNorm.set(n, { tabs: [tab], firstRaw: tab.projectPath })
    }
    // 同一项目内按 lastActiveAt 倒序，保持与 getProjectTabs 一致
    for (const entry of tabsByNorm.values()) {
      entry.tabs.sort((a, b) => b.lastActiveAt - a.lastActiveAt)
    }

    const groups: ProjectGroup[] = []
    for (const p of visibleProjects) {
      const projTabs = tabsByNorm.get(normalizePath(p.path))?.tabs ?? []
      groups.push(makeGroup(p.path, getDisplayName(p.path), projTabs, false))
    }
    // 孤儿：tabs 中有但 cachedProjects 没有（Map 已按 normalized key 去重）
    for (const [n, entry] of tabsByNorm) {
      if (known.has(n)) continue
      groups.push(makeGroup(entry.firstRaw, getDisplayName(entry.firstRaw), entry.tabs, true))
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
        const matchProject = matchProjectQuery(
          getDisplayName(g.projectPath),
          projectBasename(g.projectPath),
          g.projectPath,
          q,
        )
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

  // 多实例状态同步队列（spec §3.8）：action 无条件应用、reload 条件应用，防逆序覆盖
  const sync = createProjectsStateSync()

  /** 用后端返回的 ProjectsState 覆盖本地三份（返回值是锁内写完的最新 canonical 状态）。 */
  function applyReturnedState(s: ProjectsState) {
    pinnedProjects.value = s.pinnedProjects ?? []
    archivedSessions.clear()
    for (const [k, v] of Object.entries(s.archivedSessions ?? {})) archivedSessions.set(k, v)
    displayNames.clear()
    for (const [k, v] of Object.entries(s.displayNames ?? {})) {
      if (typeof v === 'string') displayNames.set(k, v)
    }
  }

  /**
   * 启动加载：读取 pinnedProjects + archivedSessions（参考 loadAppConfig 范式）。
   * 后端 read_projects_state_locked 已 canonical（normalize + 去重 + 排序），前端 applyReturnedState 原样应用。
   * P1.2：不吞失败--失败时 projectsStateLoaded 保持 false，ensureProjectsStateLoaded 据此抛错
   * 阻断 pin/archive；但本函数不抛出（避免阻断 App.vue fire-and-forget 启动）。
   * 赋值 loadPromise 供 ensureProjectsStateLoaded 复用（并发等待同一加载，不重复发请求）。
   * P2：失败时 loadPromise 重置为 null，允许下次 ensureProjectsStateLoaded 重试
   * （否则已 settled 的 loadPromise 被反复 await，一次临时 IPC/文件错误致本次进程永久不可用）。
   * projectsStateError 供 UI 门禁区分「加载中」与「加载失败」（失败提示 + 重试按钮）。
   */
  function loadProjectsState(): Promise<void> {
    loadPromise = (async () => {
      try {
        projectsStateError.value = false
        const state = await getProjectsState()
        applyReturnedState(state)          // 后端 read_projects_state_locked 已 canonical
        projectsStateLoaded.value = true
      } catch (err) {
        console.error('[SessionStore] loadProjectsState failed:', err)
        projectsStateLoaded.value = false
        projectsStateError.value = true
        loadPromise = null
        throw err
      }
    })()
    return loadPromise
  }

  /**
   * 确保 projects.json 已加载完成（P1.2 门禁，pin/unpin/archive/restore 开头调用）。
   * - loadPromise 已存在（含进行中）则直接 await 同一个，并发不重复触发
   * - loadPromise 为 null（首次 / 上次失败已重置）则触发 loadProjectsState 并 await
   * - 加载失败（projectsStateLoaded=false，loadPromise 已被重置为 null）则抛错，
   *   阻止后续操作用空内存覆写磁盘旧数据；下次调用可重试（P2）
   */
  async function ensureProjectsStateLoaded(): Promise<void> {
    if (loadPromise === null) {
      loadProjectsState()
    }
    await loadPromise
    if (!projectsStateLoaded.value) {
      throw new Error('projects state load failed; pin/archive blocked')
    }
  }

  /**
   * 聚焦 reload：force 读取最新 projects.json 并经 sync 队列条件应用。
   * emitSeq = 发起时 currentSeq()；期间若有 action 应用，本 reload 响应被丢弃（防逆序覆盖）。
   * 静默失败（不打断聚焦，保持上次状态）。
   */
  async function reloadProjectsState(): Promise<void> {
    if (!projectsStateLoaded.value) return
    const emitSeq = sync.currentSeq()
    try {
      const s = await getProjectsState()
      await sync.applyFromReload(s, applyReturnedState, emitSeq)
    } catch (err) {
      console.error('[SessionStore] reloadProjectsState failed:', err)
    }
  }

  /**
   * 操作锁（P1.3）：串行化 invoke + apply--ensureProjectsStateLoaded → applyFromAction（发增量 invoke）
   * → applyReturnedState 全在锁内。增量操作在后端锁内原子读改写，本锁仅保证前端单实例串行
   * （多实例跨进程排他由后端 projects.json.lock 负责）。
   */
  function withLock<T>(fn: () => Promise<T>): Promise<T> {
    const prev = opLock
    let release!: () => void
    opLock = new Promise<void>(r => { release = r })
    return (async () => {
      await prev
      try {
        return await fn()
      } finally {
        release()
      }
    })()
  }

  /** 该项目是否已置顶（normalized 比较，兼容 Windows 路径大小写/斜杠差异） */
  function isPinned(path: string): boolean {
    const n = normalizePath(path)
    return pinnedProjects.value.some(p => normalizePath(p) === n)
  }

  /** 置顶项目（始终发后端；后端锁内据最新磁盘幂等。前端不做本地短路——本地不能证明磁盘状态）。 */
  async function pinProject(path: string) {
    return withLock(async () => {
      await ensureProjectsStateLoaded()
      const s = await pinProjectApi(path)
      await sync.applyFromAction(s, applyReturnedState)
    })
  }

  /** 取消置顶（始终发后端，后端锁内 normalized 移除）。 */
  async function unpinProject(path: string) {
    return withLock(async () => {
      await ensureProjectsStateLoaded()
      const s = await unpinProjectApi(path)
      await sync.applyFromAction(s, applyReturnedState)
    })
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

  /**
   * 取该项目的已存档会话信息（name/lastActiveAt）。
   * 从 historyCacheMap 按 archived ID 查；未加载或缓存未命中则 name 回退 ID 截断、
   * lastActiveAt 回退 0（UI 据此隐藏时间）。供已存档弹层区分会话用。
   */
  function getArchivedSessionInfos(projectPath: string): { sessionId: string; name: string; lastActiveAt: number }[] {
    const ids = getArchivedSessions(projectPath)
    const cached = historyCacheMap.get(normalizePath(projectPath)) ?? []
    return ids.map(id => {
      const h = cached.find(s => s.sessionId === id)
      return { sessionId: id, name: h?.name ?? id.slice(0, 8), lastActiveAt: h?.lastActiveAt ?? 0 }
    })
  }

  /** 存档会话（始终发后端，后端锁内 sessionId 去重归并）。 */
  async function archiveSession(projectPath: string, sessionId: string) {
    return withLock(async () => {
      await ensureProjectsStateLoaded()
      const s = await archiveSessionApi(projectPath, sessionId)
      await sync.applyFromAction(s, applyReturnedState)
    })
  }

  /** 恢复会话（始终发后端，后端锁内移除 + 空数组清理 key）。 */
  async function restoreSession(projectPath: string, sessionId: string) {
    return withLock(async () => {
      await ensureProjectsStateLoaded()
      const s = await restoreSessionApi(projectPath, sessionId)
      await sync.applyFromAction(s, applyReturnedState)
    })
  }

  /**
   * 取项目显示名：有别名返别名，无则 basename 回退（projectBasename 去尾斜杠 + 反斜杠规范）。
   * 供 buildProjectGroups / TitleBar / native title / 管理页统一消费。
   * reactive Map -> 依赖 getDisplayName 的 computed/watch 自动重算。
   */
  function getDisplayName(projectPath: string): string {
    const alias = displayNames.get(normalizePath(projectPath))
    return alias ? alias : projectBasename(projectPath)
  }

  /** 设别名（前端 validateDisplayName 前置快速反馈 + 后端锁内校验兜底；空=清除）。 */
  async function setDisplayName(path: string, alias: string): Promise<void> {
    const v = validateDisplayName(alias)
    if (!v.ok) {
      throw new Error(v.error === 'tooLong' ? 'alias too long' : 'alias invalid characters')
    }
    return withLock(async () => {
      await ensureProjectsStateLoaded()
      const s = await setDisplayNameApi(path, alias)
      await sync.applyFromAction(s, applyReturnedState)
    })
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
    displayNames,
    currentHistoryProject,
    historyLoadState,

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
    removeTab,
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
    fetchHistory,
    loadHistoryFor,
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
    reloadProjectsState,
    projectsStateLoaded,
    projectsStateError,
    ensureProjectsStateLoaded,
    isPinned,
    pinProject,
    unpinProject,
    getArchivedSessions,
    getArchivedSessionInfos,
    archiveSession,
    restoreSession,
    getDisplayName,
    setDisplayName,
  }
})
