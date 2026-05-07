import { defineStore } from 'pinia'
import { ref, computed, reactive } from 'vue'
import {
  getSessionCount,
  getSessions,
  ptyKill,
  searchSessionMessages
} from '@/api/tauri'
import type { SessionSearchResult } from '@/types'
import type { ClaudeState } from '@/types/hook'

const WORKING_STATES: ClaudeState[] = ['thinking', 'tool_executing', 'subagent_running', 'compacting']
const PENDING_STATES: ClaudeState[] = ['waiting_permission', 'waiting_input']
const ACTIVE_STATES: ClaudeState[] = [...WORKING_STATES, ...PENDING_STATES]
const STALENESS_THRESHOLD_MS = 120_000

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
  tabId: string              // 稳定 ID，创建时生成
  projectPath: string        // 工作目录
  ptyId: string | null       // PTY 进程 ID（停止时 null）
  sessionId: string | null   // Claude 真实 session ID（匹配后赋值）
  name: string               // 显示名称
  status: 'starting' | 'running' | 'stopped'
  createdAt: number
  lastActiveAt: number
  claudeState: ClaudeState   // Claude 运行时状态
  model?: string             // 模型名
  lastStateUpdateAt: number  // 上次状态更新时间（用于过期检测）
  pending: boolean           // 是否需要用户关注
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

// ==================== Store ====================

export const useSessionStore = defineStore('session', () => {
  // ---- State ----
  const tabs = reactive(new Map<string, TerminalTab>())
  const activeTabId = ref<string | null>(null)
  /** 完整历史会话缓存（不含 Tab 占用的，由 computed 过滤） */
  const allHistoryCache = ref<HistorySession[]>([])
  const searchQuery = ref<string>('')
  const isLoading = ref<boolean>(false)
  const messageSearchResults = ref<SessionSearchResult[]>([])
  let messageSearchTimer: ReturnType<typeof setTimeout> | null = null

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

  /** 未被 Tab 占用的历史会话（搜索过滤由组件处理） */
  const historySessions = computed<HistorySession[]>(() => {
    const claimed = claimedSessionIds.value
    return allHistoryCache.value.filter(s => !claimed.has(s.sessionId))
  })

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
      claudeState: 'unknown',
      lastStateUpdateAt: now,
      pending: false,
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
        tab.claudeState = 'unknown'
        tab.lastActiveAt = Date.now()
        break
      }
    }
  }

  /**
   * 关闭 Tab（用户主动操作）
   * 不需要重新加载历史会话，computed 会自动显示释放的 sessionId
   */
  async function closeTab(tabId: string) {
    const tab = tabs.get(tabId)
    if (!tab) return

    const projectPath = tab.projectPath

    // 如果有运行中的 PTY，先 kill
    if (tab.ptyId) {
      try { await ptyKill(tab.ptyId) } catch {}
    }

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

  // ---- 会话匹配 ----

  // 匹配轮询定时器
  const matchTimers = new Map<string, ReturnType<typeof setInterval>>()

  /**
   * 启动自动匹配轮询
   * 每 3 秒尝试匹配，最多持续 60 秒，匹配成功后自动停止
   */
  function startMatchPolling(tabId: string) {
    if (matchTimers.has(tabId)) return // 已在轮询

    const tab = tabs.get(tabId)
    if (!tab || tab.sessionId) return

    const startedAt = Date.now()
    const MAX_DURATION = 60_000
    const INTERVAL = 3_000

    const timer = setInterval(async () => {
      const tab = tabs.get(tabId)
      // 停止条件：已匹配 / Tab 不存在 / 已停止 / 超时
      if (!tab || tab.sessionId || tab.status !== 'running' || Date.now() - startedAt > MAX_DURATION) {
        stopMatchPolling(tabId)
        return
      }

      await matchSessionForTab(tabId)
    }, INTERVAL)

    matchTimers.set(tabId, timer)
  }

  function stopMatchPolling(tabId: string) {
    const timer = matchTimers.get(tabId)
    if (timer) {
      clearInterval(timer)
      matchTimers.delete(tabId)
    }
  }

  /**
   * PTY 输出驱动的即时匹配（debounce）
   * 由 XTermTerminal 在收到 PTY 输出时调用
   */
  const matchDebounceTimers = new Map<string, ReturnType<typeof setTimeout>>()

  function triggerOutputDrivenMatch(tabId: string) {
    const tab = tabs.get(tabId)
    if (!tab || tab.sessionId) return

    // debounce：上次触发后 1.5 秒才真正执行，避免频繁调用
    if (matchDebounceTimers.has(tabId)) return

    matchDebounceTimers.set(tabId, setTimeout(async () => {
      matchDebounceTimers.delete(tabId)
      const success = await matchSessionForTab(tabId)
      if (success) stopMatchPolling(tabId)
    }, 1500))
  }

  /**
   * 为 Tab 匹配真实的 Claude session ID
   */
  async function matchSessionForTab(tabId: string): Promise<boolean> {
    const tab = tabs.get(tabId)
    if (!tab || tab.sessionId) return false // 已有 sessionId

    try {
      const sessions = await getSessions(tab.projectPath, 50, 0)
      const claimed = claimedSessionIds.value

      // 找到比 tab.createdAt 更新、且未被其他 Tab 占用的最新 session
      const match = sessions
        .filter(s =>
          s.lastActiveAt >= tab.createdAt &&
          !claimed.has(s.sessionId)
        )
        .sort((a, b) => b.lastActiveAt - a.lastActiveAt)[0]

      if (match) {
        setTabSessionId(tabId, match.sessionId, match.name)
        // computed 会自动过滤掉已占用的 sessionId
        return true
      }
    } catch (err) {
      console.error('[SessionStore] matchSessionForTab failed:', err)
    }

    return false
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

  // ---- Claude 运行时状态 ----

  function updateClaudeState(ptyId: string, state: ClaudeState, model?: string) {
    const tab = getTabByPtyId(ptyId)
    if (!tab || tab.status !== 'running') return

    const prevState = tab.claudeState
    const wasActive = tab.tabId === activeTabId.value

    tab.claudeState = state
    tab.lastStateUpdateAt = Date.now()
    if (model) tab.model = model

    // Working → Pending：需要用户操作
    if (WORKING_STATES.includes(prevState) && PENDING_STATES.includes(state)) {
      tab.pending = true
    }
    // Working → Idle：工作完成
    if (WORKING_STATES.includes(prevState) && state === 'idle') {
      tab.pending = true
    }
  }

  const hasPendingTabs = computed(() => {
    for (const tab of tabs.values()) {
      if (tab.pending) return true
    }
    return false
  })

  // ---- 过期检测 ----

  let stalenessTimer: ReturnType<typeof setInterval> | null = null
  let stalenessStarted = false

  function ensureStalenessCheck() {
    if (stalenessStarted) return
    stalenessStarted = true
    stalenessTimer = setInterval(() => {
      const now = Date.now()
      for (const tab of tabs.values()) {
        if (tab.status !== 'running' || !tab.ptyId) continue
        if (ACTIVE_STATES.includes(tab.claudeState) &&
            now - tab.lastStateUpdateAt > STALENESS_THRESHOLD_MS) {
          tab.claudeState = 'idle'
          tab.lastStateUpdateAt = now
        }
      }
    }, 30_000)
  }

  // ---- 历史会话 ----

  /**
   * 加载历史会话（全量缓存，computed 会过滤 Tab 占用的）
   */
  async function loadHistorySessions(projectPath: string) {
    isLoading.value = true
    try {
      const count = await getSessionCount(projectPath)
      const allSessions = await getSessions(projectPath, count, 0)

      allHistoryCache.value = allSessions
        .map(s => ({
          sessionId: s.sessionId,
          name: s.name,
          projectPath: s.projectPath,
          lastActiveAt: s.lastActiveAt,
        }))
        .sort((a, b) => b.lastActiveAt - a.lastActiveAt)
    } catch (err) {
      console.error('[SessionStore] loadHistorySessions failed:', err)
    } finally {
      isLoading.value = false
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

  // ---- 清理 ----

  async function cleanupAll() {
    // 清理所有匹配定时器
    for (const timer of matchTimers.values()) clearInterval(timer)
    matchTimers.clear()
    for (const timer of matchDebounceTimers.values()) clearTimeout(timer)
    matchDebounceTimers.clear()
    if (stalenessTimer) {
      clearInterval(stalenessTimer)
      stalenessTimer = null
    }

    const ids = [...tabs.values()]
      .filter(t => t.ptyId)
      .map(t => t.ptyId!)

    for (const id of ids) {
      try { await ptyKill(id) } catch {}
    }
    tabs.clear()
    allHistoryCache.value = []
  }

  function getProjectTabs(projectPath: string): TerminalTab[] {
    return [...tabs.values()]
      .filter(t => t.projectPath === projectPath)
      .sort((a, b) => b.lastActiveAt - a.lastActiveAt)
  }

  return {
    // State
    tabs,
    activeTabId,
    historySessions,
    searchQuery,
    isLoading,

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
    updateTabName,
    setTabSessionId,
    matchSessionForTab,
    startMatchPolling,
    stopMatchPolling,
    triggerOutputDrivenMatch,

    // Active tab
    setActiveTab,
    getRunningTabForProject,
    getTabByPtyId,
    getProjectTabs,

    // Claude runtime state
    updateClaudeState,
    ensureStalenessCheck,

    // History
    loadHistorySessions,

    // Search
    setSearchQuery,
    debouncedSearchMessages,
    messageSearchResults,

    // Cleanup
    cleanupAll,
  }
})
