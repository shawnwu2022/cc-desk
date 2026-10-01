import { computed, nextTick, onMounted, onUnmounted, ref, watch, type Ref } from 'vue'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { openInFileManager } from '@/api/tauri'
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import { useNativeTabsStore, captureNativeAttempt, matchesNativeAttempt } from '@/stores/nativeTabs'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useWorkspaceStore } from '@/stores/workspace'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useShellStore, type WorkspaceRequest } from '@/stores/shell'
import { useHookStore } from '@/stores/hook'
import { useAttentionStore } from '@/stores/attention'
import { createLegacyClaudeAdapter } from '@/session/adapters/legacyClaudeAdapter'
import { createNativeCliAdapter, type NativeRuntimeCreateInput } from '@/session/adapters/nativeCliAdapter'
import { mapSafeUserError, safeUserErrorCode } from '@/utils/userError'
import { sameProjectPath } from '@/utils/path'
import type { OpenTerminalSession, UnifiedTerminalHostPort } from '@/terminal/unifiedTerminalHost'
import type { UnifiedCliKind } from '@/types/unifiedSession'

/** Normal App composition root. Read-only bootstrap is independent per source;
 * only explicitly admitted operations can enter the existing runtime owners. */
export function useUnifiedWorkspaceRuntime(host: Ref<UnifiedTerminalHostPort | null>, enabled = true) {
  const app = useAppStore()
  const legacy = useSessionStore()
  const native = useNativeTabsStore()
  const history = useNativeHistoryStore()
  const profiles = useCliProfilesStore()
  const draft = useNewSessionDraftStore()
  const workspace = useWorkspaceStore()
  const projects = useProjectsStateStore()
  const catalog = useUnifiedSessionsStore()
  const shell = useShellStore()
  const legacyPaths = ref<string[]>([])
  const error = ref<string | null>(null)
  const ready = ref(false)
  const loading = ref(false)
  const fatal = computed(() => ready.value && !loading.value && !projects.loaded
    && profiles.status === 'error' && workspace.status === 'error' && app.managedProjectsStatus === 'error'
    && !openSessions.value.length && !catalog.sessions.length && !app.cachedProjects.length
    && !profiles.profiles.length && !workspace.projects.length && !projects.pinnedProjects.length)
  let retryRequest: { request: WorkspaceRequest; owns: () => boolean } | null = null
  const cliProblems = computed(() => (['claude', 'codex'] as const).flatMap(cli => {
    const latest = [...native.tabs.values()].reverse().filter(tab => tab.cli === cli).sort((a, b) => b.lastActivityAt - a.lastActivityAt)[0]
    const failed = latest?.status === 'failed' ? latest : null
    const code = latest?.status === 'running' ? null : failed?.errorCode === 'AUTH_REQUIRED' ? 'AUTH_REQUIRED'
      : draft.cliAvailability[cli] === 'unavailable' || ['CLI_NOT_FOUND', 'PROGRAM_UNAVAILABLE'].includes(failed?.errorCode ?? '') ? 'CLI_NOT_FOUND' : null
    return code ? [{ cli, messageKey: mapSafeUserError(code, 'launch').messageKey }] : []
  }))
  function retryAction() {
    const pending = retryRequest
    retryRequest = null
    if (pending?.owns()) shell.requestWorkspaceAction(pending.request)
    else shell.requestWorkspaceAction({ kind: 'refresh' })
  }
  let disposed = false
  let refreshOwner = 0
  let claimedSequence = -1

  const cliAvailability = computed(() => draft.cliAvailability)
  const awaitingSuccess = new Map<string, { attempt: ReturnType<typeof captureNativeAttempt>; projectPath: string; cli: UnifiedCliKind; profileId: string; profileRevision: string }>()
  watch(() => [...native.tabs.values()].map(tab => ({ ...tab })), rows => {
    for (const [id, frozen] of awaitingSuccess) {
      const tab = rows.find(row => row.tabId === id)
      if (!matchesNativeAttempt(tab, frozen.attempt)) { awaitingSuccess.delete(id); continue }
      if (tab?.status === 'running' && tab.launchRevision !== null) {
        awaitingSuccess.delete(id) // Claim before the canonical store write can publish state.
        if (profiles.profile(frozen.profileId)?.revision === frozen.profileRevision) {
          const sequence = shell.requestSequence
          const ownsSelection = catalog.captureSelectionOwnership()
          void draft.recordSuccess(frozen.projectPath, frozen.cli, frozen.profileId).catch(() => {
            if (!disposed && shell.requestSequence === sequence && ownsSelection() && matchesNativeAttempt(native.tab(id), frozen.attempt)) error.value = 'newSessionPreferenceSaveFailed'
          })
        }
      } else if (tab && ['failed', 'exited'].includes(tab.status)) awaitingSuccess.delete(id)
    }
  }, { flush: 'sync' })
  const openSessions = computed<OpenTerminalSession[]>(() => [
    ...[...legacy.tabs.values()].filter(tab => !tab.cli || tab.cli === 'claude').map(tab => ({
      id: `legacy-tab:${tab.tabId}`, adapterSessionId: tab.tabId, runtime: 'legacy-claude' as const,
    })),
    ...[...native.tabs.values()].map(tab => ({
      id: `native-tab:${tab.tabId}`, adapterSessionId: tab.tabId, runtime: 'native-cli' as const,
    })),
  ])
  function requireHost() {
    if (disposed || !host.value) throw new Error('TERMINAL_HOST_NOT_READY')
    return host.value
  }
  function createNativeTab(input: NativeRuntimeCreateInput) {
    if (app.isProjectAdmissionBlocked(input.projectPath)) throw new Error('PROJECT_REMOVAL_IN_PROGRESS')
    const source = input.sourceContext
    const profile = input.launchConfigId ? profiles.profile(input.launchConfigId) : profiles.selected[input.cli]
    if (!profile) throw new Error('CLI_PROFILE_REQUIRED')
    if (profiles.isDeleting(profile.id)) throw new Error('PROFILE_IN_USE')
    if (profile.cli !== input.cli) throw new Error('PROFILE_CLI_MISMATCH')
    if (input.launchConfigRevision && profile.revision !== input.launchConfigRevision) throw new Error('PROFILE_SELECTION_CHANGED')
    if (source && (source.cli !== input.cli || source.profileId !== profile.id || source.profileRevision !== profile.revision)) {
      throw new Error('PROFILE_SELECTION_CHANGED')
    }
    const matches = workspace.projects.filter(project => sameProjectPath(project.selectedPath, input.projectPath)
      && (!source || project.projectId === source.projectId)
      && (!input.registeredProjectId || project.projectId === input.registeredProjectId))
    if (matches.length !== 1 || (source && !sameProjectPath(source.projectPath, input.projectPath))) throw new Error('PROJECT_NOT_FOUND')
    // Paths selected by the frontend do not create registration or bridge authority.
    const tab = native.create({ cli: input.cli, projectId: matches[0].projectId, projectPath: matches[0].selectedPath,
      profileId: profile.id, profileRevision: profile.revision, action: input.action,
      title: input.title, sourceSessionKey: input.sourceSessionKey })
    if (!source) awaitingSuccess.set(tab.tabId, { attempt: captureNativeAttempt(tab), projectPath: tab.projectPath,
      cli: tab.cli, profileId: tab.profileId, profileRevision: tab.profileRevision })
    return tab
  }
  async function restartNative(tabId: string) {
    const tab = native.tab(tabId)
    if (!tab) throw new Error('NATIVE_SESSION_NOT_FOUND')
    const frozen = { ...tab }
    const ownsSource = catalog.captureSessionOwnership(`native-tab:${tabId}`)
    const attempt = captureNativeAttempt(frozen)
    if (frozen.status === 'unknown') throw new Error('LAUNCH_STATE_UNKNOWN')
    const requireSource = () => {
      const profile = profiles.profile(frozen.profileId)
      if (profiles.isDeleting(frozen.profileId)) throw new Error('PROFILE_IN_USE')
      if (!profile || profile.cli !== frozen.cli || profile.revision !== frozen.profileRevision) throw new Error('PROFILE_SELECTION_CHANGED')
      if (!workspace.projects.some(project => project.projectId === frozen.projectId && sameProjectPath(project.selectedPath, frozen.projectPath))) throw new Error('PROJECT_NOT_FOUND')
    }
    requireSource()
    if (frozen.status === 'starting' || frozen.status === 'running') await requireHost().stopNative(tabId, attempt)
    if (!matchesNativeAttempt(native.tab(tabId), attempt)) throw new Error('STALE_NATIVE_ATTEMPT')
    if (!ownsSource()) throw new Error('STALE_SESSION_ATTEMPT')
    requireSource()
    const restarted = native.restart(tabId, { cli: frozen.cli, profileId: frozen.profileId, profileRevision: frozen.profileRevision })
    awaitingSuccess.set(restarted.tabId, { attempt: captureNativeAttempt(restarted), projectPath: restarted.projectPath, cli: restarted.cli, profileId: restarted.profileId, profileRevision: restarted.profileRevision })
    return restarted
  }
  if (enabled) {
    // Subscribe attention before child TerminalView status consumers mount.
    useAttentionStore().init()
    catalog.configureUnknownRestartRecovery(async (id, canContinue) => {
      const row = catalog.sessions.find(row => row.id === id)
      if (row?.runtime !== 'native-cli') throw new Error('LAUNCH_STATE_UNKNOWN')
      const tab = native.tab(row.adapterSessionId)
      if (!tab || !canContinue()) throw new Error('STALE_SESSION_ATTEMPT')
      const attempt = captureNativeAttempt(tab)
      await requireHost().recoverNative(tab.tabId, attempt)
      if (!canContinue() || !matchesNativeAttempt(native.tab(tab.tabId), attempt)) throw new Error('STALE_SESSION_ATTEMPT')
      const current = native.tab(tab.tabId)!
      if (current.status === 'unknown') throw new Error('LAUNCH_STATE_UNKNOWN')
      if (current.status === 'running' || current.status === 'starting') await requireHost().stopNative(current.tabId, attempt)
      if (!canContinue() || !matchesNativeAttempt(native.tab(tab.tabId), attempt)) throw new Error('STALE_SESSION_ATTEMPT')
      const ended = native.tab(tab.tabId)!
      if (!['stopped', 'exited', 'failed'].includes(ended.status) || ended.launchRevision === null) throw new Error('NATIVE_STOP_UNCONFIRMED')
    })
    catalog.configureCreationPreparer(async input => {
      if (app.isProjectAdmissionBlocked(input.projectPath)) throw new Error('PROJECT_REMOVAL_IN_PROGRESS')
      const prepared = await draft.prepareInput(input)
      if (app.isProjectAdmissionBlocked(input.projectPath)) throw new Error('PROJECT_REMOVAL_IN_PROGRESS')
      await workspace.ensureRegistered(prepared.projectPath)
      if (disposed) throw new Error('NEW_SESSION_CANCELLED')
      const profile = prepared.launchConfigId ? profiles.profile(prepared.launchConfigId) : null
      if (!profile || profile.cli !== prepared.cli || profile.revision !== prepared.launchConfigRevision) throw new Error('PROFILE_SELECTION_CHANGED')
      if (profiles.isDeleting(profile.id)) throw new Error('PROFILE_IN_USE')
      return prepared
    })
    catalog.configureHistoryLoader(async query => {
      let partial = false
      const paths = query.scope === 'all' ? [...new Set([...legacyPaths.value, ...projects.pinnedProjects, ...[...legacy.tabs.values()].map(tab => tab.projectPath)])] : [query.projectPath]
      if (!query.cli || query.cli === 'claude') {
        for (let i = 0; i < paths.length && !disposed; i += 2) {
          const results = await Promise.all(paths.slice(i, i + 2).map(path => legacy.loadHistoryFor(path)))
          if (results.some(result => !result.ok)) partial = true
        }
      }
      const contexts = profiles.profiles.flatMap(profile => profile.cli === 'shell' || query.cli && profile.cli !== query.cli ? []
        : workspace.projects.filter(project => query.scope === 'all' || sameProjectPath(project.selectedPath, query.projectPath)).map(project => ({
          cli: profile.cli as UnifiedCliKind, profileId: profile.id, profileRevision: profile.revision, projectId: project.projectId, projectPath: project.selectedPath,
        })))
      for (let i = 0; i < contexts.length && !disposed; i += 2) await Promise.all(contexts.slice(i, i + 2).map(async context => {
        try { if ((await history.load(context)).error) partial = true } catch { partial = true }
      }))
      return partial
    })
    catalog.configureAdapters([
      createLegacyClaudeAdapter({ store: legacy, captureProjectAdmission: app.captureProjectAdmission, projectPaths: () => [...new Set([...legacyPaths.value, ...projects.pinnedProjects])],
        runtime: {
          startTab: id => {
            const tab = legacy.tabs.get(id)
            if (!tab || app.isProjectAdmissionBlocked(tab.projectPath)) throw new Error('PROJECT_REMOVAL_IN_PROGRESS')
            return requireHost().startLegacy(id)
          }, stopTab: id => requireHost().stopLegacy(id),
          restartTab: id => requireHost().restartLegacy(id), renameTab: (id, title) => requireHost().renameLegacy(id, title),
        } }),
      createNativeCliAdapter({ tabs: native, history, archive: { getArchivedSessions: legacy.getArchivedSessions, archiveSession: projects.archiveSession, restoreSession: projects.restoreSession },
        runtime: { createTab: createNativeTab, restartTab: restartNative,
          stopTab: tab => requireHost().stopNative(tab.tabId, captureNativeAttempt(tab)) } }),
    ])
  }

  async function refresh() {
    if (!enabled || disposed) return
    const owner = ++refreshOwner
    const current = () => !disposed && owner === refreshOwner
    loading.value = true
    error.value = null
    let partial = false
    const settle = async (operation: () => Promise<unknown>) => {
      try { await operation() } catch { partial = true }
    }
    await Promise.all([
      settle(() => projects.ensureLoaded()),
      settle(async () => {
        await app.loadManagedProjects()
        const rows = app.cachedProjects
        if (!current()) return
        legacyPaths.value = rows.map(row => row.path)
        const paths = [...new Set([...legacyPaths.value, ...projects.pinnedProjects, ...[...legacy.tabs.values()].map(tab => tab.projectPath)])]
        for (let i = 0; i < paths.length && current(); i += 2) {
          const results = await Promise.all(paths.slice(i, i + 2).map(path => legacy.loadHistoryFor(path, true)))
          if (results.some(result => !result.ok)) partial = true
        }
      }),
      settle(async () => {
        const results = await Promise.allSettled([profiles.load(), workspace.load()])
        if (results.some(result => result.status === 'rejected')) { partial = true; return }
        await draft.refreshAvailability()
        const contexts = profiles.profiles.flatMap(profile => profile.cli === 'shell' ? [] : workspace.projects.map(project => ({
          cli: profile.cli as UnifiedCliKind, profileId: profile.id, profileRevision: profile.revision,
          projectId: project.projectId, projectPath: project.selectedPath, force: true,
        })))
        // Match the backend's two-reader budget; isolate each source failure.
        for (let i = 0; i < contexts.length && current(); i += 2) {
          await Promise.all(contexts.slice(i, i + 2).map(context => settle(async () => {
            const entry = await history.load(context)
            if (entry.error) partial = true
          })))
        }
      }),
    ])
    if (!current()) return
    await settle(() => catalog.initialize())
    if (!current()) return
    // Even unavailable projects.json must not prevent the readable runtime catalog.
    await settle(() => catalog.refresh())
    if (!current()) return
    ready.value = true
    loading.value = false
    error.value = partial && !fatal.value ? 'workspaceRuntimePartial' : null
  }

  async function dispatch(request: WorkspaceRequest): Promise<boolean> {
    if (request.kind === 'refresh') { await refresh(); return true }
    if (request.kind === 'rename-cancel') { catalog.cancelRename(request.sessionId); return true }
    if (request.kind === 'new-session') {
      const { intent, ...project } = request.project
      if (intent === 'restore') {
        shell.requestWorkspaceAction({ kind: 'restore-session', project, mode: 'history' })
      } else if (intent === 'claude' || intent === 'codex') {
        await catalog.createSession({ ...project, cli: intent, action: { kind: 'new' } })
      } else if (intent === 'options') draft.open(project)
      else draft.openChooser(project)
      return true
    }
    if (request.kind === 'create-session') { await catalog.createSession(request.input); return true }
    if (request.kind === 'restore-session') {
      if (shell.section === 'workspace') catalog.openResumeDialog(request)
      return true
    }
    if (request.kind === 'confirmation') {
      if (shell.section === 'workspace') catalog.beginSessionConfirmation(request.request.kind, request.request.sessionId)
      return true
    }
    if (request.kind === 'add-project' || request.kind === 'open-project' || request.kind === 'project-action') return false
    if (!('sessionId' in request)) return false
    const session = catalog.sessions.find(value => value.id === request.sessionId)
    if (!session) return false
    const preparing = catalog.isPreparingSession(session.id)
    if (preparing) {
      if (request.kind === 'activate') { await catalog.activateSession(session.id); return true }
      if ('action' in request) {
        if (request.action === 'retry' && session.processState === 'failed') { await catalog.restartSession(session.id); return true }
        if (request.action === 'cancel-start' && session.processState === 'starting') { await catalog.stopSession(session.id); return true }
        if (request.action === 'close' && session.processState !== 'unknown') { await catalog.closeSession(session.id); return true }
      }
      return false
    }
    const open = openSessions.value.some(value => value.id === session.id)
    if (request.kind === 'activate') {
      if (!open) {
        if (shell.section === 'workspace') catalog.openResumeDialog({ project: session, cli: session.cli, mode: 'history', sessionId: session.id })
        return true
      }
      await catalog.activateSession(session.id)
      return true
    }
    if (request.kind === 'restore-archive') { await catalog.restoreArchivedSession(session.id); return true }
    if (request.kind === 'rename') {
      if (!open) return false
      await catalog.renameSession(session.id, request.title)
      return true
    }
    if (!('action' in request)) return false
    const action = request.action
    // Catalog publication is async. Consequential admission must use the current
    // owning store, never a stale row that still looks stopped.
    const state = open ? session.runtime === 'native-cli'
      ? native.tab(session.adapterSessionId)?.status
      : legacy.tabs.get(session.adapterSessionId)?.status : session.processState
    const live = state === 'running' || state === 'starting' || state === 'unknown'
    switch (action) {
      case 'stop':
      case 'cancel-start':
        if (action === 'cancel-start' && open && session.runtime === 'native-cli') {
          const tab = native.tab(session.adapterSessionId)
          if (tab?.status === 'stopped' && tab.launchRevision === null) { await catalog.closeSession(session.id); return true }
        }
        if (!open || !state || !['running', 'starting'].includes(state)) return false
        await catalog.stopSession(session.id); return true
      case 'confirm-status': {
        if (!open || session.runtime !== 'native-cli') return false
        const tab = native.tab(session.adapterSessionId)
        if (!tab) return false
        await requireHost().recoverNative(tab.tabId, captureNativeAttempt(tab))
        await catalog.refresh(session.projectKey); return true
      }
      case 'restart':
      case 'retry':
        if (!open) return false
        if (state === 'unknown') { if (shell.section === 'workspace') catalog.beginSessionConfirmation('restart-unknown', session.id); return true }
        await catalog.restartSession(session.id); return true
      case 'close':
        if (!open) return false
        if (live) { if (shell.section === 'workspace') catalog.beginSessionConfirmation('close-running', session.id); return true }
        await catalog.closeSession(session.id); return true
      case 'archive':
        if (live) { if (shell.section === 'workspace') catalog.beginSessionConfirmation('stop-and-archive', session.id); return true }
        await catalog.archiveSession(session.id); return true
      case 'resume':
        if (open) { await catalog.activateSession(session.id); return true }
        if (shell.section === 'workspace') catalog.openResumeDialog({ project: session, cli: session.cli, mode: 'history', sessionId: session.id })
        return true
      case 'restore-archive': await catalog.restoreArchivedSession(session.id); return true
      case 'copy-session-id':
        if (!session.nativeSessionId) return false
        await writeText(session.nativeSessionId); return true
      case 'open-project-directory': await openInFileManager(session.projectPath); return true
      default: return false
    }
  }
  watch(() => [ready.value, shell.requestSequence], async () => {
    if (!enabled || disposed || !shell.pendingRequest || claimedSequence === shell.requestSequence) return
    const pending = shell.pendingRequest
    const pendingSessionId = pending.kind === 'confirmation' ? pending.request.sessionId : 'sessionId' in pending ? pending.sessionId : undefined
    const ownsSession = typeof pendingSessionId === 'string' && (catalog.isPreparingSession(pendingSessionId)
      || openSessions.value.some(session => session.id === pendingSessionId))
    // Locally owned lifecycle controls cannot wait for unrelated history/bootstrap.
    if (!ready.value && !['new-session', 'create-session'].includes(pending.kind) && !ownsSession) return
    const sequence = shell.requestSequence
    const request = shell.pendingRequest
    catalog.closeSessionConfirmation()
    const feedback = catalog.captureFeedbackOwner()
    const ownsFeedbackSession = 'sessionId' in request && typeof request.sessionId === 'string' && catalog.sessions.some(row => row.id === request.sessionId)
      ? catalog.captureSessionOwnership(request.sessionId) : () => true
    const current = () => !disposed && shell.requestSequence === sequence && feedback() && ownsFeedbackSession()
    retryRequest = null
    claimedSequence = sequence // Claim before async work; never retry on reactive changes.
    try {
      if (await dispatch(request)) {
        shell.clearWorkspaceRequest(sequence)
        const key = request.kind === 'rename' ? 'feedbackRenamed'
          : 'action' in request && request.action === 'copy-session-id' ? 'feedbackCopied'
          : 'action' in request && request.action === 'archive' && !catalog.sessionConfirmation ? 'feedbackArchived' : null
        if (key) catalog.publishActionSuccess(current, key)
      }
    } catch (failure) {
      const code = safeUserErrorCode(failure)
      if (code === 'REVISION_CONFLICT') {
        const recovery = await Promise.allSettled([projects.reload(), profiles.load(), workspace.load()])
        if (recovery.some(result => result.status === 'rejected')) {
          catalog.publishActionFailure(current, new Error('RECOVERY_UNAVAILABLE'))
          return
        }
      }
      if (current()) {
        error.value = code === 'SESSION_ORIGIN_AMBIGUOUS' ? 'resumeAmbiguous' : null
        catalog.publishActionFailure(current, failure)
        if (mapSafeUserError(code, 'session').retryable) retryRequest = { request, owns: () => current() && ownsFeedbackSession() }
      }
      // No side effect is replayed. Retry is a new, explicit user request.
    }
  }, { immediate: true })
  watch(() => [
    [...legacy.tabs.values()].map(tab => ({ ...tab })), [...native.tabs.values()].map(tab => ({ ...tab })),
    history.all(), projects.archivedSessions,
  ], async () => {
    if (!enabled || !ready.value || disposed) return
    try { await catalog.refresh() } catch { error.value = 'workspaceRuntimePartial' }
  }, { deep: true })
  watch(() => catalog.activeSessionId, async () => {
    await nextTick()
    if (!disposed && shell.section === 'workspace') host.value?.focus()
  })
  onMounted(() => {
    if (!enabled) return
    void useHookStore().init()
    void refresh()
  })
  onUnmounted(() => { disposed = true; ++refreshOwner })
  return { openSessions, cliAvailability, cliProblems, fatal, ready, loading, error, refresh, retryAction }
}
