import type { HistorySession, TerminalTab } from '@/stores/session'
import type {
  CreateUnifiedSessionInput,
  ResumeUnifiedSessionInput,
  SessionAdapter,
  UnifiedSession,
} from '@/types/unifiedSession'
import { normalizePath } from '@/utils/path'

const ACTIVE_PREFIX = 'legacy-tab:'
const HISTORY_PREFIX = 'legacy-history:'

function normalizeProjectIdentity(value: string): string {
  const slashed = value.replace(/\\/g, '/').replace(/\/+$/, '')
  if (/^[A-Za-z]:\//.test(slashed) || slashed.startsWith('//')) {
    return slashed.toLocaleLowerCase('en-US')
  }
  return normalizePath(value)
}

function sameProjectIdentity(left: string, right: string): boolean {
  return normalizeProjectIdentity(left) === normalizeProjectIdentity(right)
}

export interface LegacyClaudeStorePort {
  readonly tabs: Map<string, TerminalTab>
  getCatalogHistoryFor(projectPath: string): HistorySession[]
  getArchivedSessions(projectPath: string): string[]
  createTab(projectPath: string, opts?: { sessionId?: string; name?: string }): string
  setActiveTab(tabId: string | null): void
  removeTab(tabId: string): void
  closeTab(tabId: string): Promise<void>
  updateTabName(tabId: string, name: string): void
  archiveSession(projectPath: string, sessionId: string): Promise<void>
  restoreSession(projectPath: string, sessionId: string): Promise<void>
}

export interface LegacyClaudeRuntimePort {
  startTab(tabId: string): Promise<void>
  stopTab(tabId: string): Promise<void>
  restartTab(tabId: string): Promise<void>
  renameTab(tabId: string, title: string): Promise<void>
}

export interface LegacyClaudeAdapterDeps {
  store: LegacyClaudeStorePort
  runtime: LegacyClaudeRuntimePort
  projectPaths(): string[]
}

function activeId(tabId: string): string {
  return `${ACTIVE_PREFIX}${tabId}`
}

function historyId(projectPath: string, sessionId: string): string {
  return `${HISTORY_PREFIX}${normalizeProjectIdentity(projectPath)}:${sessionId}`
}

function activeTabId(id: string): string {
  if (!id.startsWith(ACTIVE_PREFIX) || id.length === ACTIVE_PREFIX.length) {
    throw new Error('LEGACY_ACTIVE_SESSION_REQUIRED')
  }
  return id.slice(ACTIVE_PREFIX.length)
}

function parseHistoryId(id: string): { projectPath: string; sessionId: string } {
  if (!id.startsWith(HISTORY_PREFIX)) throw new Error('LEGACY_HISTORY_SESSION_REQUIRED')
  const value = id.slice(HISTORY_PREFIX.length)
  const split = value.lastIndexOf(':')
  if (split <= 0 || split === value.length - 1) {
    throw new Error('LEGACY_HISTORY_SESSION_REQUIRED')
  }
  return {
    projectPath: value.slice(0, split),
    sessionId: value.slice(split + 1),
  }
}

function projectActiveTab(tab: TerminalTab): UnifiedSession {
  return {
    id: activeId(tab.tabId),
    projectKey: normalizeProjectIdentity(tab.projectPath),
    projectPath: tab.projectPath,
    cli: 'claude',
    runtime: 'legacy-claude',
    title: tab.name,
    processState: tab.status,
    attentionState: tab.pending ? 'needs-user' : 'none',
    lastActivityAt: tab.lastActiveAt,
    archived: false,
    resumable: Boolean(tab.sessionId),
    adapterSessionId: tab.tabId,
    nativeSessionId: tab.sessionId,
    launchConfigId: null,
    safeErrorCode: null,
    renameState: 'idle',
  }
}

function projectHistorySession(projectPath: string, session: HistorySession): UnifiedSession {
  return {
    id: historyId(projectPath, session.sessionId),
    projectKey: normalizeProjectIdentity(projectPath),
    projectPath,
    cli: 'claude',
    runtime: 'legacy-claude',
    title: session.name,
    processState: 'stopped',
    attentionState: 'none',
    lastActivityAt: session.lastActiveAt,
    archived: false,
    resumable: true,
    adapterSessionId: session.sessionId,
    nativeSessionId: session.sessionId,
    launchConfigId: null,
    safeErrorCode: null,
    renameState: 'idle',
  }
}

function isLegacyClaudeTab(tab: TerminalTab): boolean {
  return tab.cli === undefined || tab.cli === 'claude'
}

function requireClaude(input: { cli: string }): void {
  if (input.cli !== 'claude') throw new Error('LEGACY_CLAUDE_REQUIRED')
}

export function createLegacyClaudeAdapter(deps: LegacyClaudeAdapterDeps): SessionAdapter {
  const { store, runtime } = deps

  function requireTab(id: string): TerminalTab {
    const tabId = activeTabId(id)
    const tab = store.tabs.get(tabId)
    if (!tab || !isLegacyClaudeTab(tab)) throw new Error('LEGACY_SESSION_NOT_FOUND')
    return tab
  }

  function knownProjects(): Map<string, string> {
    const paths = new Map<string, string>()
    for (const path of deps.projectPaths()) {
      const key = normalizeProjectIdentity(path)
      if (key) paths.set(key, path)
    }
    for (const tab of store.tabs.values()) {
      if (!isLegacyClaudeTab(tab)) continue
      const key = normalizeProjectIdentity(tab.projectPath)
      if (key && !paths.has(key)) paths.set(key, tab.projectPath)
    }
    return paths
  }

  async function listSessions(projectKey?: string): Promise<UnifiedSession[]> {
    const wanted = projectKey === undefined ? null : normalizeProjectIdentity(projectKey)
    const sessions: UnifiedSession[] = []

    for (const [key, projectPath] of knownProjects()) {
      if (wanted !== null && key !== wanted) continue
      const tabs = [...store.tabs.values()].filter(tab =>
        isLegacyClaudeTab(tab) && sameProjectIdentity(tab.projectPath, projectPath),
      )
      const claimed = new Set(
        tabs.map(tab => tab.sessionId).filter((value): value is string => Boolean(value)),
      )
      const archived = new Set(store.getArchivedSessions(projectPath))

      sessions.push(...tabs.map(projectActiveTab))
      for (const history of store.getCatalogHistoryFor(projectPath)) {
        if (claimed.has(history.sessionId)) continue
        sessions.push({ ...projectHistorySession(projectPath, history), archived: archived.has(history.sessionId) })
      }
    }

    return sessions.sort((a, b) =>
      b.lastActivityAt - a.lastActivityAt || a.id.localeCompare(b.id),
    )
  }

  async function createSession(input: CreateUnifiedSessionInput): Promise<UnifiedSession> {
    requireClaude(input)
    if (input.action && input.action.kind !== 'new') {
      throw new Error('LEGACY_CREATE_ACTION_UNSUPPORTED')
    }
    const tabId = store.createTab(input.projectPath, { name: input.title })
    store.setActiveTab(tabId)
    try {
      await runtime.startTab(tabId)
    } catch (failure) {
      store.removeTab(tabId)
      throw failure
    }
    return projectActiveTab(requireTab(activeId(tabId)))
  }

  async function resumeSession(input: ResumeUnifiedSessionInput): Promise<UnifiedSession> {
    requireClaude(input)
    const nativeSessionId = input.nativeSessionId ?? input.adapterSessionId
    const existing = [...store.tabs.values()].find(tab =>
      isLegacyClaudeTab(tab)
      && tab.sessionId === nativeSessionId
      && sameProjectIdentity(tab.projectPath, input.projectPath),
    )
    if (existing) {
      store.setActiveTab(existing.tabId)
      return projectActiveTab(existing)
    }

    const tabId = store.createTab(input.projectPath, {
      sessionId: nativeSessionId,
      name: input.title,
    })
    store.setActiveTab(tabId)
    try {
      await runtime.startTab(tabId)
    } catch (failure) {
      store.removeTab(tabId)
      throw failure
    }
    return projectActiveTab(requireTab(activeId(tabId)))
  }

  function captureOwnership(id: string): () => boolean {
    if (!id.startsWith(ACTIVE_PREFIX)) return () => true
    const tab = requireTab(id)
    const generation = tab.ptyGeneration ?? 0
    return () => store.tabs.get(tab.tabId) === tab && (tab.ptyGeneration ?? 0) === generation
  }

  async function activateSession(id: string): Promise<void> {
    store.setActiveTab(requireTab(id).tabId)
  }

  async function stopSession(id: string): Promise<void> {
    await runtime.stopTab(requireTab(id).tabId)
  }

  async function restartSession(id: string): Promise<UnifiedSession> {
    const tab = requireTab(id)
    await runtime.restartTab(tab.tabId)
    return projectActiveTab(requireTab(id))
  }

  async function closeSession(id: string): Promise<void> {
    await store.closeTab(requireTab(id).tabId)
  }

  async function renameSession(id: string, title: string): Promise<void> {
    const value = title.trim()
    if (!value || value.includes('\0')) throw new Error('SESSION_TITLE_REQUIRED')
    const tab = requireTab(id)
    await runtime.renameTab(tab.tabId, value)
    store.updateTabName(tab.tabId, value)
  }

  async function archiveSession(id: string): Promise<void> {
    if (id.startsWith(ACTIVE_PREFIX)) {
      const tab = requireTab(id)
      const owns = captureOwnership(id)
      if (!tab.sessionId) throw new Error('SESSION_NOT_RESUMABLE')
      if (tab.status === 'running' || tab.status === 'starting') {
        await runtime.stopTab(tab.tabId)
      }
      if (!owns()) throw new Error('STALE_SESSION_ATTEMPT')
      await store.archiveSession(tab.projectPath, tab.sessionId)
      if (!owns()) throw new Error('STALE_SESSION_ATTEMPT')
      await store.closeTab(tab.tabId)
      return
    }
    const history = parseHistoryId(id)
    await store.archiveSession(history.projectPath, history.sessionId)
  }

  async function restoreArchivedSession(id: string): Promise<void> {
    const history = parseHistoryId(id)
    await store.restoreSession(history.projectPath, history.sessionId)
  }

  return {
    runtime: 'legacy-claude',
    captureOwnership,
    listSessions,
    createSession,
    resumeSession,
    activateSession,
    stopSession,
    restartSession,
    closeSession,
    renameSession,
    archiveSession,
    restoreArchivedSession,
  }
}