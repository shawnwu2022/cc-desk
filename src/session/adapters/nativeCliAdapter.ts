import type { LaunchAction } from '@/types/cli'
import { captureNativeAttempt, matchesNativeAttempt, type NativeAttemptIdentity, type NativeCliTab } from '@/stores/nativeTabs'
import type { NativeObservationNotice, NativeObservationNoticeState } from '@/types/nativeObservationNotice'
import { nativeHistoryContextKey, type NativeHistoryContext, type NativeHistoryEntry } from '@/stores/nativeHistory'
import type {
  CreateUnifiedSessionInput,
  ResumeUnifiedSessionInput,
  SessionAdapter,
  UnifiedSession,
} from '@/types/unifiedSession'
import { makeSessionCatalogKey } from '@/utils/sessionPresentation'
import { normalizePath } from '@/utils/path'
import { saveSessionDisplayName, withSessionDisplayName, type SessionMetadataPort } from '@/session/sessionMetadata'

const ACTIVE_PREFIX = 'native-tab:'
const HISTORY_PREFIX = 'native-history:'

export interface NativeTabsPort {
  readonly tabs: Map<string, NativeCliTab>
  readonly activeTabId: string | null
  hasOwnedObservationNotice?(tabId: string, attempt: NativeAttemptIdentity): boolean
  setActive(tabId: string | null): void
  close(tabId: string): void
  rename(tabId: string, title: string): void
}

export interface NativeHistoryPort {
  all(): NativeHistoryEntry[]
  load?(input: NativeHistoryContext): Promise<NativeHistoryEntry>
}

export interface NativeRuntimeCreateInput extends CreateUnifiedSessionInput {
  action: LaunchAction
  sourceSessionKey?: string
  sourceContext?: NativeHistoryEntry['context']
}
export interface NativeRuntimePort {
  createTab(input: NativeRuntimeCreateInput): Promise<NativeCliTab> | NativeCliTab
  restartTab(tabId: string): Promise<NativeCliTab> | NativeCliTab
  stopTab(tab: NativeCliTab): Promise<void>
}

export interface NativeArchivePort {
  getArchivedSessions(projectPath: string): string[]
  archiveSession(projectPath: string, sessionId: string, beforeMutation?: () => void): Promise<unknown>
  restoreSession(projectPath: string, sessionId: string): Promise<unknown>
}

export interface NativeCliAdapterDeps {
  tabs: NativeTabsPort
  history: NativeHistoryPort
  runtime: NativeRuntimePort
  archive: NativeArchivePort
  metadata?: SessionMetadataPort
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

function projectNotice(tab: NativeCliTab, tabs: NativeTabsPort): NativeObservationNoticeState | undefined {
  if (tab.cli !== 'claude' || tab.action.kind === 'raw' || tab.status !== 'running'
    || !tabs.hasOwnedObservationNotice?.(tab.tabId, captureNativeAttempt(tab))) return
  const copy = (notice: NativeObservationNotice | null | undefined): NativeObservationNotice | null => {
    if (!notice || notice.runId !== tab.runId || notice.generation !== tab.generation) return null
    return { kind: notice.kind, eventId: notice.eventId, receivedAt: notice.receivedAt, runId: notice.runId, generation: notice.generation }
  }
  const recent = copy(tab.observationNotice?.recent)
  const unreadReplyEnd = tab.observationNotice?.unreadReplyEnd?.kind === 'reply-ended' ? copy(tab.observationNotice.unreadReplyEnd) : null
  return recent || unreadReplyEnd ? { recent, unreadReplyEnd } : undefined
}

function projectTab(tab: NativeCliTab, tabs?: NativeTabsPort): UnifiedSession {
  const nativeSessionId = tabNativeSessionId(tab)
  // The existing optional observer is a Claude-only non-raw side channel.
  // Missing/inactive observations cannot establish current turn activity.
  const claudeObservation = tab.cli === 'claude' && tab.action.kind !== 'raw'
  const activeObservation = claudeObservation && tab.status === 'running' && tab.observationState === 'active'
  const observationNotice = tabs && projectNotice(tab, tabs)
  return {
    id: activeId(tab.tabId),
    projectKey: normalizePath(tab.projectPath),
    projectPath: tab.projectPath,
    cli: tab.cli,
    runtime: 'native-cli',
    title: tab.title,
    processState: tab.status === 'stopped' && tab.launchRevision === null ? 'starting' : processState(tab.status),
    attentionState: activeObservation ? tab.attentionState ?? 'none' : 'none',
    activityState: activeObservation ? tab.activityState ?? 'unknown' : 'unknown',
    observationState: claudeObservation ? tab.observationState ?? 'off' : 'off',
    ...(observationNotice ? { observationNotice } : {}),
    lastActivityAt: tab.lastActivityAt,
    archived: false,
    opened: true,
    resumable: Boolean(nativeSessionId),
    adapterSessionId: tab.tabId,
    nativeSessionId,
    launchConfigId: tab.profileId,
    nativeOrigin: { cli: tab.cli, profileId: tab.profileId, profileRevision: tab.profileRevision, projectId: tab.projectId, projectPath: tab.projectPath },
    safeErrorCode: tab.errorCode,
    renameState: 'idle',
  }
}

function oldHistoryId(entry: NativeHistoryEntry, sessionKey: string, nativeSessionId: string): string {
  return `${HISTORY_PREFIX}${makeSessionCatalogKey({
    runtime: 'native-cli',
    cli: entry.context.cli,
    projectPath: entry.context.projectPath,
    adapterSessionId: sessionKey,
    nativeSessionId,
  })}`
}

function historyId(entry: Pick<NativeHistoryEntry, 'context'>, sessionKey: string, nativeSessionId: string): string {
  return `${HISTORY_PREFIX}${JSON.stringify(['native-history-v2', nativeHistoryContextKey(entry.context), sessionKey, nativeSessionId])}`
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
    activityState: 'unknown',
    observationState: 'off',
    lastActivityAt: Number.isFinite(updated) ? updated : 0,
    archived: false,
    opened: false,
    resumable: true,
    adapterSessionId: item.sessionKey,
    nativeSessionId: item.nativeSessionId,
    launchConfigId: entry.context.profileId,
    nativeOrigin: { ...entry.context },
    safeErrorCode: null,
    renameState: 'idle',
  }
}

function parseHistoryId(id: string): { adapterSessionId: string; nativeSessionId: string; projectPath: string } {
  if (!id.startsWith(HISTORY_PREFIX)) throw new Error('NATIVE_HISTORY_SESSION_REQUIRED')
  let value: unknown
  try { value = JSON.parse(id.slice(HISTORY_PREFIX.length)) } catch { throw new Error('NATIVE_HISTORY_SESSION_REQUIRED') }
  if (Array.isArray(value) && value[0] === 'native-history-v2' && value.length === 4) {
    try {
      const context = JSON.parse(value[1])
      if (context[0] === 'native-history-v1' && typeof context[5] === 'string' && typeof value[2] === 'string' && typeof value[3] === 'string') {
        return { projectPath: context[5], adapterSessionId: value[2], nativeSessionId: value[3] }
      }
    } catch { /* Reject malformed identity below. */ }
    throw new Error('NATIVE_HISTORY_SESSION_REQUIRED')
  }
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
  function findHistory(id: string) {
    return deps.history.all().flatMap(entry => entry.sessions.filter(item => historyId(entry, item.sessionKey, item.nativeSessionId) === id).map(item => ({ entry, item })))
  }
  function tabDisplayIdentity(tab: NativeCliTab): UnifiedSession {
    const row = projectTab(tab)
    if (tab.sourceSessionKey && row.nativeSessionId) {
      return { ...row, id: historyId({ context: row.nativeOrigin! }, tab.sourceSessionKey, row.nativeSessionId), adapterSessionId: tab.sourceSessionKey }
    }
    const matches = historyForTab(tab)
    return matches.length === 1 ? projectHistory(matches[0].entry, matches[0].item) : row
  }
  function projectActive(tab: NativeCliTab) { return withSessionDisplayName(projectTab(tab, deps.tabs), deps.metadata, tabDisplayIdentity(tab)) }
  const admissions = new Map<string, { promise: Promise<UnifiedSession>; owners: Set<() => boolean> }>()
  function coalesce(key: string, operation: (canAdmit: () => boolean) => Promise<UnifiedSession>, canAdmit: () => boolean) {
    const ownResult = (promise: Promise<UnifiedSession>) => promise.then(value => {
      if (!canAdmit()) throw new Error('RESTORE_CANCELLED')
      return value
    })
    const current = admissions.get(key)
    if (current) { current.owners.add(canAdmit); return ownResult(current.promise) }
    // Share the source check, not the first caller's cancellation lifetime. A new
    // explicit confirmation can own admission after an older dialog was closed.
    const owners = new Set([canAdmit])
    const next = operation(() => [...owners].some(isCurrent => isCurrent()))
    admissions.set(key, { promise: next, owners })
    void next.then(() => { if (admissions.get(key)?.promise === next) admissions.delete(key) }, () => { if (admissions.get(key)?.promise === next) admissions.delete(key) })
    return ownResult(next)
  }
  function oldKeyMatches(key: string) {
    return deps.history.all().flatMap(entry => entry.sessions.filter(item => oldHistoryId(entry, item.sessionKey, item.nativeSessionId) === key)
      .map(item => ({ entry, item })))
  }
  function archiveState(entry: NativeHistoryEntry, item: NativeHistoryEntry['sessions'][number]) {
    const keys = deps.archive.getArchivedSessions(entry.context.projectPath)
    const old = oldHistoryId(entry, item.sessionKey, item.nativeSessionId)
    const legacy = keys.includes(old)
    return { archived: keys.includes(historyId(entry, item.sessionKey, item.nativeSessionId)) || legacy,
      safeErrorCode: legacy && oldKeyMatches(old).length !== 1 ? 'SESSION_ORIGIN_AMBIGUOUS' : null }
  }

  function requireTab(id: string): NativeCliTab {
    const tab = deps.tabs.tabs.get(activeTabId(id))
    if (!tab) throw new Error('NATIVE_SESSION_NOT_FOUND')
    return tab
  }

  function historyForTab(tab: NativeCliTab) {
    return deps.history.all().flatMap(entry => {
      if (entry.context.cli !== tab.cli || entry.context.profileId !== tab.profileId
        || entry.context.profileRevision !== tab.profileRevision || entry.context.projectId !== tab.projectId
        || normalizePath(entry.context.projectPath) !== normalizePath(tab.projectPath)) return []
      return entry.sessions.filter(item => item.nativeSessionId === tabNativeSessionId(tab)
        && (!tab.sourceSessionKey || item.sessionKey === tab.sourceSessionKey))
        .map(item => ({ entry, item }))
    })
  }

  async function listSessions(projectKey?: string): Promise<UnifiedSession[]> {
    const wanted = projectKey == null ? null : normalizePath(projectKey)
    const tabs = [...deps.tabs.tabs.values()].filter(tab => wanted == null || normalizePath(tab.projectPath) === wanted)
    const claimed = new Set(tabs.flatMap(tab => {
      const matches = historyForTab(tab)
      return matches.length === 1
        ? [historyId(matches[0].entry, matches[0].item.sessionKey, matches[0].item.nativeSessionId)]
        : []
    }))
    const sessions = tabs.map(projectActive)
    for (const entry of deps.history.all()) {
      if (wanted != null && normalizePath(entry.context.projectPath) !== wanted) continue
      for (const item of entry.sessions) {
        const claim = historyId(entry, item.sessionKey, item.nativeSessionId)
        if (!claimed.has(claim)) sessions.push({
          ...withSessionDisplayName(projectHistory(entry, item), deps.metadata),
          ...archiveState(entry, item),
        })
      }
    }
    return sessions.sort((a, b) => b.lastActivityAt - a.lastActivityAt || a.id.localeCompare(b.id))
  }

  async function createSession(input: CreateUnifiedSessionInput, canAdmit = () => true): Promise<UnifiedSession> {
    const action = input.action ?? { kind: 'new' as const }
    if (action.kind === 'resume-id') {
      const candidates = deps.history.all().filter(entry => entry.context.cli === input.cli && entry.context.profileId === input.launchConfigId
        && entry.context.profileRevision === input.launchConfigRevision && (!input.registeredProjectId || entry.context.projectId === input.registeredProjectId) && normalizePath(entry.context.projectPath) === normalizePath(input.projectPath))
        .flatMap(entry => entry.sessions.filter(item => item.nativeSessionId === action.nativeSessionId).map(item => ({ entry, item })))
      if (candidates.length > 1) throw new Error('SESSION_ORIGIN_AMBIGUOUS')
      if (candidates.length === 1) {
        const { entry, item } = candidates[0]
        return resumeSession({ ...input, adapterSessionId: item.sessionKey, nativeSessionId: item.nativeSessionId, nativeOrigin: entry.context }, canAdmit)
      }
    }
    const create = async (owns = canAdmit) => {
      if (action.kind === 'resume-id') {
        const open = [...deps.tabs.tabs.values()].filter(tab => tab.cli === input.cli && tab.profileId === input.launchConfigId
          && tab.profileRevision === input.launchConfigRevision && (!input.registeredProjectId || tab.projectId === input.registeredProjectId) && normalizePath(tab.projectPath) === normalizePath(input.projectPath)
          && tabNativeSessionId(tab) === action.nativeSessionId)
        if (open.length > 1) throw new Error('SESSION_ORIGIN_AMBIGUOUS')
        if (open[0]) { deps.tabs.setActive(open[0].tabId); return projectActive(open[0]) }
      }
      if (!owns()) throw new Error('RESTORE_CANCELLED')
      const tab = await deps.runtime.createTab({ ...input, action })
      deps.tabs.setActive(tab.tabId)
      return projectActive(tab)
    }
    return action.kind === 'new' || action.kind === 'raw' ? create()
      : coalesce(JSON.stringify(['direct', input.projectPath, input.cli, input.launchConfigId, input.launchConfigRevision, input.registeredProjectId, action]), create, canAdmit)
  }

  function matchingOrigins(input: ResumeUnifiedSessionInput) {
    const candidates = deps.history.all().filter(entry => entry.context.cli === input.cli
      && normalizePath(entry.context.projectPath) === normalizePath(input.projectPath)
      && entry.sessions.some(item => item.sessionKey === input.adapterSessionId && item.nativeSessionId === (input.nativeSessionId ?? input.adapterSessionId)))
    const matches = candidates.filter(entry => (!input.launchConfigId || entry.context.profileId === input.launchConfigId)
      && (!input.nativeOrigin || nativeHistoryContextKey(entry.context) === nativeHistoryContextKey(input.nativeOrigin)))
    if (candidates.length && !matches.length && !input.nativeOrigin) throw new Error('PROFILE_SELECTION_CHANGED')
    if (new Set(matches.map(entry => nativeHistoryContextKey(entry.context))).size > 1) throw new Error('SESSION_ORIGIN_AMBIGUOUS')
    return matches[0]?.context ?? input.nativeOrigin
  }
  function existingFor(input: ResumeUnifiedSessionInput, origin = matchingOrigins(input)) {
    const matches = [...deps.tabs.tabs.values()].filter(tab => tab.cli === input.cli
      && normalizePath(tab.projectPath) === normalizePath(input.projectPath)
      && tabNativeSessionId(tab) === (input.nativeSessionId ?? input.adapterSessionId)
      && (!input.launchConfigId || tab.profileId === input.launchConfigId)
      && (!origin || nativeHistoryContextKey(origin) === nativeHistoryContextKey({ cli: tab.cli, profileId: tab.profileId, profileRevision: tab.profileRevision, projectId: tab.projectId, projectPath: tab.projectPath }))
      && (!tab.sourceSessionKey || tab.sourceSessionKey === input.adapterSessionId)
      && historyForTab(tab).every(({ item }) => item.sessionKey === input.adapterSessionId))
    if (matches.length > 1) throw new Error('SESSION_ORIGIN_AMBIGUOUS')
    return matches[0]
  }
  async function verify(input: ResumeUnifiedSessionInput, missing: boolean) {
    const origin = matchingOrigins(input)
    if (!origin) throw new Error('SOURCE_UNAVAILABLE')
    const entry = deps.history.load ? await deps.history.load({ ...origin, force: true })
      : deps.history.all().find(entry => nativeHistoryContextKey(entry.context) === nativeHistoryContextKey(origin))
    if (!entry?.loaded || entry.loading || entry.error) throw new Error(entry?.error || 'SOURCE_UNAVAILABLE')
    const present = entry.sessions.some(item => item.sessionKey === input.adapterSessionId && item.nativeSessionId === (input.nativeSessionId ?? input.adapterSessionId))
    if (missing && (present || existingFor(input, origin))) throw new Error('SESSION_EXISTS')
    if (!present) {
      const proof = entry.absenceEvidence
      if (!proof) throw new Error('HISTORY_ABSENCE_UNVERIFIED')
      let identity: unknown
      try { identity = JSON.parse(input.adapterSessionId) } catch { throw new Error('HISTORY_ABSENCE_UNVERIFIED') }
      if (!Array.isArray(identity) || identity.length !== 4 || identity[0] !== 'local'
        || identity[1] !== input.cli || identity[3] !== (input.nativeSessionId ?? input.adapterSessionId)
        || typeof identity[2] !== 'string' || !identity[2]) throw new Error('HISTORY_ABSENCE_UNVERIFIED')
      if (proof.cli !== input.cli || proof.sourceRootKey !== identity[2]) throw new Error('SOURCE_CHANGED')
      if (!missing) throw new Error('SESSION_NOT_FOUND')
    }
    return origin
  }
  function resumeSession(input: ResumeUnifiedSessionInput, canAdmit = () => true): Promise<UnifiedSession> {
    let origin: ReturnType<typeof matchingOrigins>
    try { origin = matchingOrigins(input) } catch (error) { return Promise.reject(error) }
    const key = JSON.stringify([origin ? nativeHistoryContextKey(origin) : null, input.cli, normalizePath(input.projectPath), input.adapterSessionId, input.nativeSessionId])
    return coalesce(key, async owns => {
      const existing = existingFor(input, origin)
      if (existing) { deps.tabs.setActive(existing.tabId); return projectActive(existing) }
      if (!origin) throw new Error('SESSION_NOT_FOUND')
      const frozen = { ...input, nativeOrigin: { ...origin } }
      await verify(frozen, false)
      if (!owns()) throw new Error('RESTORE_CANCELLED')
      const admitted = existingFor(frozen, origin)
      if (admitted) { deps.tabs.setActive(admitted.tabId); return projectActive(admitted) }
      const created = await deps.runtime.createTab({
        projectKey: input.projectKey, projectPath: input.projectPath, cli: input.cli,
        launchConfigId: origin.profileId, launchConfigRevision: origin.profileRevision,
        title: input.title, sourceSessionKey: input.adapterSessionId, sourceContext: { ...origin },
        action: { kind: 'resume-id', nativeSessionId: input.nativeSessionId ?? input.adapterSessionId },
      })
      deps.tabs.setActive(created.tabId)
      return projectActive(created)
    }, canAdmit)
  }
  async function verifyMissingSession(input: ResumeUnifiedSessionInput) { await verify(input, true) }

  function captureOwnership(id: string, operation?: 'close' | 'archive'): () => boolean {
    if (!id.startsWith(ACTIVE_PREFIX)) return () => findHistory(id).length === 1
    const tab = requireTab(id)
    const attempt = captureNativeAttempt(tab)
    const source = JSON.stringify([tab.cli, tab.projectId, tab.projectPath, tab.profileId, tab.profileRevision, tab.sourceSessionKey, tab.action])
    const live = (value: NativeCliTab | undefined) => value && ['running', 'starting', 'unknown'].includes(value.status)
    const mustRemainEnded = !!operation && !live(tab)
    return () => {
      const current = deps.tabs.tabs.get(tab.tabId)
      return matchesNativeAttempt(current, attempt) && !!current
        && JSON.stringify([current.cli, current.projectId, current.projectPath, current.profileId, current.profileRevision, current.sourceSessionKey, current.action]) === source
        && (!mustRemainEnded || !live(current))
    }
  }
  async function activateSession(id: string): Promise<void> { deps.tabs.setActive(requireTab(id).tabId) }
  async function stopSession(id: string): Promise<void> { await deps.runtime.stopTab({ ...requireTab(id) }) }
  async function restartSession(id: string, canContinue = () => true): Promise<UnifiedSession> {
    if (!canContinue()) throw new Error('STALE_SESSION_ATTEMPT')
    const current = requireTab(id)
    if (current.status === 'unknown') throw new Error('LAUNCH_STATE_UNKNOWN')
    return projectActive(await deps.runtime.restartTab(current.tabId))
  }
  async function closeSession(id: string, canContinue = () => true): Promise<void> {
    const owns = captureOwnership(id)
    if (!canContinue() || !owns()) throw new Error('STALE_SESSION_ATTEMPT')
    const tab = { ...requireTab(id) }
    const attempt = captureNativeAttempt(tab)
    if (tab.status === 'running' || tab.status === 'starting' || tab.status === 'unknown') await deps.runtime.stopTab(tab)
    if (!matchesNativeAttempt(deps.tabs.tabs.get(tab.tabId), attempt) || !owns() || !canContinue()) throw new Error('STALE_SESSION_ATTEMPT')
    deps.tabs.close(tab.tabId)
  }
  async function renameSession(id: string, title: string, canContinue = () => true, onIssued?: () => void): Promise<void> {
    const owns = captureOwnership(id)
    const current = () => canContinue() && owns()
    if (!current()) throw new Error('STALE_SESSION_ATTEMPT')
    if (id.startsWith(ACTIVE_PREFIX)) {
      const tab = requireTab(id)
      if (deps.metadata) await saveSessionDisplayName(deps.metadata, tabDisplayIdentity(tab), title, current, onIssued)
      if (!current()) throw new Error('STALE_SESSION_ATTEMPT')
      if (!deps.metadata) onIssued?.()
      deps.tabs.rename(tab.tabId, title)
    } else {
      const matches = findHistory(id)
      if (matches.length !== 1 || !deps.metadata) throw new Error('SESSION_NOT_FOUND')
      await saveSessionDisplayName(deps.metadata, projectHistory(matches[0].entry, matches[0].item), title, current, onIssued)
    }
  }
  async function archiveSession(id: string, canContinue = () => true): Promise<void> {
    const owns = captureOwnership(id)
    const requireCurrent = () => { if (!canContinue() || !owns()) throw new Error('STALE_SESSION_ATTEMPT') }
    requireCurrent()
    if (id.startsWith(ACTIVE_PREFIX)) {
      const tab = { ...requireTab(id) }
      const attempt = captureNativeAttempt(tab)
      if (tab.status === 'unknown') throw new Error('LAUNCH_STATE_UNKNOWN')
      const nativeSessionId = tabNativeSessionId(tab)
      if (!nativeSessionId) throw new Error('SESSION_NOT_RESUMABLE')
      const candidates = historyForTab(tab)
      if (candidates.length !== 1) throw new Error('SESSION_ORIGIN_AMBIGUOUS')
      const { entry, item } = candidates[0]
      if (tab.status === 'running' || tab.status === 'starting') await deps.runtime.stopTab(tab)
      if (!matchesNativeAttempt(deps.tabs.tabs.get(tab.tabId), attempt) || !owns() || !canContinue()) throw new Error('STALE_SESSION_ATTEMPT')
      await deps.archive.archiveSession(tab.projectPath, historyId(entry, item.sessionKey, item.nativeSessionId), requireCurrent)
      if (!matchesNativeAttempt(deps.tabs.tabs.get(tab.tabId), attempt) || !owns() || !canContinue()) throw new Error('STALE_SESSION_ATTEMPT')
      deps.tabs.close(tab.tabId)
      return
    }
    const history = parseHistoryId(id)
    await deps.archive.archiveSession(history.projectPath, id, requireCurrent)
  }
  async function restoreArchivedSession(id: string): Promise<void> {
    const history = parseHistoryId(id)
    const match = deps.history.all().flatMap(entry => entry.sessions.filter(item => historyId(entry, item.sessionKey, item.nativeSessionId) === id).map(item => ({ entry, item })))[0]
    if (match) {
      const old = oldHistoryId(match.entry, match.item.sessionKey, match.item.nativeSessionId)
      if (deps.archive.getArchivedSessions(history.projectPath).includes(old)) {
        if (oldKeyMatches(old).length !== 1) throw new Error('SESSION_ORIGIN_AMBIGUOUS')
        await deps.archive.restoreSession(history.projectPath, old)
      }
    }
    if (deps.archive.getArchivedSessions(history.projectPath).includes(id)) await deps.archive.restoreSession(history.projectPath, id)
  }

  return { runtime: 'native-cli', captureOwnership, listSessions, createSession, resumeSession, activateSession, stopSession, restartSession, closeSession, renameSession, archiveSession, restoreArchivedSession, verifyMissingSession }
}
