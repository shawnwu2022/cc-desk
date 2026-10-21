import { computed, shallowRef, nextTick, onMounted, onUnmounted, ref, watch, type Ref } from 'vue'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { openInFileManager, nativeUnarchiveSession, nativeRenameSession, nativePrepareOpencodeSession, nativeGetScope, legacyClaudeRootKey, getSessions } from '@/api/tauri'
import { useSessionIndexMaintenanceStore } from '@/stores/sessionIndexMaintenance'
import { useLegacyHistoryRefresh } from '@/composables/useLegacyHistoryRefresh'
import { createNativeId } from '@/utils/nativeId'
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import { useNativeTabsStore, captureNativeAttempt, matchesNativeAttempt, type NativeCliTab } from '@/stores/nativeTabs'
import { createNativeHistoryReader, nativeHistoryContextKey, useNativeHistoryStore, type NativeHistoryEntry } from '@/stores/nativeHistory'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useWorkspaceStore } from '@/stores/workspace'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useCliSupportStore } from '@/stores/cliSupport'
import { useShellStore, type WorkspaceRequest } from '@/stores/shell'
import { useHookStore } from '@/stores/hook'
import { useAttentionStore } from '@/stores/attention'
import { createLegacyClaudeAdapter } from '@/session/adapters/legacyClaudeAdapter'
import { createNativeCliAdapter, type NativeRuntimeCreateInput, type NativeRestartContinuation } from '@/session/adapters/nativeCliAdapter'
import { mapSafeUserError, safeUserErrorCode } from '@/utils/userError'
import { projectSessionDiagnostics } from '@/utils/sessionDiagnostics'
import { normalizePath, sameProjectPath } from '@/utils/path'
import { createWorkspaceSourceWarnings, workspaceWarningKey, type WorkspaceHistoryIssue, type WorkspaceSourceConfiguration, type WorkspaceSourceWarning, type WorkspaceWarningSource } from '@/utils/workspaceSourceWarnings'
import type { OpenTerminalSession, UnifiedTerminalHostPort } from '@/terminal/unifiedTerminalHost'
import type { UnifiedCliKind, UnifiedSession } from '@/types/unifiedSession'

/** 历史读取失败的警告源按发起读取的 CLI 归属，不能把非 Claude 失败一律归给 Codex。
 * 用 Record 而非链式三元：新增 CLI 时由编译器强制补映射，避免静默落入他人来源。 */
const historyWarningSources: Record<UnifiedCliKind, WorkspaceWarningSource> = {
  claude: 'claude-history',
  codex: 'codex-history',
  opencode: 'opencode-history',
}
export const historyWarningSource = (cli: UnifiedCliKind): WorkspaceWarningSource => historyWarningSources[cli]

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
  const cliSupport = useCliSupportStore()
  const attention = useAttentionStore()
  const shell = useShellStore()
  const indexMaintenance = useSessionIndexMaintenanceStore()
  const legacyPaths = ref<string[]>([])
  /** 根证明只绑定本 runtime 初始目录；替换或验证失败即撤销，不能重绑旧终端和缓存。 */
  let legacyClaudeRoot: string | undefined
  let legacyRootRetired = false
  let legacyRootValidation: Promise<string | undefined> | undefined
  function validateLegacyClaudeRoot(): Promise<string | undefined> {
    const expected = legacyClaudeRoot
    if (!expected || legacyRootRetired || disposed) return Promise.resolve(undefined)
    if (legacyRootValidation) return legacyRootValidation
    const retire = () => {
      if (disposed || legacyRootRetired) return
      legacyRootRetired = true
      legacyClaudeRoot = undefined
      // 证明属于整个旧根：局部读取发现撤销时，其他依赖项目也必须解除折叠。
      queueCatalogProjects(...history.all().filter(entry => entry.sourceRootKey === expected).map(entry => entry.context.projectPath))
    }
    const validation = legacyClaudeRootKey().then(proof => {
      if (disposed || legacyRootRetired) return undefined
      if (proof?.rootKey === expected) return expected
      retire()
      return undefined
    }).catch(() => {
      retire()
      return undefined
    }).finally(() => {
      if (legacyRootValidation === validation) legacyRootValidation = undefined
    })
    legacyRootValidation = validation
    return validation
  }
  const error = ref<string | null>(null)
  const sourceWarnings = ref<WorkspaceSourceWarning[]>([])
  const sourceWarningsTruncated = ref(false)
  // Configuration identities come from admitted load contexts, never error fields.
  const sourceConfigurationOwners = ref<Omit<WorkspaceSourceConfiguration, 'name'>[]>([])
  const sourceWarningConfigurations = computed<WorkspaceSourceConfiguration[]>(() => sourceConfigurationOwners.value.flatMap(owner => {
    const profile = profiles.profile(owner.profileId)
    return profile && profile.revision === owner.profileRevision ? [{ ...owner, name: profile.name }] : []
  }))
  const acknowledgedSourceWarnings = ref(new Set<string>())
  const acknowledgedHistoryWarnings = ref(new Set<string>())
  // Replaced per owned refresh, deduped per canonical profile and fixed triple.
  // Never accumulate historical profiles or raw errors, including omitted rows.
  const sourceNoticeKeys = ref<string[]>([])
  const sourceNoticeDismissed = computed(() => sourceNoticeKeys.value.length > 0
    && sourceNoticeKeys.value.every(key => acknowledgedSourceWarnings.value.has(key)))
  function dismissSourceNotice() {
    acknowledgedSourceWarnings.value = new Set(sourceNoticeKeys.value)
    acknowledgedHistoryWarnings.value = new Set([...acknowledgedHistoryWarnings.value, ...historyNoticeKeys.value])
  }

  const relevantHistoryEntries = computed(() => history.all().filter(entry => profiles.profile(entry.context.profileId)?.revision === entry.context.profileRevision
    && workspace.projects.some(project => project.projectId === entry.context.projectId && sameProjectPath(project.selectedPath, entry.context.projectPath))))
  const currentHistoryEntries = computed(() => relevantHistoryEntries.value.filter(entry => entry.loaded && !entry.error))
  // 横幅只对「确有省略」的来源亮起：读失败/整条省略才会让列表可能不完整。
  // 有界观察（Claude 头窗口按设计在 cwd+标题处停）本身不是列表缺陷，把每个
  // 正常扫描都标成“可能不完整”会让横幅变成常驻噪音（实测 48/50 行如此）。
  // metadataIncomplete 仍继续承担缺席证据抑制，二者语义在此分叉。
  const historyMetadataPartial = computed(() => currentHistoryEntries.value.some(entry => (entry.readFailures?.length ?? 0) > 0))
  const historyReadWarnings = computed(() => {
    const warnings = createWorkspaceSourceWarnings()
    for (const entry of currentHistoryEntries.value) for (const code of entry.readFailures ?? []) {
      warnings.add(historyWarningSource(entry.context.cli), { code, stage: 'read-source-enumeration' })
    }
    return warnings.items
  })
  // Fixed diagnostics + canonical context/root identity, never raw transcript data.
  // Project navigation does not reset this workspace-owned acknowledgement.
  const historyNoticeKeys = computed(() => currentHistoryEntries.value.flatMap(entry => [
    ...(entry.readFailures ?? []).map(code => JSON.stringify([entry.key, entry.sourceRootKey ?? null, code, 'source'])),
    ...entry.sessions.filter(row => row.readFailure).map(row => JSON.stringify([entry.key, entry.sourceRootKey ?? null, row.readFailure, row.sessionKey])),
  ]))
  const historyNoticeDismissed = computed(() => historyNoticeKeys.value.length > 0
    && historyNoticeKeys.value.every(key => acknowledgedHistoryWarnings.value.has(key)))
  // Adapter-owned forced reads (close/resume) can finish outside refresh(). Keep
  // their authenticated fixed diagnostics discoverable, never infer health from
  // their absence in the positive-session projection.
  const sourceDiagnostics = computed(() => {
    const warnings = createWorkspaceSourceWarnings(sourceWarnings.value, sourceWarningsTruncated.value)
    for (const entry of relevantHistoryEntries.value) if (!entry.loading && entry.error) {
      warnings.add(historyWarningSource(entry.context.cli), { code: entry.error, stage: entry.diagnosticStage })
    }
    return { warnings: warnings.items, truncated: warnings.truncated }
  })
  const sourceDiagnosticsWarnings = computed(() => sourceDiagnostics.value.warnings)
  const sourceDiagnosticsTruncated = computed(() => sourceDiagnostics.value.truncated)
  const sourceIssuesAvailable = computed(() => sourceDiagnosticsWarnings.value.length > 0 || historyMetadataPartial.value)
  const allHistoryIssues = computed<WorkspaceHistoryIssue[]>(() => relevantHistoryEntries.value.flatMap(entry => {
    if (entry.loading) return []
    const source = historyWarningSource(entry.context.cli)
    const configurationName = profiles.profile(entry.context.profileId)?.name ?? ''
    const stage = entry.error ? entry.diagnosticStage : 'read-source-enumeration' as const
    const issue = (code: string, session?: NativeHistoryEntry['sessions'][number]): WorkspaceHistoryIssue => ({
      key: JSON.stringify([entry.key, entry.sourceRootKey ?? null, code, stage, session?.sessionKey ?? null]),
      entryKey: entry.key, warningKey: workspaceWarningKey({ source, code, stage }), source, code, stage,
      configurationName, projectPath: entry.context.projectPath,
      ...(session ? { sessionId: session.nativeSessionId, sessionTitle: session.lastKnownTitle || session.title } : {}),
    })
    // 汇总没有条目身份时仍保留范围未知，不把同代码的其它会话推断为全部受影响条目。
    return [...(entry.error ? [issue(entry.error)] : (entry.readFailures ?? []).map(code => issue(code))),
      ...(!entry.error ? entry.sessions.filter(row => row.readFailure).map(row => issue(row.readFailure!, row)) : [])]
  }).sort((a, b) => a.key.localeCompare(b.key, 'en')))
  const historyIssues = computed(() => allHistoryIssues.value.slice(0, 48))
  const historyIssuesTruncated = computed(() => allHistoryIssues.value.length > 48)
  const historyRecoveryError = ref(false)
  const historyRecoveryOwners = new Set<string>()
  function currentHistoryIssue(key: string) {
    const issue = allHistoryIssues.value.find(row => row.key === key)
    const entry = issue && relevantHistoryEntries.value.find(row => row.key === issue.entryKey)
    return issue && entry ? { issue, entry } : undefined
  }
  function reconcileHistoryWarningSnapshot() {
    const isHistory = (source: WorkspaceWarningSource) => source === 'claude-history' || source === 'codex-history' || source === 'opencode-history'
    const warnings = createWorkspaceSourceWarnings(sourceWarnings.value.filter(row => !isHistory(row.source)), sourceWarningsTruncated.value)
    const owners: Omit<WorkspaceSourceConfiguration, 'name'>[] = []
    const identities = new Map<string, Set<string>>()
    for (const entry of relevantHistoryEntries.value) if (entry.error) {
      const warning = warnings.add(historyWarningSource(entry.context.cli), { code: entry.error, stage: entry.diagnosticStage })
      const key = workspaceWarningKey(warning)
      const set = identities.get(key) ?? new Set<string>()
      set.add(JSON.stringify([key, entry.context.profileId, entry.context.profileRevision, entry.sourceRootKey ?? null]))
      identities.set(key, set)
      if (!owners.some(owner => owner.warningKey === key && owner.profileId === entry.context.profileId)) owners.push({ warningKey: key, profileId: entry.context.profileId, profileRevision: entry.context.profileRevision })
    }
    sourceWarnings.value = warnings.items
    sourceWarningsTruncated.value = warnings.truncated
    sourceConfigurationOwners.value = owners
    sourceNoticeKeys.value = warnings.identityKeys().flatMap(key => [...(identities.get(key) ?? [key])])
    if (warnings.truncated) sourceNoticeKeys.value.push('truncated')
    const currentKeys = new Set(sourceNoticeKeys.value)
    acknowledgedSourceWarnings.value = new Set([...acknowledgedSourceWarnings.value].filter(key => currentKeys.has(key)))
    error.value = warnings.items.length && !fatal.value ? 'workspaceRuntimePartial' : null
  }
  async function retryHistoryIssue(key: string) {
    const target = currentHistoryIssue(key)
    if (!target || disposed || loading.value || historyRecoveryOwners.has(target.entry.key)) return
    const context = { ...target.entry.context }
    historyRecoveryOwners.add(target.entry.key)
    historyRecoveryError.value = false
    try {
      // 只重新读取已授权配置/项目，不重放生命周期命令或改写 CLI 原始历史。
      await history.load({ ...context, force: true, retainOnFailure: true }).catch(() => {})
      if (disposed || !relevantHistoryEntries.value.some(row => row.key === target.entry.key)) return
      reconcileHistoryWarningSnapshot()
      await catalog.refresh(context.projectPath)
    } catch { historyRecoveryError.value = true }
    finally { historyRecoveryOwners.delete(target.entry.key) }
  }
  async function openHistoryIssueProject(key: string) {
    const target = currentHistoryIssue(key)
    if (!target || disposed) return
    historyRecoveryError.value = false
    try { await openInFileManager(target.entry.context.projectPath) }
    catch { historyRecoveryError.value = true }
  }
  function retireHistoryAcknowledgements() {
    const current = new Set(historyNoticeKeys.value)
    acknowledgedHistoryWarnings.value = new Set([...acknowledgedHistoryWarnings.value].filter(key => current.has(key)))
  }
  // A forced read temporarily removes loaded entries. Do not treat that as health.
  watch(historyNoticeKeys, () => {
    if (!loading.value && !history.all().some(entry => entry.loading)) retireHistoryAcknowledgements()
  }, { flush: 'post' })
  const ready = ref(false)
  const loading = ref(false)
  const sourceChecksPending = computed(() => loading.value || relevantHistoryEntries.value.some(entry => entry.loading))
  const fatal = computed(() => ready.value && !loading.value && !projects.loaded
    && profiles.status === 'error' && workspace.status === 'error' && app.managedProjectsStatus === 'error'
    && !openSessions.value.length && !catalog.sessions.length && !app.cachedProjects.length
    && !profiles.profiles.length && !workspace.projects.length && !projects.pinnedProjects.length)
  let retryRequest: { request: WorkspaceRequest; owns: () => boolean; resumeTarget?: UnifiedSession } | null = null
  let retryResumeTarget: { sequence: number; target: UnifiedSession } | null = null
  const cliProblems = computed(() => (['claude', 'codex', 'opencode'] as const).flatMap(cli => {
    const latest = [...native.tabs.values()].reverse().filter(tab => tab.cli === cli).sort((a, b) => b.lastActivityAt - a.lastActivityAt)[0]
    const failed = latest?.status === 'failed' ? latest : null
    const code = latest?.status === 'running' ? null : failed?.errorCode === 'AUTH_REQUIRED' ? 'AUTH_REQUIRED'
      : draft.cliAvailability[cli] === 'unavailable' || ['CLI_NOT_FOUND', 'PROGRAM_UNAVAILABLE'].includes(failed?.errorCode ?? '') ? 'CLI_NOT_FOUND' : null
    return code ? [{ cli, messageKey: mapSafeUserError(code, 'launch').messageKey }] : []
  }))
  function retryAction() {
    const pending = retryRequest
    retryRequest = null
    if (pending?.owns()) {
      if (pending.resumeTarget) retryResumeTarget = { sequence: shell.requestSequence + 1, target: pending.resumeTarget }
      shell.requestWorkspaceAction(pending.request)
    }
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
  const diagnosticsOwner = shallowRef<{ id: string; owns: () => boolean } | null>(null)
  const diagnostics = computed(() => {
    const owner = diagnosticsOwner.value
    if (!owner || shell.section !== 'workspace' || !owner.owns()) return null
    const row = catalog.sessions.find(row => row.id === owner.id)
    if (!row) return null
    const open = openSessions.value.some(session => session.id === row.id)
    return projectSessionDiagnostics(row, open, row.runtime === 'native-cli' && open ? native.tab(row.adapterSessionId)?.generation : undefined, catalog.isPreparingSession(row.id))
  })
  function closeDiagnostics() { diagnosticsOwner.value = null }
  watch(diagnostics, value => { if (!value) closeDiagnostics() }, { flush: 'sync' })
  watch(() => [shell.navigationSequence, catalog.activeSessionId], closeDiagnostics, { flush: 'sync' })
  function requireHost() {
    if (disposed || !host.value) throw new Error('TERMINAL_HOST_NOT_READY')
    return host.value
  }
  async function createNativeTab(input: NativeRuntimeCreateInput) {
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
    if (matches.length !== 1 || (source && !sameProjectPath(source.projectPath, input.projectPath))) {
      throw new Error(source ? 'PROJECT_IDENTITY_CHANGED' : 'PROJECT_NOT_FOUND')
    }
    // Paths selected by the frontend do not create registration or bridge authority.
    let action = input.action
    let preparedIdentity: { tabId: string; requestId: string; runId: string } | undefined
    if (input.cli === 'opencode' && action.kind === 'new') {
      preparedIdentity = { tabId: createNativeId('tab'), requestId: createNativeId('request'),
        runId: createNativeId('run') }
      let nativeSessionId: string | undefined
      try {
        nativeSessionId = await nativePrepareOpencodeSession({
          ...preparedIdentity, generation: 1, profileId: profile.id,
          expectedProfileRevision: profile.revision, cli: input.cli,
          launchCwd: matches[0].selectedPath, action: { kind: 'new' },
          extraArgs: [], cols: 80, rows: 24,
        })
      } catch (failure) {
        const code = failure && typeof failure === 'object' && 'code' in failure ? failure.code : undefined
        // 只有后端明确保证尚未发送创建请求，才能降级到原生交互；未知结果不能再启动一次。
        if (code !== 'OPENCODE_SESSION_PREPARE_UNSUPPORTED'
          && code !== 'OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE') throw failure
      }
      if (disposed || app.isProjectAdmissionBlocked(input.projectPath)
        || profiles.profile(profile.id)?.revision !== profile.revision
        || !workspace.projects.some(project => project.projectId === matches[0].projectId
          && sameProjectPath(project.selectedPath, matches[0].selectedPath))) {
        throw new Error('STALE_NATIVE_ATTEMPT')
      }
      if (nativeSessionId) action = { kind: 'resume-id', nativeSessionId }
    }
    const tab = native.create({ cli: input.cli, projectId: matches[0].projectId, projectPath: matches[0].selectedPath,
      profileId: profile.id, profileRevision: profile.revision, action, preparedIdentity,
      title: input.title, sourceSessionKey: input.sourceSessionKey })
    if (!source) awaitingSuccess.set(tab.tabId, { attempt: captureNativeAttempt(tab), projectPath: tab.projectPath,
      cli: tab.cli, profileId: tab.profileId, profileRevision: tab.profileRevision })
    return tab
  }
  async function restartNative(tabId: string, continuation?: NativeRestartContinuation) {
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
    const verifyContinuation = async () => { if (continuation) {
      const proof=await nativeGetScope({kind:'run',runId:attempt.runId,generation:attempt.generation})
      if (!matchesNativeAttempt(native.tab(tabId),attempt) || !ownsSource()) throw new Error('STALE_SESSION_ATTEMPT')
      if (proof.cli!==frozen.cli || proof.profileId!==frozen.profileId || proof.profileRevision!==frozen.profileRevision || proof.sourceRootKey!==continuation.sourceRootKey
        || proof.target.kind!=='run' || proof.target.runId!==attempt.runId || proof.target.generation!==attempt.generation) throw new Error('SOURCE_CHANGED')
      requireSource()
    } }
    await verifyContinuation()
    if (frozen.status === 'starting' || frozen.status === 'running') await requireHost().stopNative(tabId, attempt)
    if (!matchesNativeAttempt(native.tab(tabId), attempt)) throw new Error('STALE_NATIVE_ATTEMPT')
    if (!ownsSource()) throw new Error('STALE_SESSION_ATTEMPT')
    requireSource()
    await verifyContinuation()
    const restarted = native.restart(tabId, { cli: frozen.cli, profileId: frozen.profileId, profileRevision: frozen.profileRevision },continuation)
    awaitingSuccess.set(restarted.tabId, { attempt: captureNativeAttempt(restarted), projectPath: restarted.projectPath, cli: restarted.cli, profileId: restarted.profileId, profileRevision: restarted.profileRevision })
    return restarted
  }
  if (enabled) {
    // Subscribe attention before child TerminalView status consumers mount.
    attention.init()
    // 折叠 legacy/native 重复历史行的前提是根身份已证明：后端给出 legacy 固定根
    // （~/.claude）的 native key；终端自身另持有本次启动的环境/shell 证明。取不到时
    // 保持未知，store 端不折叠（v0.20.1 的 fail-closed 立场）。该 IPC 晚于首次
    // 历史加载完成时，仅更新非响应式状态不会改动已投影的行，必须重投影一次。
    void legacyClaudeRootKey().then(proof => {
      if (disposed || !proof?.rootKey) return
      legacyClaudeRoot = proof.rootKey
      catalog.configureAdapters(sessionAdapters, { legacyClaudeRootKey: legacyClaudeRoot, validateLegacyClaudeRoot })
      // 该 IPC 晚于首次历史加载完成时，仅更新非响应式状态不会改动已投影的行。
      void refreshCatalog().catch(() => undefined)
    }).catch(() => undefined)
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
      if ((!query.cli || query.cli === 'claude') && cliSupport.isEnabled('claude')) {
        for (let i = 0; i < paths.length && !disposed; i += 2) {
          const results = await Promise.all(paths.slice(i, i + 2).map(path => legacy.loadHistoryFor(path)))
          if (results.some(result => !result.ok)) partial = true
        }
      }
      const contexts = profiles.profiles.flatMap(profile => profile.cli === 'shell' || (query.cli && profile.cli !== query.cli) || !cliSupport.isEnabled(profile.cli as UnifiedCliKind) ? []
        : workspace.projects.filter(project => query.scope === 'all' || sameProjectPath(project.selectedPath, query.projectPath)).map(project => ({
          cli: profile.cli as UnifiedCliKind, profileId: profile.id, profileRevision: profile.revision, projectId: project.projectId, projectPath: project.selectedPath,
        })))
      for (let i = 0; i < contexts.length && !disposed; i += 2) await Promise.all(contexts.slice(i, i + 2).map(async context => {
        // 恢复搜索显式重读当前项目；普通 owner 投影不经过此 loader。
        try { const entry = await history.load({ ...context, force: query.scope !== 'all', retainOnFailure: true }); if (entry.error || entry.metadataIncomplete) partial = true } catch { partial = true }
      }))
      return partial
    })
    const sessionAdapters = [
      createLegacyClaudeAdapter({ store: legacy, metadata: projects, attention, validateHistoryRoot: validateLegacyClaudeRoot, captureProjectAdmission: app.captureProjectAdmission, projectPaths: () => [...new Set([...legacyPaths.value, ...projects.pinnedProjects])],
        runtime: {
          startTab: id => {
            const tab = legacy.tabs.get(id)
            if (!tab || app.isProjectAdmissionBlocked(tab.projectPath)) throw new Error('PROJECT_REMOVAL_IN_PROGRESS')
            return requireHost().startLegacy(id)
          }, stopTab: id => requireHost().stopLegacy(id),
          restartTab: id => requireHost().restartLegacy(id), renameTab: (id, title, admission) => requireHost().renameLegacy(id, title, admission),
        } }),
      createNativeCliAdapter({ tabs: native, history, metadata: projects, legacyClaudeRootKey: validateLegacyClaudeRoot, archive: { getArchivedSessions: legacy.getArchivedSessions, archiveSession: projects.archiveSession, restoreSession: projects.restoreSession },
        nativeRename: { rename: nativeRenameSession },
        runSource: tab => nativeGetScope({kind:'run',runId:tab.runId,generation:tab.generation}),
        cliUnarchive: { unarchive: (context, nativeSessionId) => {
          if (app.isProjectAdmissionBlocked(context.projectPath)) throw new Error('PROJECT_REMOVAL_IN_PROGRESS')
          // 只有 codex 有 CLI 侧归档；claude 行不进入此路径。
          if (context.cli !== 'codex') throw new Error('CLI_UNARCHIVE_UNAVAILABLE')
          return nativeUnarchiveSession({ requestId: createNativeId('request'), cli: 'codex', profileId: context.profileId,
            expectedProfileRevision: context.profileRevision, projectPath: context.projectPath, nativeSessionId })
        } },
        runtime: { createTab: createNativeTab, restartTab: restartNative,
          stopTab: tab => requireHost().stopNative(tab.tabId, captureNativeAttempt(tab)),
          renameTab: (tabId, title, admission) => requireHost().renameNative(tabId, title, admission) } }),
    ]
    catalog.configureAdapters(sessionAdapters, { legacyClaudeRootKey: legacyClaudeRoot, validateLegacyClaudeRoot })
  }

  async function refresh() {
    if (!enabled || disposed) return
    const owner = ++refreshOwner
    const current = () => !disposed && owner === refreshOwner
    loading.value = true
    error.value = null
    const warnings = createWorkspaceSourceWarnings()
    const configurationOwners = new Map<string, Omit<WorkspaceSourceConfiguration, 'name'>>()
    const historyWarningIdentities = new Map<string, Set<string>>()
    const addHistoryWarning = (context: { cli: UnifiedCliKind; profileId: string; profileRevision: string; sourceRootKey?: string }, failure: unknown) => {
      const warning = warnings.add(historyWarningSource(context.cli), failure)
      const warningKey = workspaceWarningKey(warning)
      const identities = historyWarningIdentities.get(warningKey) ?? new Set<string>()
      identities.add(JSON.stringify([warningKey, context.profileId, context.profileRevision, context.sourceRootKey ?? null]))
      historyWarningIdentities.set(warningKey, identities)
      configurationOwners.set(JSON.stringify([warningKey, context.profileId]), {
        warningKey, profileId: context.profileId, profileRevision: context.profileRevision,
      })
    }
    const settle = async (source: WorkspaceWarningSource, operation: () => Promise<unknown>) => {
      try { await operation() } catch (failure) { warnings.add(source, failure) }
    }
    await Promise.all([
      settle('project-metadata', () => projects.ensureLoaded()),
      settle('project-discovery', async () => {
        await app.loadManagedProjects()
        const rows = app.cachedProjects
        if (!current()) return
        legacyPaths.value = rows.map(row => row.path)
        const paths = [...new Set([...legacyPaths.value, ...projects.pinnedProjects, ...[...legacy.tabs.values()].map(tab => tab.projectPath)])]
        // 关闭的 CLI 不做历史扫描：目录行的过滤由 catalog 兜底，这里省掉全部 IO。
        if (cliSupport.isEnabled('claude')) for (let i = 0; i < paths.length && current(); i += 2) {
          await Promise.all(paths.slice(i, i + 2).map(path => settle('legacy-history', async () => {
            if (!(await legacy.loadHistoryFor(path, true)).ok) warnings.add('legacy-history')
          })))
        }
      }),
      settle('configurations', async () => {
        const results = await Promise.allSettled([profiles.load(), workspace.load()])
        if (results[0].status === 'rejected') warnings.add('configurations', results[0].reason)
        if (results[1].status === 'rejected') warnings.add('registrations', results[1].reason)
        if (results.some(result => result.status === 'rejected')) return
        // 旧模板的 false 与用户主动关闭无法区分；读取工作区不能猜测来源并覆写。
        await draft.refreshAvailability()
        // 同 profile 的全部项目合并为一次后端观察；逐项目加载会让同一棵历史树被完整扫描 N 次。
        const groups = new Map<string, { cli: UnifiedCliKind; profileId: string; profileRevision: string; projects: { projectId: string; projectPath: string }[] }>()
        for (const profile of profiles.profiles) {
          // 关闭的 CLI 不组装历史观察组；validKeys 随之不含其 context，
          // 下面的失效循环会把已加载的 entry 一并撤销。
          if (profile.cli === 'shell' || !cliSupport.isEnabled(profile.cli as UnifiedCliKind)) continue
          groups.set(`${profile.id}\u0000${profile.revision}`, { cli: profile.cli as UnifiedCliKind, profileId: profile.id, profileRevision: profile.revision,
            projects: workspace.projects.map(project => ({ projectId: project.projectId, projectPath: project.selectedPath })) })
        }
        const contexts = [...groups.values()]
        // 清理 profile revision 变更、profile 删除或项目移除后残留的历史 entry：
        // 否则旧 revision 的幽灵行会与新 observation 的行并存显示（同会话重复）。
        const validKeys = new Set(contexts.flatMap(group => group.projects.map(project => nativeHistoryContextKey({
          cli: group.cli, profileId: group.profileId, profileRevision: group.profileRevision, projectId: project.projectId, projectPath: project.projectPath,
        }))))
        for (const entry of history.all()) {
          // invalidate 同时撤销异步 owner，读取中的旧配置也不能在迟到完成时恢复。
          if (!validKeys.has(nativeHistoryContextKey(entry.context))) history.invalidate(entry.context)
        }
        // Match the backend's two-reader budget; isolate each source failure.
        for (let i = 0; i < contexts.length && current(); i += 2) {
          await Promise.all(contexts.slice(i, i + 2).map(async group => {
            try {
              const loaded = await history.loadGrouped(group, { force: true, retainOnFailure: true })
              for (const entry of loaded.values()) {
                if (entry.error) addHistoryWarning({ ...entry.context, sourceRootKey: entry.sourceRootKey }, { code: entry.error, stage: entry.diagnosticStage })
              }
            } catch (failure) {
              for (const project of group.projects) {
                addHistoryWarning({ cli: group.cli, profileId: group.profileId, profileRevision: group.profileRevision, ...project,
                  sourceRootKey: history.get({ cli: group.cli, profileId: group.profileId, profileRevision: group.profileRevision, ...project })?.sourceRootKey }, failure)
              }
            }
          }))
        }
      }),
    ])
    if (!current()) return
    await settle('catalog', () => catalog.initialize())
    if (!current()) return
    // Even unavailable projects.json must not prevent the readable runtime catalog.
    await settle('catalog', () => catalog.refresh())
    if (!current()) return
    ready.value = true
    loading.value = false
    sourceWarnings.value = warnings.items
    sourceWarningsTruncated.value = warnings.truncated
    sourceConfigurationOwners.value = [...configurationOwners.values()].filter(owner => warnings.items.some(warning => workspaceWarningKey(warning) === owner.warningKey))
    sourceNoticeKeys.value = warnings.identityKeys().flatMap(key => {
      const identities = historyWarningIdentities.get(key)
      return identities ? [...identities] : [key]
    })
    if (warnings.truncated) sourceNoticeKeys.value.push('truncated')
    // Only a completed owned check retires acknowledgements. Loading's temporary
    // error=null and background catalog publication cannot resurrect a notice.
    const currentKeys = new Set(sourceNoticeKeys.value)
    acknowledgedSourceWarnings.value = new Set([...acknowledgedSourceWarnings.value].filter(key => currentKeys.has(key)))
    retireHistoryAcknowledgements()
    error.value = warnings.items.length && !fatal.value ? 'workspaceRuntimePartial' : null
  }

  // 支持开关切换后触发完整工作区刷新：catalog.refresh 只读历史缓存，停用期间
  // 失效的 entry（含启动即停用的冷缓存）必须经由这里的历史观察组重新装载。
  // Pinia 会解包 setup store 的 ref，必须用 getter 观察每次替换后的新对象。
  watch(() => cliSupport.support, () => { if (!disposed) void refresh() })

  async function resumeDirect(row: UnifiedSession, claimFeedback: (target: UnifiedSession) => void) {
    if (shell.section !== 'workspace') return
    const sequence = shell.requestSequence
    const navigation = shell.navigationSequence
    const canAdmit = () => !disposed && shell.section === 'workspace'
      && shell.requestSequence === sequence && shell.navigationSequence === navigation
    // Freeze the selected source. The adapters still verify its current history,
    // configuration revision and registered project before admitting a process.
    const frozen = { ...row, ...(row.nativeOrigin ? { nativeOrigin: { ...row.nativeOrigin } } : {}) }
    // History and ended Legacy terminals claim selection at different async
    // boundaries. Capture feedback only after the actual owning selection claim.
    await catalog.resumeCatalogSession(frozen, canAdmit, () => claimFeedback(frozen))
  }

  async function dispatch(request: WorkspaceRequest, claimResumeFeedback: (target: UnifiedSession) => void, resumeTarget?: UnifiedSession): Promise<boolean | 'rename-written'> {
    if (request.kind === 'refresh') { await refresh(); return true }
    if (request.kind === 'rename-cancel') { catalog.cancelRename(request.sessionId); return true }
    if (request.kind === 'new-session') {
      const { intent, ...project } = request.project
      if (intent === 'restore') {
        shell.requestWorkspaceAction({ kind: 'restore-session', project, mode: 'history' })
      } else if (intent === 'claude' || intent === 'codex' || intent === 'opencode') {
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
    const session = resumeTarget ?? catalog.sessions.find(value => value.id === request.sessionId)
    if (!session) return false
    if (request.kind === 'menu-action' && request.action === 'view-diagnostics') {
      if (shell.section === 'workspace') diagnosticsOwner.value = { id: session.id, owns: catalog.captureSessionOwnership(session.id) }
      return true
    }
    const preparing = catalog.isPreparingSession(session.id)
    if (preparing) {
      if (request.kind === 'activate') return false
      if ('action' in request) {
        if (request.action === 'discard-creation') { await catalog.discardPreparation(session.id); return true }
        if (request.action === 'retry' && session.processState === 'failed') { await catalog.restartSession(session.id); return true }
        if (request.action === 'cancel-start' && session.processState === 'starting') { await catalog.stopSession(session.id); return true }
        if (request.action === 'close' && session.processState !== 'unknown') { await catalog.closeSession(session.id); return true }
      }
      return false
    }
    const open = openSessions.value.some(value => value.id === session.id)
    if (request.kind === 'activate') {
      if (!open) return false
      await catalog.activateSession(session.id)
      return true
    }
    if (request.kind === 'restore-archive') { await catalog.restoreArchivedSession(session.id); return true }
    if (request.kind === 'rename') {
      const result = await catalog.renameSession(session.id, request.title, true)
      return result === 'input-written' ? 'rename-written' : true
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
      case 'rename': catalog.beginRename(session.id); return true
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
        await catalog.closeSession(session.id); return true
      case 'archive':
        if (live) { if (shell.section === 'workspace') catalog.beginSessionConfirmation('stop-and-archive', session.id); return true }
        await catalog.archiveSession(session.id); return true
      case 'resume':
        await resumeDirect(session, claimResumeFeedback)
        return true
      case 'restore-archive': await catalog.restoreArchivedSession(session.id); return true
      case 'clear-unread':
        if (request.kind !== 'menu-action') return false
        await catalog.clearUnread(session.id, request.expectedNotice); return true
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
    const retryTarget = retryResumeTarget?.sequence === sequence ? retryResumeTarget.target : undefined
    retryResumeTarget = null
    let directResumeTarget: UnifiedSession | undefined
    catalog.closeSessionConfirmation()
    closeDiagnostics()
    const navigation = shell.navigationSequence
    let feedback = catalog.captureFeedbackOwner()
    let ownsFeedbackSession = 'sessionId' in request && typeof request.sessionId === 'string' && catalog.sessions.some(row => row.id === request.sessionId)
      ? catalog.captureSessionOwnership(request.sessionId) : () => true
    const current = () => !disposed && shell.requestSequence === sequence && shell.navigationSequence === navigation && feedback() && ownsFeedbackSession()
    const claimResumeFeedback = (target: UnifiedSession) => {
      feedback = catalog.captureFeedbackOwner()
      directResumeTarget = target
      // A failed exact-source read may remove its history row. Its owned error
      // still belongs to this explicit request, even when that row is gone.
      ownsFeedbackSession = () => true
    }
    retryRequest = null
    claimedSequence = sequence // Claim before async work; never retry on reactive changes.
    try {
      const dispatched = await dispatch(request, claimResumeFeedback, retryTarget)
      if (dispatched) {
        shell.clearWorkspaceRequest(sequence)
        const key = request.kind === 'rename' ? (dispatched === 'rename-written' ? 'feedbackRenameSent' : 'feedbackRenamed')
          : 'action' in request && request.action === 'copy-session-id' ? 'feedbackCopied'
          : 'action' in request && request.action === 'archive' && !catalog.sessionConfirmation ? 'feedbackArchived' : null
        if (key) catalog.publishActionSuccess(current, key)
      }
    } catch (failure) {
      // This dispatch has finished unsuccessfully. Retire only its own pending
      // banner; retry still requires a new explicit request and sequence.
      shell.clearWorkspaceRequest(sequence)
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
        if (mapSafeUserError(code, 'session').retryable) retryRequest = { request, owns: () => current() && ownsFeedbackSession(), resumeTarget: directResumeTarget }
      }
      // No side effect is replayed. Retry is a new, explicit user request.
    }
  }, { immediate: true })
  async function refreshCatalog(projectPaths?: string[]) {
    if (!enabled || !ready.value || disposed || loading.value) return
    const owner = refreshOwner
    try {
      if (projectPaths) await Promise.all(projectPaths.map(path => catalog.refresh(path, { background: true })))
      else await catalog.refresh(undefined, { background: true })
    } catch (failure) {
      if (disposed || owner !== refreshOwner || loading.value) return
      const warnings = createWorkspaceSourceWarnings(sourceWarnings.value, sourceWarningsTruncated.value)
      const warning = warnings.add('catalog', failure)
      sourceNoticeKeys.value = [...new Set([...sourceNoticeKeys.value, workspaceWarningKey(warning), ...(warnings.truncated ? ['truncated'] : [])])]
      sourceWarnings.value = warnings.items
      sourceWarningsTruncated.value = warnings.truncated
      error.value = 'workspaceRuntimePartial'
    }
  }
  const pendingCatalogProjects = new Set<string>()
  let catalogRefreshScheduled = false
  function queueCatalogProjects(...paths: string[]) {
    if (!enabled || !ready.value || disposed || loading.value || !paths.length) return
    for (const path of paths) pendingCatalogProjects.add(normalizePath(path))
    if (catalogRefreshScheduled) return
    catalogRefreshScheduled = true
    void nextTick().then(async () => {
      catalogRefreshScheduled = false
      const changed = [...pendingCatalogProjects]
      pendingCatalogProjects.clear()
      await refreshCatalog(changed)
    })
  }
  useLegacyHistoryRefresh({
    ready: () => enabled && ready.value && !loading.value && !disposed && cliSupport.isEnabled('claude'),
    validateRoot: validateLegacyClaudeRoot,
    publish: queueCatalogProjects,
  })
  // 身份确认/交互/标题失效才请求历史 IO；普通输出和焦点不使历史失效。
  // 每个 tab 仅保留最新请求，同 profile 合并项目，读取中至多留一批后继任务。
  type HistoryOwner = { tab: NativeCliTab; attempt: ReturnType<typeof captureNativeAttempt>;
    context: Pick<NativeCliTab, 'cli' | 'profileId' | 'profileRevision' | 'projectId' | 'projectPath'> }
  const pendingHistoryOwners = new Map<string, HistoryOwner>()
  let historyTimer: ReturnType<typeof setTimeout> | undefined
  let historyReading = false
  function historyCue(tab: NativeCliTab): { identity: string | null; reply: string | null; prompt: string | null; titleRevision: number } | null {
    if (tab.cli === 'opencode' && tab.action.kind === 'resume-id') {
      return { identity: JSON.stringify([tab.requestId, tab.runId, tab.generation, tab.action.nativeSessionId, tab.status]),
        reply: null, prompt: null, titleRevision: tab.titleChangeRevision ?? 0 }
    }
    if (tab.cli !== 'claude' || tab.action.kind === 'raw' || tab.status !== 'running') return null
    const attempt = captureNativeAttempt(tab)
    const observing = tab.observationState === 'active'
    const notice = observing && native.hasOwnedObservationNotice(tab.tabId, attempt) ? tab.observationNotice : undefined
    const reply = notice?.recent?.kind === 'reply-ended' ? notice.recent : notice?.unreadReplyEnd
    const prompt = notice?.lastPromptSubmitted?.kind === 'prompt-submitted' ? notice.lastPromptSubmitted
      : notice?.recent?.kind === 'prompt-submitted' ? notice.recent : null
    const key = [attempt.requestId, attempt.runId, attempt.generation]
    // 已冻结的恢复来源或本次运行已证明的身份足以处理 OSC；hook 事件仍要求观察已激活。
    return tab.sourceSessionKey || tab.observedSessionId || reply || prompt ? {
      identity: observing && tab.observedSessionId ? JSON.stringify([...key, tab.observedSessionId]) : null,
      reply: reply ? JSON.stringify([...key, reply.eventId]) : null,
      prompt: prompt ? JSON.stringify([...key, prompt.eventId]) : null,
      titleRevision: tab.titleChangeRevision ?? 0,
    } : null
  }
  function scheduleHistoryRead() {
    if (disposed || historyReading || loading.value || !ready.value || !pendingHistoryOwners.size) return
    if (historyTimer) clearTimeout(historyTimer)
    historyTimer = setTimeout(() => { historyTimer = undefined; void refreshObservedHistory() }, 250)
  }
  async function refreshObservedHistory() {
    if (disposed || historyReading || loading.value || !ready.value) return
    const groups = new Map<string, HistoryOwner[]>()
    for (const owner of pendingHistoryOwners.values()) {
      const key = JSON.stringify([owner.context.cli, owner.context.profileId, owner.context.profileRevision])
      const group = groups.get(key) ?? []
      group.push(owner)
      groups.set(key, group)
    }
    pendingHistoryOwners.clear()
    historyReading = true
    try {
      const reads = [...groups.values()]
      for (let index = 0; index < reads.length && !disposed; index += 2) {
        await Promise.all(reads.slice(index, index + 2).map(group => {
          // 前一批可能等待磁盘；每一组真正发起前重新核对冻结 owner/context。
          const current = group.filter(({ tab, attempt, context }) => native.tabs.get(tab.tabId) === tab
            && matchesNativeAttempt(tab, attempt) && historyCue(tab)
            && (context.cli === 'claude' || context.cli === 'opencode') && cliSupport.isEnabled(context.cli)
            && tab.profileId === context.profileId && tab.profileRevision === context.profileRevision
            && tab.projectId === context.projectId && sameProjectPath(tab.projectPath, context.projectPath)
            && profiles.profile(context.profileId)?.revision === context.profileRevision
            && workspace.projects.some(project => project.projectId === context.projectId && sameProjectPath(project.selectedPath, context.projectPath)))
          if (!current.length) return
          const context = current[0].context
          const projects = new Map(current.map(owner => [owner.context.projectId, { projectId: owner.context.projectId, projectPath: owner.context.projectPath }]))
          // 自动投影失败保留已有行，但 store 仍发布错误且撤销 absence 证明。
          return history.loadGrouped({ cli: context.cli, profileId: context.profileId, profileRevision: context.profileRevision,
            projects: [...projects.values()] }, { force: true, retainOnFailure: true }).catch(() => undefined)
        }))
      }
    } finally { historyReading = false; scheduleHistoryRead() }
  }
  watch(() => [ready.value, loading.value], scheduleHistoryRead)
  // 每个运行 owner 独立监听；状态变化不再遍历全部历史。移动或替换时刷新旧、新项目。
  // AI 标题晚于 reply-ended 异步写盘；最多在约 15、30、60 秒补读，避免持续轮询。
  const lateTitleReadDelays = [15_000, 15_000, 30_000]
  type CatalogOwner = { projectPath: string; stop: () => void; historyCue: ReturnType<typeof historyCue>; titleReadTimer?: ReturnType<typeof setTimeout> }
  const catalogOwnerWatches = new Map<object, CatalogOwner>()
  watch(() => [...legacy.tabs.values(), ...native.tabs.values()], tabs => {
    const current = new Set<object>(tabs)
    for (const [tab, owner] of catalogOwnerWatches) {
      if (current.has(tab)) continue
      owner.stop(); catalogOwnerWatches.delete(tab)
      if (owner.titleReadTimer) clearTimeout(owner.titleReadTimer)
      const removedId = (tab as { tabId: string }).tabId
      if (pendingHistoryOwners.get(removedId)?.tab === tab) pendingHistoryOwners.delete(removedId)
      queueCatalogProjects(owner.projectPath)
    }
    for (const tab of tabs) {
      if (catalogOwnerWatches.has(tab)) continue
      const nativeTab = native.tabs.get(tab.tabId) === tab ? tab as NativeCliTab : null
      const owner: CatalogOwner = { projectPath: tab.projectPath, stop: () => {}, historyCue: nativeTab && historyCue(nativeTab) }
      const queueHistory = () => {
        if (!nativeTab) return
        pendingHistoryOwners.set(nativeTab.tabId, { tab: nativeTab, attempt: captureNativeAttempt(nativeTab), context: {
          cli: nativeTab.cli, profileId: nativeTab.profileId, profileRevision: nativeTab.profileRevision,
          projectId: nativeTab.projectId, projectPath: nativeTab.projectPath,
        } })
        scheduleHistoryRead()
      }
      // 输出/输入只推进 lastActivityAt，不使整个项目目录失效；状态/身份仍立即投影。
      owner.stop = watch(() => {
        const value = tab as unknown as Record<string, unknown>
        return ['projectPath', 'projectId', 'profileId', 'profileRevision', 'cli', 'title', 'name', 'status',
          'requestId', 'runId', 'generation', 'ptyId', 'ptyGeneration', 'sessionId', 'observedSessionId',
          'activity', 'activityState', 'working', 'pending', 'attentionState', 'observation', 'observationState',
          'observationNotice', 'observerEnabled', 'observerReasonCode', 'errorCode', 'sourceSessionKey', 'createdAt', 'usesDefaultClaudeRoot', 'titleChangeRevision']
          .map(key => value[key])
      }, () => {
        const previousPath = owner.projectPath
        owner.projectPath = tab.projectPath
        queueCatalogProjects(previousPath, owner.projectPath)
        const cue = nativeTab && historyCue(nativeTab)
        if (cue && (cue.identity && cue.identity !== owner.historyCue?.identity || cue.reply && cue.reply !== owner.historyCue?.reply
          || cue.prompt && cue.prompt !== owner.historyCue?.prompt || cue.titleRevision !== owner.historyCue?.titleRevision)) queueHistory()
        // 每轮重读绑定冻结的运行、来源与最新交互；已有旧 AI 标题不能阻止本轮更新。
        if (nativeTab && cue?.reply && cue.reply !== owner.historyCue?.reply) {
          if (owner.titleReadTimer) clearTimeout(owner.titleReadTimer)
          const attempt = captureNativeAttempt(nativeTab)
          const contextKey = () => JSON.stringify([nativeTab.cli, nativeTab.profileId, nativeTab.profileRevision,
            nativeTab.projectId, normalizePath(nativeTab.projectPath), nativeTab.sourceSessionKey])
          const context = contextKey()
          const explicitTitle = () => {
            const matches = history.get(nativeTab)?.sessions.filter(session => nativeTab.sourceSessionKey
              ? session.sessionKey === nativeTab.sourceSessionKey
              : !!nativeTab.observedSessionId && session.nativeSessionId === nativeTab.observedSessionId) ?? []
            const session = matches.length === 1 ? matches[0] : undefined
            if (!session) return undefined
            const source = session.lastKnownTitleSource ?? session.titleSource
            return source === 'ai' || source === 'custom' ? JSON.stringify([source, session.lastKnownTitle ?? session.title]) : undefined
          }
          const previousTitle = explicitTitle()
          const retryTitle = (index: number) => {
            owner.titleReadTimer = setTimeout(() => {
              owner.titleReadTimer = undefined
              const currentCue = historyCue(nativeTab)
              if (disposed || !matchesNativeAttempt(nativeTab, attempt) || native.tabs.get(nativeTab.tabId) !== nativeTab
                || contextKey() !== context || currentCue?.reply !== cue.reply
                || currentCue?.identity !== cue.identity || currentCue?.prompt !== cue.prompt) return
              const title = explicitTitle()
              if (title !== undefined && title !== previousTitle) return
              queueHistory()
              if (index + 1 < lateTitleReadDelays.length) retryTitle(index + 1)
            }, lateTitleReadDelays[index])
          }
          retryTitle(0)
        }
        owner.historyCue = cue
      })
      catalogOwnerWatches.set(tab, owner)
      queueCatalogProjects(owner.projectPath)
      if (owner.historyCue) queueHistory()
    }
  }, { immediate: true })
  watch(() => attention.queue, (next, previous) => {
    const ptyIds = new Set([...next, ...previous].map(item => item.ptyId))
    queueCatalogProjects(...[...legacy.tabs.values()].filter(tab => tab.ptyId && ptyIds.has(tab.ptyId)).map(tab => tab.projectPath))
  }, { deep: true })
  // 历史/归档的每次响应式写入（entry 替换、sessions、loading 翻转）都按项目范围
  // 合批刷新：全量 refresh 会霸占 refresh 版本并作废并发范围刷新，单条标题变化
  // 曾引发 4 次全量 listSessions；queueCatalogProjects 在同一 nextTick 去重合批。
  // 恢复提交成功但回执失败时，失败协调的重读会删除空归档键并抛错，成功路径的
  // refresh 不再执行；排队范围必须取上一批与当前批的并集，删除键/条目的项目
  // 不能从刷新范围里消失。
  // 只有会影响目录投影的历史变化才需要刷新该项目：已发布行 + 来源根构成投影签名。
  // loading/requestEpoch/error 等读取进度不改变行，逐字段把全部历史项目入队会让
  // 单项目读取放大成全项目反复重扫（实测 8 项目一次重读 32 次刷新、24 次发布）。
  // 撤销/删除的 entry 也必须入队一次：行消失同样改变该项目投影。
  const historySignatures = new Map<string, { path: string; signature: string }>()
  type NativeIdProjects = { count: number; paths: Set<string> }
  let nativeIdProjects = new Map<string, NativeIdProjects>()
  const archiveSignatures = new Map<string, string>()
  watch(() => [history.all(), projects.archivedSessions] as const, () => {
    const changed = new Set<string>()
    const seenEntries = new Set<string>()
    const nextIdProjects = new Map<string, NativeIdProjects>()
    for (const entry of history.all()) {
      seenEntries.add(entry.key)
      const path = entry.context.projectPath
      for (const row of entry.sessions) {
        const identity = nextIdProjects.get(row.nativeSessionId) ?? { count: 0, paths: new Set<string>() }
        identity.count++
        identity.paths.add(normalizePath(path))
        nextIdProjects.set(row.nativeSessionId, identity)
      }
      // 标题来自 CLI，可能包含任意分隔符；结构编码避免不同字段组合撞签名。
      const signature = JSON.stringify([entry.sourceRootKey ?? '', entry.sessions.map(row => [
        row.sessionKey, row.nativeSessionId, row.archived ? 1 : 0, row.title,
        row.titleSource ?? '', row.lastKnownTitle ?? '', row.lastKnownTitleSource ?? '',
        row.updatedAt ?? '', row.lastKnownActivityAt ?? '', row.truncated ? 1 : 0,
        row.metadataIncomplete ? 1 : 0, row.readFailure ?? '', row.createdAt ?? '',
      ])])
      const previous = historySignatures.get(entry.key)
      if (previous && previous.signature === signature) continue
      changed.add(normalizePath(path))
      if (previous && previous.path !== path) changed.add(normalizePath(previous.path))
      historySignatures.set(entry.key, { path, signature })
    }
    for (const [key, previous] of historySignatures) {
      if (seenEntries.has(key)) continue
      historySignatures.delete(key)
      changed.add(normalizePath(previous.path))
    }
    // 裸 ID 归档的歧义依赖所有项目；仅唯一性改变才使相关归档项目失效。
    // 标题/读取进度不改变唯一性，不能借此扩散为全目录刷新。
    for (const id of new Set([...nativeIdProjects.keys(), ...nextIdProjects.keys()])) {
      const previous = nativeIdProjects.get(id), next = nextIdProjects.get(id)
      if ((previous?.count === 1) === (next?.count === 1)) continue
      for (const path of new Set([...(previous?.paths ?? []), ...(next?.paths ?? [])])) {
        if (legacy.getArchivedSessions(path).includes(id)) changed.add(path)
      }
    }
    nativeIdProjects = nextIdProjects
    // 使用与 adapter 相同的归一化、合并查询，兼容重启后保留的路径别名。
    const archivedProjects = new Set([...projects.archivedSessions.keys()].map(normalizePath))
    for (const key of archivedProjects) {
      const signature = JSON.stringify([...new Set(legacy.getArchivedSessions(key))].sort())
      if (archiveSignatures.get(key) === signature) continue
      archiveSignatures.set(key, signature)
      changed.add(key)
    }
    for (const key of archiveSignatures.keys()) {
      if (archivedProjects.has(key)) continue
      archiveSignatures.delete(key)
      changed.add(key)
    }
    if (changed.size) queueCatalogProjects(...changed)
  }, { deep: true, immediate: true })
  watch(() => catalog.activeSessionId, async () => {
    await nextTick()
    if (!disposed && shell.section === 'workspace') host.value?.focus()
  })
  onMounted(() => {
    if (!enabled) return
    void useHookStore().init()
    void refresh()
  })
  const stopIndexMaintenance = indexMaintenance.configure(async control => {
    if (!ready.value || loading.value || disposed) throw new Error('INDEX_REBUILD_NOT_READY')
    const sources = profiles.profiles.filter(profile => profile.cli !== 'shell' && cliSupport.isEnabled(profile.cli as UnifiedCliKind))
      .map(profile => ({ cli: profile.cli as UnifiedCliKind, profileId: profile.id, profileRevision: profile.revision,
        projects: workspace.projects.map(project => ({ projectId: project.projectId, projectPath: project.selectedPath })) }))
    const identity = JSON.stringify(sources)
    const paths = cliSupport.isEnabled('claude') ? [...new Set([...legacyPaths.value, ...projects.pinnedProjects, ...[...legacy.tabs.values()].map(tab => tab.projectPath)])] : []
    const scratch = createNativeHistoryReader()
    let completed = 0
    control.progress(0, paths.length + sources.length)
    try {
      for (const path of paths) {
        if (control.cancelled()) return
        for (let offset = 0; ; offset += 200) {
          const rows = await getSessions(path, 200, offset)
          if (control.cancelled()) return
          if (rows.length < 200) break
          if (offset >= 1_000_000) throw new Error('INDEX_SCAN_LIMIT')
        }
        control.progress(++completed, paths.length + sources.length)
      }
      for (const source of sources) {
        if (control.cancelled()) return
        const entries = await scratch.loadGrouped(source, { force: true })
        if (control.cancelled()) return
        if ([...entries.values()].some(entry => entry.error || entry.readFailures?.length)) throw new Error('INDEX_SCAN_INCOMPLETE')
        control.progress(++completed, paths.length + sources.length)
      }
      const current = profiles.profiles.filter(profile => profile.cli !== 'shell' && cliSupport.isEnabled(profile.cli as UnifiedCliKind))
        .map(profile => ({ cli: profile.cli as UnifiedCliKind, profileId: profile.id, profileRevision: profile.revision,
          projects: workspace.projects.map(project => ({ projectId: project.projectId, projectPath: project.selectedPath })) }))
      if (disposed || JSON.stringify(current) !== identity) throw new Error('INDEX_SOURCE_CHANGED')
    } finally { scratch.invalidate() }
  }, () => refresh())
  onUnmounted(() => {
    stopIndexMaintenance()
    disposed = true; ++refreshOwner
    if (historyTimer) clearTimeout(historyTimer)
    pendingHistoryOwners.clear()
    for (const owner of catalogOwnerWatches.values()) {
      owner.stop()
      if (owner.titleReadTimer) clearTimeout(owner.titleReadTimer)
    }
    catalogOwnerWatches.clear(); pendingCatalogProjects.clear()
  })
  return { historyIssues, historyIssuesTruncated, historyRecoveryError, retryHistoryIssue, openHistoryIssueProject, diagnostics, closeDiagnostics, openSessions, cliAvailability, cliProblems, fatal, ready, loading, error, sourceWarnings, sourceWarningsTruncated, sourceWarningConfigurations, sourceNoticeDismissed, historyNoticeDismissed, sourceIssuesAvailable, sourceDiagnosticsWarnings, sourceDiagnosticsTruncated, sourceChecksPending, dismissSourceNotice, historyMetadataPartial, historyReadWarnings, refresh, retryAction }
}
