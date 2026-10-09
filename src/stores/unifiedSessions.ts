import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { useProjectsStateStore } from '@/stores/projectsState'
import type {
  CreateUnifiedSessionInput,
  ResumeUnifiedSessionInput,
  SessionAdapter,
  SessionRuntimeKind,
  UnifiedProjectGroup,
  UnifiedSession,
  ResumeDialogRequest, ResumeHistoryQuery,
} from '@/types/unifiedSession'
import { createNativeId } from '@/utils/nativeId'
import type { SessionConfirmationRequest } from '@/types/confirmation'
import { mapSafeUserError, safeUserErrorCode, type UserErrorPresentation } from '@/utils/userError'
import { useNotificationsStore } from '@/stores/notifications'
import type { ToastInput } from '@/stores/notifications'
import { makeSessionCatalogKey, makeSessionRenameOwnerKey } from '@/utils/sessionPresentation'
import { nativeHistoryContextKey } from '@/stores/nativeHistory'
import { normalizePath } from '@/utils/path'
import { LaunchConfigurationRequiredError } from '@/utils/launchPreparation'

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
  type RenameOwner = { key: string; owns: () => boolean; state: 'editing' | 'saving'; issued?: boolean }
  const renameOwners = new Map<string, RenameOwner>()
  const pendingRenames = new Map<string, number>()
  function captureRenameOwner(row: UnifiedSession, state: RenameOwner['state']): RenameOwner {
    return { key: makeSessionRenameOwnerKey(row), owns: adapters.find(adapter => adapter.runtime === row.runtime)?.captureOwnership?.(row.id) ?? (() => true), state }
  }
  function ownsRename(owner: RenameOwner, row: UnifiedSession | undefined) {
    return !!row && owner.key === makeSessionRenameOwnerKey(row) && owner.owns()
  }
  function revokeUnissuedRenames(selectedId: string | null) {
    const revoked = new Set<string>()
    for (const [id, owner] of renameOwners) {
      if (id !== selectedId && !owner.issued) { renameOwners.delete(id); revoked.add(id) }
    }
    if (revoked.size) sessions.value = sessions.value.map(row => revoked.has(row.id) ? { ...row, renameState: 'idle' } : row)
  }
  watch(activeSessionId, revokeUnissuedRenames, { flush: 'sync' })
  const initialized = ref(false)
  const loading = ref(false)
  const error = ref<string | null>(null)

  const sessionConfirmation = ref<SessionConfirmationRequest | null>(null)
  const confirmationBusy = ref(false)
  const confirmationError = ref<UserErrorPresentation | null>(null)
  const actionFeedback = ref<UserErrorPresentation | null>(null)
  let confirmationOwner: { request: SessionConfirmationRequest; owns: () => boolean; selected: () => boolean } | null = null
  let feedbackVersion = 0
  let recoverForRestart: (id: string, canContinue: () => boolean) => Promise<void> = async () => { throw new Error('LAUNCH_STATE_UNKNOWN') }
  function configureUnknownRestartRecovery(recover: typeof recoverForRestart) { recoverForRestart = recover }
  function captureSelectionOwnership() { const intent = selectionIntentEpoch; return () => intent === selectionIntentEpoch }
  function captureFeedbackOwner() {
    const version = ++feedbackVersion
    const intent = selectionIntentEpoch
    actionFeedback.value = null
    return () => version === feedbackVersion && intent === selectionIntentEpoch
  }
  function clearActionFeedback() { ++feedbackVersion; actionFeedback.value = null }
  function publishActionFailure(current: () => boolean, failure: unknown) {
    if (current()) actionFeedback.value = mapSafeUserError(safeUserErrorCode(failure), 'session')
  }
  function publishActionSuccess(current: () => boolean, key: ToastInput['messageKey']) {
    if (current()) useNotificationsStore().pushToast({ kind: 'success', messageKey: key })
  }
  function closeSessionConfirmation() {
    sessionConfirmation.value = null; confirmationOwner = null; confirmationBusy.value = false; confirmationError.value = null
  }
  function beginSessionConfirmation(kind: SessionConfirmationRequest['kind'], id: string) {
    const row = requireSession(id)
    const request: SessionConfirmationRequest = { kind, sessionId: id, title: row.title }
    const intent = selectionIntentEpoch
    sessionConfirmation.value = request
    // Pinia wraps the request; keep the published identity for comparison.
    confirmationOwner = { request: sessionConfirmation.value, owns: adapterForRuntime(row.runtime).captureOwnership?.(id) ?? (() => false), selected: () => selectionIntentEpoch === intent }
    confirmationError.value = null; confirmationBusy.value = false
  }
  async function confirmSessionAction() {
    const owner = confirmationOwner
    if (!owner || confirmationBusy.value) return
    const current = () => confirmationOwner === owner && sessionConfirmation.value === owner.request && owner.selected()
    const canContinue = () => current() && owner.owns()
    const feedback = captureFeedbackOwner()
    confirmationBusy.value = true; confirmationError.value = null
    try {
      if (!canContinue()) throw new Error('STALE_SESSION_ATTEMPT')
      const { kind, sessionId } = owner.request
      if (kind === 'close-running') await closeSession(sessionId, canContinue)
      else if (kind === 'stop-and-archive') await archiveSession(sessionId, canContinue)
      else {
        await recoverForRestart(sessionId, canContinue)
        if (!canContinue()) throw new Error('STALE_SESSION_ATTEMPT')
        await restartSession(sessionId, canContinue)
      }
      if (current()) {
        if (kind === 'stop-and-archive') publishActionSuccess(feedback, 'feedbackArchived')
        closeSessionConfirmation()
      }
    } catch (failure) {
      if (current() && !owner.owns()) closeSessionConfirmation()
      else if (current()) confirmationError.value = mapSafeUserError(safeUserErrorCode(failure), 'session')
    } finally { if (current()) confirmationBusy.value = false }
  }

  const resumeDialog = ref<ResumeDialogRequest | null>(null)
  const missingRecords = new Map<string, UnifiedSession>()
  let historyLoader: (query: ResumeHistoryQuery) => Promise<boolean> = async () => false
  function configureHistoryLoader(loader: typeof historyLoader) { historyLoader = loader }
  function openResumeDialog(request: ResumeDialogRequest) {
    resumeDialog.value = { ...request, project: { ...request.project } }
  }
  function closeResumeDialog() { resumeDialog.value = null }
  async function searchSessions(query: ResumeHistoryQuery) {
    const frozen = { ...query }
    const partial = await historyLoader(frozen)
    await refresh(frozen.scope === 'all' ? undefined : frozen.projectPath)
    const text = (frozen.query ?? '').trim().toLocaleLowerCase()
    return { partial, sessions: sessions.value.filter(row => !row.archived
      && (frozen.scope === 'all' || normalizePath(row.projectPath) === normalizePath(frozen.projectPath))
      && (!frozen.cli || row.cli === frozen.cli)
      && (!frozen.since || row.lastActivityAt >= frozen.since)
      && (!text || row.title.toLocaleLowerCase().includes(text) || row.nativeSessionId?.toLocaleLowerCase().includes(text))) }
  }
  function resumeInput(row: UnifiedSession): ResumeUnifiedSessionInput {
    return { runtime: row.runtime, cli: row.cli, projectKey: row.projectKey, projectPath: row.projectPath,
      adapterSessionId: row.adapterSessionId, nativeSessionId: row.nativeSessionId, launchConfigId: row.launchConfigId,
      ...(row.nativeOrigin ? { nativeOrigin: { ...row.nativeOrigin } } : {}), title: row.title }
  }
  async function resumeCatalogSession(target: string | UnifiedSession, canAdmit = () => true) {
    const row = typeof target === 'string' ? requireSession(target) : target
    const id = row.id
    if (id.startsWith('native-tab:')) { await activateSession(id); return row }
    if (id.startsWith('legacy-tab:')) {
      const owns = captureSessionOwnership(id)
      const ownsSelection = captureSelectionOwnership()
      if (!canAdmit()) throw new Error('RESTORE_CANCELLED')
      // A chooser may retain an ended snapshot after the owning tab has started.
      // Only fresh adapter state can authorize an explicit ended-session resume.
      const current = (await adapterForRuntime('legacy-claude').listSessions(row.projectKey)).find(value => value.id === id)
      if (!canAdmit() || !owns() || !ownsSelection() || !current || current.nativeSessionId !== row.nativeSessionId
        || normalizePath(current.projectPath) !== normalizePath(row.projectPath)) throw new Error('STALE_SESSION_ATTEMPT')
      await activateSession(id)
      if (current.processState === 'stopped' && current.resumable) {
        const selected = captureSelectionOwnership()
        return restartSession(id, () => canAdmit() && owns() && selected() && activeSessionId.value === id)
      }
      return current
    }
    if (row.archived) throw new Error('SESSION_ARCHIVED')
    try {
      const resumed = await resumeSession(resumeInput(row), canAdmit)
      missingRecords.delete(id)
      sessions.value = sessions.value.filter(value => value.id !== id || value.id === resumed.id)
      return resumed
    }
    catch (failure) {
      if (failure instanceof Error && failure.message === 'SESSION_NOT_FOUND') {
        const missing = { ...row, safeErrorCode: 'SESSION_NOT_FOUND' }
        missingRecords.set(id, missing)
        sessions.value = [...sessions.value.filter(value => value.id !== id), missing]
      }
      throw failure
    }
  }
  async function removeMissingRecord(id: string) {
    const row = missingRecords.get(id)
    const adapter = row && adapterForRuntime(row.runtime)
    if (!row || !adapter?.verifyMissingSession) throw new Error('SESSION_MISSING_NOT_VERIFIED')
    await projects.ensureLoaded()
    try { await adapter.verifyMissingSession(resumeInput(row)) }
    catch (failure) {
      if (failure instanceof Error && failure.message === 'SESSION_EXISTS') {
        missingRecords.delete(id)
        await refresh(row.projectKey)
      }
      throw failure
    }
    // UI metadata keys must be exact. Old raw/native IDs cannot authorize deletion
    // of a different source's record, even when their displayed IDs happen to match.
    const keys = [id]
    if (row.runtime === 'legacy-claude') keys.push(makeSessionCatalogKey(row))
    for (const key of keys) if (projects.sessionRecords.has(key)) await projects.removeSessionRecord(key)
    missingRecords.delete(id)
    sessions.value = sessions.value.filter(value => value.id !== id)
    if (activeSessionId.value === id) activeSessionId.value = null
  }
  async function launchResume(input: CreateUnifiedSessionInput, canAdmit = () => true): Promise<UnifiedSession> {
    if (!input.launchConfigId || !input.launchConfigRevision || !input.action || !['resume-id', 'resume-picker'].includes(input.action.kind)) throw new Error('RESTORE_CONFIGURATION_REQUIRED')
    revokeUnissuedRenames(null)
    const epoch = ++selectionEpoch
    ++selectionIntentEpoch
    // Never use new-session preparation: an explicit existing configuration and
    // already registered project are checked by the Native runtime owner.
    const opened = await adapterForRuntime('native-cli').createSession(copyInput(input), canAdmit)
    sessions.value = [...sessions.value.filter(row => row.id !== opened.id), opened]
    if (epoch === selectionEpoch) activeSessionId.value = opened.id
    await refresh(input.projectKey).catch(() => { error.value = 'SESSION_REFRESH_FAILED' })
    return opened
  }

  let adapters: SessionAdapter[] = []
  let prepareCreation: (input: CreateUnifiedSessionInput) => Promise<CreateUnifiedSessionInput> = async input => input
  const creations = new Map<string, { input: CreateUnifiedSessionInput; row: UnifiedSession; owner: object; preparing: boolean; selectionIntentEpoch: number }>()
  function configureCreationPreparer(prepare: typeof prepareCreation) { prepareCreation = prepare }
  function isPreparingSession(id: string) { return creations.has(id) }
  function hasUnadmittedConfiguration(profileId: string, cli: UnifiedSession['cli']): boolean {
    return [...creations.values()].some(creation => ['starting', 'unknown'].includes(creation.row.processState)
      && (creation.input.launchConfigId ? creation.input.launchConfigId === profileId : creation.input.cli === cli))
  }
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

  function captureSessionOwnership(id: string) {
    const creation = creations.get(id)
    if (creation) { const owner = creation.owner; return () => creations.get(id)?.owner === owner }
    return adapterForRuntime(requireSession(id).runtime).captureOwnership?.(id) ?? (() => false)
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
      && (input.runtime === undefined || session.runtime === input.runtime)
      && (!input.nativeOrigin || session.nativeOrigin && nativeHistoryContextKey(session.nativeOrigin) === nativeHistoryContextKey(input.nativeOrigin)),
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
      for (const [id, row] of missingRecords) byId.set(id, row)
      // Adapter reads project runtime state; an unsaved draft belongs to the
      // current UI owner until cancellation or exact source/attempt invalidation.
      for (const [id, owner] of renameOwners) {
        if (!ownsRename(owner, byId.get(id))) {
          renameOwners.delete(id)
          const row = byId.get(id)
          if (row) byId.set(id, { ...row, renameState: 'idle' })
        }
      }
      sessions.value = [...byId.values()].map(row => {
        const owner = renameOwners.get(row.id)
        return owner ? { ...row, renameState: owner.state } : row
      }).sort((a, b) =>
        b.lastActivityAt - a.lastActivityAt || a.id.localeCompare(b.id),
      )
      if (activeSessionId.value && !byId.has(activeSessionId.value)) {
        activeSessionId.value = null
      }
    } catch (failure) {
      if (refreshVersion === version) {
        error.value = safeUserErrorCode(failure)
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

  /** Explicit project navigation changes selection, never runtime ownership. */
  function selectProjectContext(projectPath: string): void {
    if (activeSession.value && normalizePath(activeSession.value.projectPath) === normalizePath(projectPath)) return
    clearActionFeedback(); closeSessionConfirmation()
    revokeUnissuedRenames(null)
    ++selectionEpoch
    ++selectionIntentEpoch
    activeSessionId.value = null
  }

  async function activateSession(id: string): Promise<void> {
    clearActionFeedback(); closeSessionConfirmation()
    const session = requireSession(id)
    revokeUnissuedRenames(id)
    const epoch = ++selectionEpoch
    const intentEpoch = ++selectionIntentEpoch
    const creation = creations.get(id)
    if (creation) creation.selectionIntentEpoch = intentEpoch
    else await adapterForRuntime(session.runtime).activateSession(id)
    if (epoch === selectionEpoch && sessions.value.some(value => value.id === id)) {
      activeSessionId.value = id
    }
  }

  async function createSession(input: CreateUnifiedSessionInput, retryId?: string, confirmationGuard?: () => boolean): Promise<UnifiedSession> {
    ++selectionEpoch
    const intentEpoch = ++selectionIntentEpoch
    const id = retryId ?? createNativeId('preparing')
    const owner = {}
    const row: UnifiedSession = { id, projectKey: normalizePath(input.projectPath), projectPath: input.projectPath,
      cli: input.cli, runtime: 'native-cli', title: input.title || (input.cli === 'claude' ? 'Claude Code' : 'Codex CLI'),
      processState: 'starting', attentionState: 'none', lastActivityAt: Date.now(), archived: false, opened: false, preparationState: 'pending', resumable: false, adapterSessionId: id }
    const creation = { input: copyInput(input), row, owner, preparing: true, selectionIntentEpoch: intentEpoch }
    creations.set(id, creation)
    sessions.value = [...sessions.value.filter(session => session.id !== id), row]
    activeSessionId.value = id
    const current = () => creations.get(id)?.owner === owner
    const canAdmit = () => current() && (!confirmationGuard || confirmationGuard()
      && intentEpoch === selectionIntentEpoch && activeSessionId.value === id)
    try {
      const prepared = await prepareCreation(copyInput(creation.input))
      if (!canAdmit()) throw new Error('NEW_SESSION_CANCELLED')
      if (confirmationGuard && (prepared.launchConfigId !== input.launchConfigId || prepared.launchConfigRevision !== input.launchConfigRevision)) throw new Error('PROFILE_SELECTION_CHANGED')
      creation.preparing = false
      const created = await adapterForRuntime('native-cli').createSession(prepared, canAdmit)
      creations.delete(id)
      sessions.value = [...sessions.value.filter(session => session.id !== id && session.id !== created.id), created]
      if (creation.selectionIntentEpoch === selectionIntentEpoch && activeSessionId.value === id) activeSessionId.value = created.id
      // Admission is not CLI success. A failed catalog read cannot turn an
      // admitted attempt into a retryable preparation row.
      await refresh(creation.input.projectKey).catch(() => { error.value = 'SESSION_REFRESH_FAILED' })
      return sessions.value.find(value => value.id === created.id) ?? created
    } catch (failure) {
      if (!current()) throw new Error('NEW_SESSION_CANCELLED')
      row.processState = creation.preparing ? 'failed' : 'unknown'
      row.preparationState = creation.preparing ? 'failed' : 'unknown'
      if (creation.preparing && failure instanceof LaunchConfigurationRequiredError) {
        row.safeErrorCode = 'LAUNCH_CONFIGURATION_REQUIRED'
        row.preparationIssueCode = failure.issueCode
        row.launchConfigId = failure.profileId
      } else row.safeErrorCode = creation.preparing ? 'NEW_SESSION_PREPARATION_FAILED' : 'LAUNCH_STATE_UNKNOWN'
      sessions.value = sessions.value.map(session => session.id === id ? { ...row } : session)
      throw new Error(row.safeErrorCode)
    }
  }

  async function resumeSession(input: ResumeUnifiedSessionInput, canAdmit = () => true): Promise<UnifiedSession> {
    revokeUnissuedRenames(null)
    const epoch = ++selectionEpoch
    ++selectionIntentEpoch
    const resumed = await adapterForResume(input).resumeSession(input, canAdmit)
    sessions.value = [...sessions.value.filter(row => row.id !== resumed.id), resumed]
    await refresh(input.projectKey).catch(() => { error.value = 'SESSION_REFRESH_FAILED' })
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
      creation.owner = {}; creation.row.processState = 'failed'; creation.row.safeErrorCode = 'NEW_SESSION_CANCELLED'; creation.row.preparationState = 'failed'
      sessions.value = sessions.value.map(row => row.id === id ? { ...creation.row } : row)
      return Promise.resolve()
    }
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    return enqueue(id, () => adapter.stopSession(id), () => refresh(session.projectKey))
  }

  function retryConfirmedCreation(id: string, profileId: string, profileRevision: string, canContinue: () => boolean): Promise<UnifiedSession> {
    const creation = creations.get(id)
    if (!canContinue() || activeSessionId.value !== id || !creation || creation.row.preparationState !== 'failed'
      || creation.row.preparationIssueCode !== 'PROGRAM_TRUST_REQUIRED' || creation.row.launchConfigId !== profileId) return Promise.reject(new Error('NEW_SESSION_CANCELLED'))
    return createSession({ ...copyInput(creation.input), launchConfigId: profileId, launchConfigRevision: profileRevision }, id, canContinue)
  }

  function restartSession(id: string, canContinue = () => true): Promise<UnifiedSession> {
    const creation = creations.get(id)
    if (creation) {
      if (creation.row.processState !== 'failed') return Promise.reject(new Error('LAUNCH_STATE_UNKNOWN'))
      const input = copyInput(creation.input)
      if (creation.row.safeErrorCode === 'LAUNCH_CONFIGURATION_REQUIRED' && creation.row.launchConfigId) {
        // Explicit retry of a never-admitted prerequisite failure may use the
        // user's saved repair, but only for the same configuration and CLI.
        input.launchConfigId = creation.row.launchConfigId
        delete input.launchConfigRevision
      }
      return createSession(input, id)
    }
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    return enqueue(id, () => adapter.restartSession(id, canContinue), async restarted => {
      await refresh(session.projectKey)
      if (activeSessionId.value === id) activeSessionId.value = restarted.id
    })
  }

  function closeSession(id: string, canContinue = () => true): Promise<void> {
    const wasSelected = activeSessionId.value === id
    const ownsSelection = captureSelectionOwnership()
    async function selectRemaining(projectKey: string) {
      // Ownership publication can clear the closed ID before this operation
      // finishes. Only its original selection intent may choose a replacement.
      if (!wasSelected || !ownsSelection() || activeSessionId.value !== null && activeSessionId.value !== id) return
      activeSessionId.value = null
      const open = sessions.value.filter(row => row.id !== id && !row.archived
        && row.id === `${row.runtime === 'native-cli' ? 'native-tab' : 'legacy-tab'}:${row.adapterSessionId}`)
      const next = open.find(row => normalizePath(row.projectPath) === normalizePath(projectKey)) ?? open[0]
      // History is never resumed by a close; the existing runtime adapter owns
      // activation, including the Legacy aggregate's internal selected tab.
      if (next) await activateSession(next.id)
    }
    const creation = creations.get(id)
    if (creation) {
      if (!creation.preparing && creation.row.processState === 'starting') return Promise.reject(new Error('NEW_SESSION_ADMISSION_IN_PROGRESS'))
      if (creation.row.processState === 'unknown') return Promise.reject(new Error('LAUNCH_STATE_UNKNOWN'))
      creations.delete(id); sessions.value = sessions.value.filter(row => row.id !== id)
      if (activeSessionId.value === id) activeSessionId.value = null
      ++selectionEpoch
      return selectRemaining(creation.row.projectKey)
    }
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    ++selectionEpoch
    return enqueue(id, () => adapter.closeSession(id, canContinue), async () => {
      if (activeSessionId.value === id) activeSessionId.value = null
      await refresh(session.projectKey)
      await selectRemaining(session.projectKey)
    }, 'close')
  }

  function discardPreparation(id: string): Promise<void> {
    const creation = creations.get(id)
    if (!creation?.preparing || creation.row.processState !== 'failed') return Promise.reject(new Error('STALE_SESSION_ATTEMPT'))
    return closeSession(id)
  }

  function beginRename(id: string) {
    const row = sessions.value.find(value => value.id === id)
    if (!row || activeSessionId.value !== id || row.renameState === 'saving' || (pendingRenames.get(id) ?? 0) > 0 || isPreparingSession(id)) return
    const current = renameOwners.get(id)
    if (!current || !ownsRename(current, row)) renameOwners.set(id, captureRenameOwner(row, 'editing'))
    sessions.value = sessions.value.map(row => row.id === id ? { ...row, renameState: 'editing' } : row)
  }
  function cancelRename(id: string) {
    if ((pendingRenames.get(id) ?? 0) > 0) return
    renameOwners.delete(id)
    sessions.value = sessions.value.map(row => row.id === id ? { ...row, renameState: 'idle' } : row)
  }
  function renameSession(id: string, title: string, requireEditor = false): Promise<void> {
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    const current = renameOwners.get(id)
    if ((requireEditor && (!current || activeSessionId.value !== id)) || (current && !ownsRename(current, session))) {
      if (!current?.issued) {
        renameOwners.delete(id)
        sessions.value = sessions.value.map(row => row.id === id ? { ...row, renameState: 'idle' } : row)
      }
      return Promise.reject(new Error('STALE_SESSION_ATTEMPT'))
    }
    const owner = current ?? captureRenameOwner(session, 'saving')
    owner.state = 'saving'
    renameOwners.set(id, owner)
    pendingRenames.set(id, (pendingRenames.get(id) ?? 0) + 1)
    sessions.value = sessions.value.map(row => row.id === id ? { ...row, renameState: 'saving' } : row)
    return enqueue(id, () => {
      if (renameOwners.get(id) !== owner || !ownsRename(owner, sessions.value.find(row => row.id === id))) throw new Error('STALE_SESSION_ATTEMPT')
      return adapter.renameSession(id, title, () => renameOwners.get(id) === owner && ownsRename(owner, sessions.value.find(row => row.id === id)), () => { owner.issued = true })
    }, () => refresh(session.projectKey)).finally(() => {
      const remaining = (pendingRenames.get(id) ?? 1) - 1
      if (remaining) pendingRenames.set(id, remaining)
      else pendingRenames.delete(id)
      if (renameOwners.get(id) === owner && !remaining) cancelRename(id)
    })
  }

  function archiveSession(id: string, canContinue = () => true): Promise<void> {
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    ++selectionEpoch
    return enqueue(id, () => adapter.archiveSession(id, canContinue), async () => {
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
    sessionConfirmation, confirmationBusy, confirmationError, beginSessionConfirmation, closeSessionConfirmation, confirmSessionAction,
    configureUnknownRestartRecovery, captureSelectionOwnership, captureFeedbackOwner, publishActionFailure, publishActionSuccess, clearActionFeedback, actionFeedback,
    projectGroups,
    activeSessionId,
    activeSession,
    initialized,
    loading,
    error,
    configureAdapters,
    captureSessionOwnership,
    resumeDialog, openResumeDialog, closeResumeDialog, configureHistoryLoader, searchSessions,
    resumeCatalogSession, removeMissingRecord, launchResume,
    configureCreationPreparer,
    isPreparingSession,
    hasUnadmittedConfiguration,
    initialize,
    refresh,
    selectProjectContext,
    activateSession,
    createSession,
    retryConfirmedCreation,
    resumeSession,
    stopSession,
    restartSession,
    closeSession,
    renameSession, beginRename, cancelRename, discardPreparation,
    archiveSession,
    restoreArchivedSession,
  }
})
