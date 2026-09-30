import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { useProjectsStateStore } from '@/stores/projectsState'
import type {
  CreateUnifiedSessionInput,
  ResumeUnifiedSessionInput,
  SessionAdapter,
  SessionRuntimeKind,
  UnifiedProjectGroup,
  UnifiedSession,
} from '@/types/unifiedSession'
import { createNativeId } from '@/utils/nativeId'
import { normalizePath } from '@/utils/path'

function projectName(path: string): string {
  const normalized = path.replace(/[\\/]+$/, '')
  const parts = normalized.split(/[\\/]/).filter(Boolean)
  return parts.length ? parts[parts.length - 1] : path
}

function mergeDuplicateSession(existing: UnifiedSession | undefined, candidate: UnifiedSession): UnifiedSession {
  const normalized: UnifiedSession = {
    ...candidate,
    projectKey: normalizePath(candidate.projectPath),
  }
  if (!existing) return normalized

  // The newest projection wins, except attention is sticky across duplicate projections.
  const newest = normalized.lastActivityAt >= existing.lastActivityAt ? normalized : existing
  return {
    ...newest,
    attentionState:
      existing.attentionState === 'needs-user' || normalized.attentionState === 'needs-user'
        ? 'needs-user'
        : 'none',
  }
}

export const useUnifiedSessionsStore = defineStore('unified-sessions', () => {
  const projects = useProjectsStateStore()
  const sessions = ref<UnifiedSession[]>([])
  const activeSessionId = ref<string | null>(null)
  const initialized = ref(false)
  const loading = ref(false)
  const error = ref<string | null>(null)

  let adapters: SessionAdapter[] = []
  let prepareCreation: (input: CreateUnifiedSessionInput) => Promise<CreateUnifiedSessionInput> = async input => input
  const creations = new Map<string, { input: CreateUnifiedSessionInput; row: UnifiedSession; owner: object; preparing: boolean; selectionIntentEpoch: number }>()
  function configureCreationPreparer(prepare: typeof prepareCreation) { prepareCreation = prepare }
  function isPreparingSession(id: string) { return creations.has(id) }
  function copyInput(input: CreateUnifiedSessionInput): CreateUnifiedSessionInput {
    return { ...input, action: input.action?.kind === 'raw' ? { kind: 'raw', argv: [...input.action.argv] } : input.action ? { ...input.action } : undefined }
  }
  let refreshVersion = 0
  let fullRefreshVersion = 0
  let pendingRefreshes = 0
  const projectRefreshVersions = new Map<string, number>()
  let selectionEpoch = 0
  // Selection intent is distinct from lifecycle invalidation: closing an unrelated
  // row must not revoke the selected preparation's eventual identity transfer.
  let selectionIntentEpoch = 0
  const actionVersion = new Map<string, number>()
  const actionTails = new Map<string, Promise<void>>()

  const activeSession = computed(() =>
    sessions.value.find(session => session.id === activeSessionId.value) ?? null,
  )

  const projectGroups = computed<UnifiedProjectGroup[]>(() => {
    const groups = new Map<string, UnifiedProjectGroup>()
    const pinned = new Set(projects.pinnedProjects.map(normalizePath))

    for (const session of sessions.value) {
      if (session.archived) continue
      const key = normalizePath(session.projectPath)
      let group = groups.get(key)
      if (!group) {
        const alias = projects.displayNames.get(key) ?? projects.displayNames.get(session.projectPath)
        group = {
          projectKey: key,
          projectPath: session.projectPath,
          name: alias || projectName(session.projectPath),
          sessions: [],
          pinned: pinned.has(key),
          hidden: false,
          runningCount: 0,
          needsUserCount: 0,
          lastActivityAt: 0,
        }
        groups.set(key, group)
      }
      group.sessions.push(session)
      if (session.processState === 'running' || session.processState === 'starting') {
        group.runningCount += 1
      }
      if (session.attentionState === 'needs-user') group.needsUserCount += 1
      group.lastActivityAt = Math.max(group.lastActivityAt, session.lastActivityAt)
    }

    for (const group of groups.values()) {
      group.sessions.sort((a, b) =>
        b.lastActivityAt - a.lastActivityAt || a.id.localeCompare(b.id),
      )
    }

    return [...groups.values()].sort((a, b) =>
      Number(b.pinned) - Number(a.pinned)
      || b.lastActivityAt - a.lastActivityAt
      || a.name.localeCompare(b.name),
    )
  })

  function configureAdapters(next: SessionAdapter[]): void {
    const seen = new Set<SessionRuntimeKind>()
    for (const adapter of next) {
      if (seen.has(adapter.runtime)) throw new Error('DUPLICATE_SESSION_ADAPTER')
      seen.add(adapter.runtime)
    }
    adapters = [...next]
  }

  function adapterForRuntime(runtime: SessionRuntimeKind): SessionAdapter {
    const adapter = adapters.find(candidate => candidate.runtime === runtime)
    if (!adapter) throw new Error('SESSION_ADAPTER_UNAVAILABLE')
    return adapter
  }

  function adapterForResume(input: ResumeUnifiedSessionInput): SessionAdapter {
    const matches = sessions.value.filter(session =>
      session.cli === input.cli
      && normalizePath(session.projectPath) === normalizePath(input.projectPath)
      && session.adapterSessionId === input.adapterSessionId
      && (input.runtime === undefined || session.runtime === input.runtime),
    )
    const runtimes = new Set(matches.map(session => session.runtime))
    if (runtimes.size > 1) throw new Error('SESSION_ORIGIN_AMBIGUOUS')
    return adapterForRuntime(matches[0]?.runtime ?? input.runtime ?? 'native-cli')
  }

  async function refresh(projectKey?: string): Promise<void> {
    const version = ++refreshVersion
    const key = projectKey === undefined ? undefined : normalizePath(projectKey)
    if (key === undefined) fullRefreshVersion = version
    else projectRefreshVersions.set(key, version)
    ++pendingRefreshes
    loading.value = true
    error.value = null

    const ownsProject = (candidate: string): boolean =>
      fullRefreshVersion <= version
      && (projectRefreshVersions.get(candidate) ?? 0) <= version

    try {
      const lists = await Promise.all(adapters.map(adapter => adapter.listSessions(projectKey)))
      if (fullRefreshVersion > version || (key !== undefined && !ownsProject(key))) return

      // Replace only the requested project. A full read also preserves scopes
      // refreshed after it began, regardless of completion order.
      const byId = new Map<string, UnifiedSession>(sessions.value
        .filter(session => {
          const sessionKey = normalizePath(session.projectPath)
          return key !== undefined ? sessionKey !== key : !ownsProject(sessionKey)
        })
        .map(session => [session.id, session]),
      )
      for (const creation of creations.values()) byId.set(creation.row.id, creation.row)
      for (const session of lists.flat()) {
        const sessionKey = normalizePath(session.projectPath)
        if ((key !== undefined && key !== sessionKey) || !ownsProject(sessionKey)) continue
        byId.set(session.id, mergeDuplicateSession(byId.get(session.id), session))
      }
      sessions.value = [...byId.values()].sort((a, b) =>
        b.lastActivityAt - a.lastActivityAt || a.id.localeCompare(b.id),
      )
      if (activeSessionId.value && !byId.has(activeSessionId.value)) {
        activeSessionId.value = null
      }
    } catch (failure) {
      if (refreshVersion === version) {
        error.value = failure instanceof Error ? failure.message : 'SESSION_REFRESH_FAILED'
      }
      throw failure
    } finally {
      loading.value = --pendingRefreshes > 0
    }
  }

  async function initialize(): Promise<void> {
    if (initialized.value) return
    await projects.ensureLoaded()
    await refresh()
    initialized.value = true
  }

  async function activateSession(id: string): Promise<void> {
    const session = requireSession(id)
    const epoch = ++selectionEpoch
    const intentEpoch = ++selectionIntentEpoch
    const creation = creations.get(id)
    if (creation) creation.selectionIntentEpoch = intentEpoch
    else await adapterForRuntime(session.runtime).activateSession(id)
    if (epoch === selectionEpoch && sessions.value.some(value => value.id === id)) {
      activeSessionId.value = id
    }
  }

  async function createSession(input: CreateUnifiedSessionInput, retryId?: string): Promise<UnifiedSession> {
    ++selectionEpoch
    const intentEpoch = ++selectionIntentEpoch
    const id = retryId ?? createNativeId('preparing')
    const owner = {}
    const row: UnifiedSession = { id, projectKey: normalizePath(input.projectPath), projectPath: input.projectPath,
      cli: input.cli, runtime: 'native-cli', title: input.title || (input.cli === 'claude' ? 'Claude Code' : 'Codex CLI'),
      processState: 'starting', attentionState: 'none', lastActivityAt: Date.now(), archived: false, resumable: false, adapterSessionId: id }
    const creation = { input: copyInput(input), row, owner, preparing: true, selectionIntentEpoch: intentEpoch }
    creations.set(id, creation)
    sessions.value = [...sessions.value.filter(session => session.id !== id), row]
    activeSessionId.value = id
    const current = () => creations.get(id)?.owner === owner
    try {
      const prepared = await prepareCreation(copyInput(creation.input))
      if (!current()) throw new Error('NEW_SESSION_CANCELLED')
      creation.preparing = false
      const created = await adapterForRuntime('native-cli').createSession(prepared)
      creations.delete(id)
      sessions.value = [...sessions.value.filter(session => session.id !== id && session.id !== created.id), created]
      if (creation.selectionIntentEpoch === selectionIntentEpoch && activeSessionId.value === id) activeSessionId.value = created.id
      // Admission is not CLI success. A failed catalog read cannot turn an
      // admitted attempt into a retryable preparation row.
      await refresh(creation.input.projectKey).catch(() => { error.value = 'SESSION_REFRESH_FAILED' })
      return sessions.value.find(value => value.id === created.id) ?? created
    } catch {
      if (!current()) throw new Error('NEW_SESSION_CANCELLED')
      row.processState = creation.preparing ? 'failed' : 'unknown'
      row.safeErrorCode = creation.preparing ? 'NEW_SESSION_PREPARATION_FAILED' : 'LAUNCH_STATE_UNKNOWN'
      sessions.value = sessions.value.map(session => session.id === id ? { ...row } : session)
      throw new Error(row.safeErrorCode)
    }
  }

  async function resumeSession(input: ResumeUnifiedSessionInput): Promise<UnifiedSession> {
    const epoch = ++selectionEpoch
    ++selectionIntentEpoch
    const resumed = await adapterForResume(input).resumeSession(input)
    await refresh(input.projectKey)
    if (epoch === selectionEpoch) activeSessionId.value = resumed.id
    return sessions.value.find(value => value.id === resumed.id) ?? resumed
  }

  function enqueue<T>(
    id: string,
    operation: () => Promise<T>,
    publish?: (value: T) => Promise<void> | void,
    operationKind?: 'close' | 'archive',
  ): Promise<T> {
    const version = (actionVersion.get(id) ?? 0) + 1
    actionVersion.set(id, version)
    const session = requireSession(id)
    const owns = adapterForRuntime(session.runtime).captureOwnership?.(id, operationKind) ?? (() => true)
    const previous = actionTails.get(id) ?? Promise.resolve()

    let resolveValue!: (value: T | PromiseLike<T>) => void
    let rejectValue!: (reason?: unknown) => void
    const result = new Promise<T>((resolve, reject) => {
      resolveValue = resolve
      rejectValue = reject
    })

    const task = previous.catch(() => undefined).then(async () => {
      try {
        if (!owns()) throw new Error('STALE_SESSION_ATTEMPT')
        const value = await operation()
        if (actionVersion.get(id) === version && publish) await publish(value)
        resolveValue(value)
      } catch (failure) {
        rejectValue(failure)
      }
    })
    const tail = task.then(() => undefined, () => undefined)
    actionTails.set(id, tail)
    void tail.finally(() => {
      if (actionTails.get(id) === tail) actionTails.delete(id)
    })
    return result
  }

  function requireSession(id: string): UnifiedSession {
    const session = sessions.value.find(value => value.id === id)
    if (!session) throw new Error('SESSION_NOT_FOUND')
    return session
  }

  function stopSession(id: string): Promise<void> {
    const creation = creations.get(id)
    if (creation?.preparing) {
      creation.owner = {}; creation.row.processState = 'failed'; creation.row.safeErrorCode = 'NEW_SESSION_CANCELLED'
      sessions.value = sessions.value.map(row => row.id === id ? { ...creation.row } : row)
      return Promise.resolve()
    }
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    return enqueue(id, () => adapter.stopSession(id), () => refresh(session.projectKey))
  }

  function restartSession(id: string): Promise<UnifiedSession> {
    const creation = creations.get(id)
    if (creation) {
      if (creation.row.processState !== 'failed') return Promise.reject(new Error('LAUNCH_STATE_UNKNOWN'))
      return createSession(copyInput(creation.input), id)
    }
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    return enqueue(id, () => adapter.restartSession(id), async restarted => {
      await refresh(session.projectKey)
      if (activeSessionId.value === id) activeSessionId.value = restarted.id
    })
  }

  function closeSession(id: string): Promise<void> {
    const creation = creations.get(id)
    if (creation) {
      if (!creation.preparing && creation.row.processState === 'starting') return Promise.reject(new Error('NEW_SESSION_ADMISSION_IN_PROGRESS'))
      if (creation.row.processState === 'unknown') return Promise.reject(new Error('LAUNCH_STATE_UNKNOWN'))
      creations.delete(id); sessions.value = sessions.value.filter(row => row.id !== id)
      if (activeSessionId.value === id) activeSessionId.value = null
      ++selectionEpoch
      return Promise.resolve()
    }
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    ++selectionEpoch
    return enqueue(id, () => adapter.closeSession(id), async () => {
      if (activeSessionId.value === id) activeSessionId.value = null
      await refresh(session.projectKey)
    }, 'close')
  }

  function renameSession(id: string, title: string): Promise<void> {
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    return enqueue(id, () => adapter.renameSession(id, title), () => refresh(session.projectKey))
  }

  function archiveSession(id: string): Promise<void> {
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    ++selectionEpoch
    return enqueue(id, () => adapter.archiveSession(id), async () => {
      if (activeSessionId.value === id) activeSessionId.value = null
      await refresh(session.projectKey)
    }, 'archive')
  }

  function restoreArchivedSession(id: string): Promise<void> {
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    return enqueue(
      id,
      () => adapter.restoreArchivedSession(id),
      () => refresh(session.projectKey),
    )
  }

  return {
    sessions,
    projectGroups,
    activeSessionId,
    activeSession,
    initialized,
    loading,
    error,
    configureAdapters,
    configureCreationPreparer,
    isPreparingSession,
    initialize,
    refresh,
    activateSession,
    createSession,
    resumeSession,
    stopSession,
    restartSession,
    closeSession,
    renameSession,
    archiveSession,
    restoreArchivedSession,
  }
})
