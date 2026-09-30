import type { LaunchAction } from '@/types/cli'
import type { NativeCliTab } from '@/stores/nativeTabs'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'
import type {
  CreateUnifiedSessionInput,
  ResumeUnifiedSessionInput,
  SessionAdapter,
  UnifiedSession,
} from '@/types/unifiedSession'
import { makeSessionCatalogKey } from '@/utils/sessionPresentation'
import { normalizePath } from '@/utils/path'

const ACTIVE_PREFIX = 'native-tab:'
const HISTORY_PREFIX = 'native-history:'

export interface NativeTabsPort {
  readonly tabs: Map<string, NativeCliTab>
  readonly activeTabId: string | null
  setActive(tabId: string | null): void
  close(tabId: string): void
  rename(tabId: string, title: string): void
}

export interface NativeHistoryPort {
  all(): NativeHistoryEntry[]
}

export interface NativeRuntimePort {
  createTab(input: CreateUnifiedSessionInput & { action: LaunchAction }): Promise<NativeCliTab> | NativeCliTab
  restartTab(tabId: string): Promise<NativeCliTab> | NativeCliTab
  stopTab(tab: NativeCliTab): Promise<void>
}

export interface NativeArchivePort {
  archiveSession(projectPath: string, sessionId: string): Promise<unknown>
  restoreSession(projectPath: string, sessionId: string): Promise<unknown>
}

export interface NativeCliAdapterDeps {
  tabs: NativeTabsPort
  history: NativeHistoryPort
  runtime: NativeRuntimePort
  archive: NativeArchivePort
}

function processState(status: NativeCliTab['status']): UnifiedSession['processState'] {
  if (status === 'starting') return 'starting'
  if (status === 'running') return 'running'
  if (status === 'unknown') return 'unknown'
  if (status === 'failed') return 'failed'
  return 'stopped'
}

function activeId(tabId: string): string { return `${ACTIVE_PREFIX}${tabId}` }
function activeTabId(id: string): string {
  if (!id.startsWith(ACTIVE_PREFIX) || id.length === ACTIVE_PREFIX.length) throw new Error('NATIVE_ACTIVE_SESSION_REQUIRED')
  return id.slice(ACTIVE_PREFIX.length)
}

function tabNativeSessionId(tab: NativeCliTab): string | null {
  return tab.action.kind === 'resume-id' ? tab.action.nativeSessionId : null
}

function projectTab(tab: NativeCliTab): UnifiedSession {
  const nativeSessionId = tabNativeSessionId(tab)
  return {
    id: activeId(tab.tabId),
    projectKey: normalizePath(tab.projectPath),
    projectPath: tab.projectPath,
    cli: tab.cli,
    runtime: 'native-cli',
    title: tab.title,
    processState: processState(tab.status),
    attentionState: 'none',
    lastActivityAt: tab.lastActivityAt,
    archived: false,
    resumable: Boolean(nativeSessionId),
    adapterSessionId: tab.tabId,
    nativeSessionId,
    launchConfigId: tab.profileId,
    safeErrorCode: tab.errorCode,
    renameState: 'idle',
  }
}

function historyId(entry: NativeHistoryEntry, sessionKey: string, nativeSessionId: string): string {
  return `${HISTORY_PREFIX}${makeSessionCatalogKey({
    runtime: 'native-cli',
    cli: entry.context.cli,
    projectPath: entry.context.projectPath,
    adapterSessionId: sessionKey,
    nativeSessionId,
  })}`
}

function projectHistory(entry: NativeHistoryEntry, item: NativeHistoryEntry['sessions'][number]): UnifiedSession {
  const updated = item.updatedAt ? Date.parse(item.updatedAt) : 0
  return {
    id: historyId(entry, item.sessionKey, item.nativeSessionId),
    projectKey: normalizePath(entry.context.projectPath),
    projectPath: entry.context.projectPath,
    cli: entry.context.cli,
    runtime: 'native-cli',
    title: item.title || item.nativeSessionId,
    processState: 'stopped',
    attentionState: 'none',
    lastActivityAt: Number.isFinite(updated) ? updated : 0,
    archived: false,
    resumable: true,
    adapterSessionId: item.sessionKey,
    nativeSessionId: item.nativeSessionId,
    launchConfigId: entry.context.profileId,
    safeErrorCode: null,
    renameState: 'idle',
  }
}

function parseHistoryId(id: string): { adapterSessionId: string; nativeSessionId: string; projectPath: string } {
  if (!id.startsWith(HISTORY_PREFIX)) throw new Error('NATIVE_HISTORY_SESSION_REQUIRED')
  let value: unknown
  try { value = JSON.parse(id.slice(HISTORY_PREFIX.length)) } catch { throw new Error('NATIVE_HISTORY_SESSION_REQUIRED') }
  if (!Array.isArray(value) || value.length !== 6 || value[0] !== 'cc-desk-session-v1' || value[1] !== 'native-cli') {
    throw new Error('NATIVE_HISTORY_SESSION_REQUIRED')
  }
  const [, , , projectPath, adapterSessionId, nativeSessionId] = value
  if (typeof projectPath !== 'string' || typeof adapterSessionId !== 'string' || typeof nativeSessionId !== 'string') {
    throw new Error('NATIVE_HISTORY_SESSION_REQUIRED')
  }
  return { projectPath, adapterSessionId, nativeSessionId }
}

export function createNativeCliAdapter(deps: NativeCliAdapterDeps): SessionAdapter {
  function requireTab(id: string): NativeCliTab {
    const tab = deps.tabs.tabs.get(activeTabId(id))
    if (!tab) throw new Error('NATIVE_SESSION_NOT_FOUND')
    return tab
  }

  async function listSessions(projectKey?: string): Promise<UnifiedSession[]> {
    const wanted = projectKey == null ? null : normalizePath(projectKey)
    const tabs = [...deps.tabs.tabs.values()].filter(tab => wanted == null || normalizePath(tab.projectPath) === wanted)
    const claimed = new Set(tabs.map(tab => {
      const id = tabNativeSessionId(tab)
      return id ? JSON.stringify([tab.cli, tab.profileId, tab.profileRevision, tab.projectId, normalizePath(tab.projectPath), id]) : null
    }).filter((value): value is string => value !== null))
    const sessions = tabs.map(projectTab)
    for (const entry of deps.history.all()) {
      if (wanted != null && normalizePath(entry.context.projectPath) !== wanted) continue
      for (const item of entry.sessions) {
        const claim = JSON.stringify([entry.context.cli, entry.context.profileId, entry.context.profileRevision, entry.context.projectId, normalizePath(entry.context.projectPath), item.nativeSessionId])
        if (!claimed.has(claim)) sessions.push(projectHistory(entry, item))
      }
    }
    return sessions.sort((a, b) => b.lastActivityAt - a.lastActivityAt || a.id.localeCompare(b.id))
  }

  async function createSession(input: CreateUnifiedSessionInput): Promise<UnifiedSession> {
    const action = input.action ?? { kind: 'new' as const }
    const tab = await deps.runtime.createTab({ ...input, action })
    deps.tabs.setActive(tab.tabId)
    return projectTab(tab)
  }

  async function resumeSession(input: ResumeUnifiedSessionInput): Promise<UnifiedSession> {
    const nativeSessionId = input.nativeSessionId ?? input.adapterSessionId
    const existing = [...deps.tabs.tabs.values()].find(tab =>
      tab.cli === input.cli
      && normalizePath(tab.projectPath) === normalizePath(input.projectPath)
      && tabNativeSessionId(tab) === nativeSessionId,
    )
    if (existing) {
      deps.tabs.setActive(existing.tabId)
      return projectTab(existing)
    }
    return createSession({
      projectKey: input.projectKey,
      projectPath: input.projectPath,
      cli: input.cli,
      launchConfigId: input.launchConfigId,
      title: input.title,
      action: { kind: 'resume-id', nativeSessionId },
    })
  }

  async function activateSession(id: string): Promise<void> { deps.tabs.setActive(requireTab(id).tabId) }
  async function stopSession(id: string): Promise<void> { await deps.runtime.stopTab(requireTab(id)) }
  async function restartSession(id: string): Promise<UnifiedSession> {
    const current = requireTab(id)
    if (current.status === 'unknown') throw new Error('LAUNCH_STATE_UNKNOWN')
    return projectTab(await deps.runtime.restartTab(current.tabId))
  }
  async function closeSession(id: string): Promise<void> { deps.tabs.close(requireTab(id).tabId) }
  async function renameSession(id: string, title: string): Promise<void> { deps.tabs.rename(requireTab(id).tabId, title) }
  async function archiveSession(id: string): Promise<void> {
    if (id.startsWith(ACTIVE_PREFIX)) {
      const tab = requireTab(id)
      const nativeSessionId = tabNativeSessionId(tab)
      if (!nativeSessionId) throw new Error('SESSION_NOT_RESUMABLE')
      if (tab.status === 'running' || tab.status === 'starting') await deps.runtime.stopTab(tab)
      await deps.archive.archiveSession(tab.projectPath, nativeSessionId)
      deps.tabs.close(tab.tabId)
      return
    }
    const history = parseHistoryId(id)
    await deps.archive.archiveSession(history.projectPath, history.nativeSessionId)
  }
  async function restoreArchivedSession(id: string): Promise<void> {
    const history = parseHistoryId(id)
    await deps.archive.restoreSession(history.projectPath, history.nativeSessionId)
  }

  return { runtime: 'native-cli', listSessions, createSession, resumeSession, activateSession, stopSession, restartSession, closeSession, renameSession, archiveSession, restoreArchivedSession }
}