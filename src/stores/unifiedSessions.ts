import { computed, nextTick, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useCliSupportStore } from '@/stores/cliSupport'
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
import { makeSessionCatalogKey, makeSessionRenameOwnerKey, nativeHistoryRecordKey } from '@/utils/sessionPresentation'
import { matchesSessionMetadata } from '@/session/sessionMetadata'
import type { SessionUiRecord } from '@/types/app'
import { nativeHistoryContextKey } from '@/stores/nativeHistory'
import { normalizePath } from '@/utils/path'
import { compareProjectGroups } from '@/utils/projectGroupOrder'
import { compareSessionOpenOrder } from '@/utils/sessionOpenOrder'
import { LaunchConfigurationRequiredError } from '@/utils/launchPreparation'

function projectName(path: string): string {
  const normalized = path.replace(/[\\/]+$/, '')
  const parts = normalized.split(/[\\/]/).filter(Boolean)
  return parts.length ? parts[parts.length - 1] : path
}

/**
 * 折叠 legacy 行前，把它独占的、属于同一会话的证据交给接替的 native 行：
 * 归档状态、显式 Desk 覆盖名与最近打开时间只挂在 legacy 键上（归档存裸 session
 * ID，显示元数据键是 legacy 历史键）。**已打开的终端必须保持自己的非归档
 * 投影**：它由运行所有权决定，继承归档会让普通树隐藏该终端，而归档抽屉对活动
 * ID 调恢复会因不是历史 ID 直接失败。
 *
 * native 覆盖名由 adapter 按精确历史/终端身份解析后随行携带；目录层不再按 UUID
 * 扫描保存记录。没有 native 覆盖名时才读取 legacy 历史键，最后回落 CLI 标题。
 */
function adoptLegacyRow(
  native: UnifiedSession,
  legacy: UnifiedSession,
  records: ReadonlyMap<string, SessionUiRecord>,
): UnifiedSession {
  const legacySaved = records.get(legacy.id)
  const saved = matchesSessionMetadata(legacySaved, legacy) ? legacySaved : undefined
  return {
    ...native,
    archived: native.opened === true ? native.archived : native.archived || legacy.archived,
    title: native.savedDisplayName || saved?.title || native.title,
    lastActivityAt: Math.max(native.lastActivityAt, legacy.lastActivityAt),
    lastOpenedAt: Math.max(native.lastOpenedAt ?? 0, legacy.lastOpenedAt ?? 0),
  }
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

// CLI UUID 只在原配置来源内有意义；缺少两端同一根的正向证明时，保留各自恢复和归档 owner。
export const useUnifiedSessionsStore = defineStore('unified-sessions', () => {
  const projects = useProjectsStateStore()
  const cliSupport = useCliSupportStore()
  const sessions = ref<UnifiedSession[]>([])
  const activeSessionId = ref<string | null>(null)
  type RenameOwner = { key: string; owns: () => boolean; state: 'editing' | 'saving'; issued?: boolean; controller: AbortController }
  const renameOwners = new Map<string, RenameOwner>()
  const pendingRenames = new Map<string, number>()
  function captureRenameOwner(row: UnifiedSession, state: RenameOwner['state']): RenameOwner {
    return { controller: new AbortController(), key: makeSessionRenameOwnerKey(row), owns: adapters.find(adapter => adapter.runtime === row.runtime)?.captureOwnership?.(row.id) ?? (() => true), state }
  }
  function ownsRename(owner: RenameOwner, row: UnifiedSession | undefined) {
    return !!row && owner.key === makeSessionRenameOwnerKey(row) && owner.owns()
  }
  function revokeUnissuedRenames(selectedId: string | null) {
    const revoked = new Set<string>()
    for (const [id, owner] of renameOwners) {
      if (id !== selectedId && !owner.issued) { renameOwners.delete(id); owner.controller.abort(); revoked.add(id) }
    }
    if (revoked.size) sessions.value = sessions.value.map(row => revoked.has(row.id) ? { ...row, renameState: 'idle' } : row)
  }
  watch(activeSessionId, revokeUnissuedRenames, { flush: 'sync' })
  const initialized = ref(false)
  const loading = ref(false)
  const error = ref<string | null>(null)
  const completedClose = ref<{ projectKey: string; sequence: number } | null>(null)

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
  // 行投影缓存：让 projectGroups 在无关变化时保留行对象身份，避免整棵树重渲染。
  const projectedRows = new WeakMap<UnifiedSession, { resumePending: boolean; row: UnifiedSession }>()
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
  type ResumeCaller = { canAdmit: () => boolean; onSelectionClaim: () => void }
  type PendingCatalogResume = { task: Promise<UnifiedSession>; callers: ResumeCaller[]; selectionClaimed: boolean }
  const pendingCatalogResumes = new Map<string, PendingCatalogResume>()
  const pendingResumeKeys = ref(new Set<string>())
  const pendingArchiveRestores = new Map<string, Promise<void>>()
  const pendingArchiveKeys = ref(new Set<string>())
  function isResumePending(row: UnifiedSession) {
    const key = makeSessionRenameOwnerKey(row)
    return pendingResumeKeys.value.has(key) || pendingArchiveKeys.value.has(key)
  }
  function resumeCatalogSession(target: string | UnifiedSession, canAdmit = () => true, onSelectionClaim = () => {}) {
    const row = typeof target === 'string' ? requireSession(target) : target
    const key = makeSessionRenameOwnerKey(row)
    const caller = { canAdmit, onSelectionClaim }
    const existing = pendingCatalogResumes.get(key)
    if (existing) {
      existing.callers.push(caller)
      if (existing.selectionClaimed && canAdmit()) onSelectionClaim()
      return existing.task
    }
    pendingResumeKeys.value = new Set([...pendingResumeKeys.value, key])
    const callers = [caller]
    let selectionClaimed = false
    const task = performCatalogResume(row, () => callers.some(value => value.canAdmit()), () => {
      selectionClaimed = true
      const pending = pendingCatalogResumes.get(key)
      if (pending) pending.selectionClaimed = true
      for (const value of callers) if (value.canAdmit()) value.onSelectionClaim()
    })
    pendingCatalogResumes.set(key, { task, callers, selectionClaimed })
    void task.finally(() => {
      if (pendingCatalogResumes.get(key)?.task !== task) return
      pendingCatalogResumes.delete(key)
      const next = new Set(pendingResumeKeys.value)
      next.delete(key)
      pendingResumeKeys.value = next
    }).catch(() => undefined)
    return task
  }

  async function performCatalogResume(target: string | UnifiedSession, canAdmit = () => true, onSelectionClaim = () => {}) {
    const row = typeof target === 'string' ? requireSession(target) : target
    const id = row.id
    if (id.startsWith('native-tab:')) {
      const activation = activateSession(id)
      onSelectionClaim()
      await activation
      return row
    }
    if (id.startsWith('legacy-tab:')) {
      const owns = captureSessionOwnership(id)
      const ownsSelection = captureSelectionOwnership()
      if (!canAdmit()) throw new Error('RESTORE_CANCELLED')
      // A chooser may retain an ended snapshot after the owning tab has started.
      // Only fresh adapter state can authorize an explicit ended-session resume.
      const current = (await adapterForRuntime('legacy-claude').listSessions(row.projectKey)).find(value => value.id === id)
      if (!canAdmit() || !owns() || !ownsSelection() || !current || current.nativeSessionId !== row.nativeSessionId
        || normalizePath(current.projectPath) !== normalizePath(row.projectPath)) throw new Error('STALE_SESSION_ATTEMPT')
      const activation = activateSession(id)
      onSelectionClaim()
      await activation
      if (current.processState === 'stopped' && current.resumable) {
        const selected = captureSelectionOwnership()
        return restartSession(id, () => canAdmit() && owns() && selected() && activeSessionId.value === id)
      }
      return current
    }
    if (row.archived) throw new Error('SESSION_ARCHIVED')
    try {
      const admission = resumeSession(resumeInput(row), canAdmit)
      onSelectionClaim()
      const resumed = await admission
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
    sessions.value = [...sessions.value.filter(row => row.id !== opened.id && !supersedesSourceRow(row, opened)), opened]
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
  const closingSessions = new Map<string, { owns: () => boolean; promise: Promise<void>; fallback: { enabled: boolean } }>()

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
          workingCount: 0,
          errorCount: 0,
          permissionCount: 0,
          completedCount: 0,
          unreadReplyCount: 0,
          lastActivityAt: 0,
        }
        groups.set(key, group)
      }
      // 行投影复用同一对象：refresh 已保留未变化行的身份，这里若每次展开新对象，
      // 所有 SessionItem 的 prop 都会换身份，任何单行状态变化都会重渲染整棵树。
      const resumePending = isResumePending(session)
      let projected = projectedRows.get(session)
      if (!projected || projected.resumePending !== resumePending) {
        projected = { resumePending, row: { ...session, resumePending } }
        projectedRows.set(session, projected)
      }
      group.sessions.push(projected.row)
      if (session.processState === 'running' || session.processState === 'starting') {
        group.runningCount += 1
      }
      if (session.attentionState === 'needs-user') group.needsUserCount += 1
      if (session.observationNotice?.unreadReplyEnd?.kind === 'reply-ended') group.unreadReplyCount! += 1
      if (session.processState === 'running') {
        if (['working', 'thinking', 'tool_executing', 'subagent_running', 'compacting'].includes(session.activityState ?? 'unknown')) group.workingCount! += 1
        if (session.attentionKind === 'error' || session.activityState === 'error') group.errorCount! += 1
        else if (session.attentionKind === 'permission' || session.activityState === 'waiting_permission') group.permissionCount! += 1
        else if (session.attentionKind === 'completed') group.completedCount! += 1
      }
      group.lastActivityAt = Math.max(group.lastActivityAt, session.lastActivityAt)
    }

    for (const group of groups.values()) {
      group.sessions.sort(compareSessionOpenOrder)
    }

    return [...groups.values()].sort(compareProjectGroups)
  })

  /** legacy Claude 历史固定读 `~/.claude`；只有它的 native 根身份被证明后，同 ID 的 native 行才可折叠它。 */
  let legacyClaudeRoot: string | undefined
  let validateLegacyClaudeRoot: (() => Promise<string | undefined>) | undefined

  function configureAdapters(next: SessionAdapter[], roots?: { legacyClaudeRootKey?: string; validateLegacyClaudeRoot?: () => Promise<string | undefined> }): void {
    const seen = new Set<SessionRuntimeKind>()
    for (const adapter of next) {
      if (seen.has(adapter.runtime)) throw new Error('DUPLICATE_SESSION_ADAPTER')
      seen.add(adapter.runtime)
    }
    adapters = [...next]
    legacyClaudeRoot = roots?.legacyClaudeRootKey
    validateLegacyClaudeRoot = roots?.validateLegacyClaudeRoot
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

  async function refresh(projectKey?: string, options: { background?: boolean } = {}): Promise<void> {
    const version = ++refreshVersion
    const key = projectKey === undefined ? undefined : normalizePath(projectKey)
    if (key === undefined) fullRefreshVersion = version
    else projectRefreshVersions.set(key, version)
    const visible = !options.background
    if (visible) ++pendingRefreshes
    loading.value = pendingRefreshes > 0
    error.value = null

    const ownsProject = (candidate: string): boolean =>
      fullRefreshVersion <= version
      && (projectRefreshVersions.get(candidate) ?? 0) <= version

    try {
      const lists = await Promise.all(adapters.map(adapter => adapter.listSessions(projectKey)))
      const provenLegacyRoot = validateLegacyClaudeRoot ? await validateLegacyClaudeRoot() : legacyClaudeRoot
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
        // 支持开关关闭的 CLI 不列出其历史行；已打开终端是活进程，保留可选中与关闭。
        if (!cliSupport.isEnabled(session.cli) && session.opened !== true) continue
        byId.set(session.id, mergeDuplicateSession(byId.get(session.id), session))
      }
      const physicalRecords = new Map<string, UnifiedSession[]>()
      const previousIds = new Set(sessions.value.map(row => row.id))
      for (const row of byId.values()) {
        const record = nativeHistoryRecordKey(row)
        if (!record || row.safeErrorCode === 'SESSION_ORIGIN_AMBIGUOUS') continue
        const key = JSON.stringify([record, row.archived])
        const rows = physicalRecords.get(key)
        if (rows) rows.push(row)
        else physicalRecords.set(key, [row])
      }
      for (const rows of physicalRecords.values()) {
        if (rows.length < 2) continue
        const opened = rows.filter(row => row.opened === true)
        // 两个终端仍各有进程 owner；这里只移除同物理记录的关闭投影。
        const retained = opened.length ? opened : [rows.find(row => row.id === activeSessionId.value)
          ?? rows.find(row => previousIds.has(row.id)) ?? rows[0]]
        for (const row of rows) if (!retained.includes(row)) byId.delete(row.id)
      }
      // legacy 与 native 历史扫描同一 CLI 默认数据源,同一会话会各投影一行；以
      // native 行为准（归档/改名/恢复语义更完整），legacy 历史行在存在同一
      // native 身份时让位；已归档 legacy 行只交给关闭的 native 历史行。
      // 已打开终端保留自身进程 owner，不接管独立 legacy 归档 owner。
      // 折叠的前提是**根身份已证明**：legacy 读固定 `~/.claude`，native 读
      // profile 的 CLAUDE_CONFIG_DIR；只有两者的根 key 相同（同一目录的同一
      // 文件索引）才是同一来源，UUID 相等本身不是证据。根身份未知或不同一律
      // 不折叠，保留两端各自投影（v0.20.1 的 fail-closed 立场）。
      const sameLegacyRoot = (row: UnifiedSession) =>
        provenLegacyRoot !== undefined && row.nativeRootKey === provenLegacyRoot
      const nativeHistoryKeys = new Map<string, UnifiedSession[]>()
      for (const row of byId.values()) {
        if (row.runtime === 'native-cli' && row.nativeSessionId && sameLegacyRoot(row)) {
          const key = `${row.cli}\u0000${normalizePath(row.projectPath)}\u0000${row.nativeSessionId}`
          nativeHistoryKeys.set(key, [...(nativeHistoryKeys.get(key) ?? []), row])
        }
      }
      if (nativeHistoryKeys.size) {
        for (const [id, row] of byId) {
          if (row.runtime !== 'legacy-claude' || row.opened === true) continue
          const matches = nativeHistoryKeys.get(`${row.cli}\u0000${normalizePath(row.projectPath)}\u0000${row.adapterSessionId}`)
          if (matches?.length !== 1 || matches[0].safeErrorCode === 'SESSION_ORIGIN_AMBIGUOUS') continue
          const native = matches[0]
          if (row.archived && native.opened === true) continue
          // 折叠前先接管 legacy 行独占的证据：归档保存的是裸 session ID，而
          // nativeCliAdapter.archiveState 只识别完整 Native key 与旧 Native key，
          // 直接删除会让已归档会话重新出现在普通列表；已保存的 Desk 显示名与
          // 最近打开时间同样只挂在 legacy 记录上。
          byId.set(native.id, adoptLegacyRow(native, row, projects.sessionRecords))
          byId.delete(id)
        }
      }
      // 反向同理：同一会话已在 legacy 运行时打开时，其 native 历史行是同一
      // 内容的第二行（标题与时间来源各不相同）。隐藏关闭的 native 行，保留
      // 可选中、可关闭的打开行；关闭该 tab 后的下一次刷新会让 native 行
      // （AI 标题与记录时间）回到列表。但打开的 legacy 终端只有在**它自己这次
      // 启动**确实跑在默认根上时才是「同源孪生」：该证明随每次 PTY 启动冻结
      // （`usesDefaultClaudeRoot`），运行期改配置不会让旧终端变成另一种根。
      const openLegacyKeys = new Set<string>()
      for (const row of byId.values()) {
        if (row.runtime === 'legacy-claude' && row.opened === true && row.nativeSessionId
          && row.usesDefaultClaudeRoot === true) {
          openLegacyKeys.add(`${row.cli}\u0000${normalizePath(row.projectPath)}\u0000${row.nativeSessionId}`)
        }
      }
      if (openLegacyKeys.size) {
        for (const [id, row] of byId) {
          if (row.runtime === 'native-cli' && row.opened !== true && sameLegacyRoot(row)
            && openLegacyKeys.has(`${row.cli}\u0000${normalizePath(row.projectPath)}\u0000${row.nativeSessionId}`)) {
            byId.delete(id)
          }
        }
      }
      for (const [id, row] of missingRecords) {
        // 缺失记录与普通行同一支持过滤，不能经该路径让停用 CLI 绕过隐藏。
        if (!cliSupport.isEnabled(row.cli) && row.opened !== true) continue
        byId.set(id, row)
      }
      // Adapter reads project runtime state; an unsaved draft belongs to the
      // current UI owner until cancellation or exact source/attempt invalidation.
      for (const [id, owner] of renameOwners) {
        if (!ownsRename(owner, byId.get(id))) {
          renameOwners.delete(id)
          owner.controller.abort()
          const row = byId.get(id)
          if (row) byId.set(id, { ...row, renameState: 'idle' })
        }
      }
      const previousRows = new Map(sessions.value.map(row => [row.id, row]))
      const nextRows = [...byId.values()].map(row => {
        const owner = renameOwners.get(row.id)
        const next = owner ? { ...row, renameState: owner.state } : row
        const previous = previousRows.get(row.id)
        // 未变化的安全投影保留对象身份，避免同一项目内无关行重绘。
        return previous && Object.keys(previous).length === Object.keys(next).length
          && Object.keys(next).every(key => JSON.stringify(previous[key as keyof UnifiedSession]) === JSON.stringify(next[key as keyof UnifiedSession])) ? previous : next
      }).sort(compareSessionOpenOrder)
      // 行序列完全一致时不发布新数组：下游以数组身份为界，无变化发布会让
      // 整条 watch/投影链空转（实测同数据单项目读取 24 次数组发布）。
      if (nextRows.length !== sessions.value.length || nextRows.some((row, index) => row !== sessions.value[index])) {
        sessions.value = nextRows
      }
      if (activeSessionId.value && !byId.has(activeSessionId.value)) {
        activeSessionId.value = null
      }
    } catch (failure) {
      if (refreshVersion === version) {
        error.value = safeUserErrorCode(failure)
      }
      throw failure
    } finally {
      if (visible) --pendingRefreshes
      loading.value = pendingRefreshes > 0
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
    // 支持开关是新建的权威门禁：对话框只隐藏入口，这里拒绝任何路径的停用 CLI 创建。
    if (!cliSupport.isEnabled(input.cli)) throw new Error('NEW_SESSION_PREPARATION_FAILED')
    ++selectionEpoch
    const intentEpoch = ++selectionIntentEpoch
    const id = retryId ?? createNativeId('preparing')
    const owner = {}
    const row: UnifiedSession = { id, projectKey: normalizePath(input.projectPath), projectPath: input.projectPath,
      cli: input.cli, runtime: 'native-cli', title: input.title || (input.cli === 'claude' ? 'Claude Code' : input.cli === 'codex' ? 'Codex CLI' : 'opencode'),
      processState: 'starting', attentionState: 'none', lastActivityAt: Date.now(), lastOpenedAt: Date.now(), archived: false, opened: false, preparationState: 'pending', resumable: false, adapterSessionId: id }
    const creation = { input: copyInput(input), row, owner, preparing: true, selectionIntentEpoch: intentEpoch }
    creations.set(id, creation)
    sessions.value = [...sessions.value.filter(session => session.id !== id), row]
    activeSessionId.value = id
    const current = () => creations.get(id)?.owner === owner
    const canAdmit = () => current() && (!confirmationGuard || confirmationGuard()
      && intentEpoch === selectionIntentEpoch && activeSessionId.value === id)
    let admitted = false
    const publishAdmitted = (created: UnifiedSession) => {
      if (admitted) return
      admitted = true
      creations.delete(id)
      sessions.value = [...sessions.value.filter(session => session.id !== id && session.id !== created.id), created]
      if (creation.selectionIntentEpoch === selectionIntentEpoch && activeSessionId.value === id) activeSessionId.value = created.id
    }
    try {
      const prepared = await prepareCreation(copyInput(creation.input))
      if (!canAdmit()) throw new Error('NEW_SESSION_CANCELLED')
      if (confirmationGuard && (prepared.launchConfigId !== input.launchConfigId || prepared.launchConfigRevision !== input.launchConfigRevision)) throw new Error('PROFILE_SELECTION_CHANGED')
      creation.preparing = false
      const created = await adapterForRuntime('native-cli').createSession(prepared, canAdmit, publishAdmitted)
      // 提前交接后用户可能已经关闭运行行；迟到的显示元数据回执不能再次插入它。
      publishAdmitted(created)
      // Admission is not CLI success. A failed catalog read cannot turn an
      // admitted attempt into a retryable preparation row.
      await refresh(creation.input.projectKey).catch(() => { error.value = 'SESSION_REFRESH_FAILED' })
      return sessions.value.find(value => value.id === created.id) ?? created
    } catch (failure) {
      if (admitted) throw new Error('LAUNCH_STATE_UNKNOWN')
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

  /**
   * 打开行接管它取代的关闭历史投影：打开行 id 是 tab 身份，历史行 id 不同，
   * 只按 resumed.id 过滤会让两行在列表里并存一个刷新往返。折叠口径与 refresh
   * 一致：同 runtime 的历史行只在物理记录键相同时让位；跨 runtime 的 legacy
   * 孪生仅在根身份已证明时让位；绝不触碰其他打开的终端。
   */
  function supersedesSourceRow(row: UnifiedSession, opened: UnifiedSession): boolean {
    if (row.opened === true || row.archived || row.id === opened.id) return false
    if (row.cli !== opened.cli || normalizePath(row.projectPath) !== normalizePath(opened.projectPath)) return false
    if (!opened.nativeSessionId || row.nativeSessionId !== opened.nativeSessionId) return false
    if (opened.runtime === 'legacy-claude') {
      return row.runtime === 'legacy-claude' && row.id.startsWith('legacy-history:')
    }
    const record = nativeHistoryRecordKey(opened)
    if (row.runtime === 'native-cli') return record !== null && nativeHistoryRecordKey(row) === record
    if (row.runtime === 'legacy-claude') {
      return legacyClaudeRoot !== undefined && opened.nativeRootKey === legacyClaudeRoot
        && row.id.startsWith('legacy-history:')
    }
    return false
  }

  async function resumeSession(input: ResumeUnifiedSessionInput, canAdmit = () => true): Promise<UnifiedSession> {
    revokeUnissuedRenames(null)
    const epoch = ++selectionEpoch
    ++selectionIntentEpoch
    const resumed = await adapterForResume(input).resumeSession(input, canAdmit)
    // 打开行必须在同一次发布里接管它的来源历史行：打开行 id 是 tab 身份，
    // 历史行 id 不同，只按 resumed.id 过滤会让两行并存一个刷新往返。
    sessions.value = [...sessions.value.filter(row => row.id !== resumed.id
      && !supersedesSourceRow(row, resumed)), resumed]
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

  async function clearUnread(id: string, expected: NonNullable<NonNullable<UnifiedSession['observationNotice']>['unreadReplyEnd']>): Promise<void> {
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    if (session.opened || !adapter.clearUnread) throw new Error('STALE_SESSION_ATTEMPT')
    const current = session.observationNotice?.unreadReplyEnd
    if (!current || !expected || current.kind !== expected.kind || current.eventId !== expected.eventId
      || current.runId !== expected.runId || current.generation !== expected.generation
      || current.receivedAt !== expected.receivedAt) throw new Error('STALE_SESSION_ATTEMPT')
    await adapter.clearUnread(id, session)
    await refresh(session.projectKey)
  }

  async function selectRemainingAfterClose(id: string, projectKey: string, wasSelected: boolean, ownsSelection: () => boolean) {
    if (!wasSelected || !ownsSelection() || activeSessionId.value !== null && activeSessionId.value !== id) return
    activeSessionId.value = null
    const open = sessions.value.filter(row => row.id !== id && !row.archived
      && row.id === `${row.runtime === 'native-cli' ? 'native-tab' : 'legacy-tab'}:${row.adapterSessionId}`)
    const next = open.find(row => normalizePath(row.projectPath) === normalizePath(projectKey)) ?? open[0]
    if (next) await activateSession(next.id)
  }
  function closeSession(id: string, canContinue = () => true, selectFallback = true): Promise<void> {
    if (!canContinue()) return Promise.reject(new Error('STALE_SESSION_ATTEMPT'))
    const closing = closingSessions.get(id)
    // Repeated clicks join only the exact still-owned attempt. Enqueuing another
    // close would invalidate the first operation's selection publication.
    if (closing?.owns()) {
      if (!selectFallback) closing.fallback.enabled = false
      return closing.promise
    }
    const fallback = { enabled: selectFallback }
    const wasSelected = activeSessionId.value === id
    const ownsSelection = captureSelectionOwnership()
    async function selectRemaining(projectKey: string) {
      // Ownership publication can clear the closed ID before this operation
      // finishes. Only its original selection intent may choose a replacement.
      // History is never resumed by a close; the existing runtime adapter owns
      // activation, including the Legacy aggregate's internal selected tab.
      if (fallback.enabled) await selectRemainingAfterClose(id, projectKey, wasSelected, ownsSelection)
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
    const owns = adapter.captureOwnership?.(id, 'close') ?? (() => sessions.value.some(row => row === session))
    const promise = enqueue(id, () => adapter.closeSession(id, canContinue), async () => {
      if (activeSessionId.value === id) activeSessionId.value = null
      await refresh(session.projectKey)
      // A close replaces an open identity with its history identity. The tree
      // takes one fresh activity snapshot only after that reconciliation, while
      // ordinary output/status publications keep their stable row positions.
      completedClose.value = { projectKey: session.projectKey, sequence: (completedClose.value?.sequence ?? 0) + 1 }
      await selectRemaining(session.projectKey)
    }, 'close')
    const pending = { owns, promise, fallback }
    closingSessions.set(id, pending)
    void promise.then(() => {
      if (closingSessions.get(id) === pending) closingSessions.delete(id)
    }, () => {
      if (closingSessions.get(id) === pending) closingSessions.delete(id)
    })
    return promise
  }

  function captureProjectClose(path: string, expectedOwnerIds?: readonly string[]): (canContinue?: () => boolean) => Promise<void> {
    const projectKey = normalizePath(path)
    const targets = sessions.value.filter(row => normalizePath(row.projectPath) === projectKey
      && !row.archived && (row.opened === true || isPreparingSession(row.id))).map(row => ({
      id: row.id, key: makeSessionRenameOwnerKey(row),
      owns: creations.has(row.id) ? captureSessionOwnership(row.id)
        : adapterForRuntime(row.runtime).captureOwnership?.(row.id, 'close') ?? (() => sessions.value.includes(row)),
    }))
    const expected = expectedOwnerIds && new Set(expectedOwnerIds)
    const inventoryMatches = !expected || expected.size === expectedOwnerIds?.length
      && targets.length === expected.size && targets.every(target => expected.has(target.id))
    return async (canContinue = () => true) => {
      if (!inventoryMatches) throw new Error('PROJECT_CLOSE_PARTIAL')
      if (!canContinue()) throw new Error('ACTION_CANCELLED')
      let failed = false
      const selected = targets.find(target => target.id === activeSessionId.value)
      const ownsSelection = captureSelectionOwnership()
      try {
        // 已开始的单个关闭也可能先于其批量次序完成，须在第一次 await 前接管后备选择。
        for (const target of targets) {
          const closing = closingSessions.get(target.id)
          if (target.owns() && closing?.owns()) closing.fallback.enabled = false
        }
        // Legacy 内部关闭也会选择替补；先隐藏前台宿主，避免该内部切换触发未读 ACK。
        if (selected) { activeSessionId.value = null; await nextTick() }
        for (const target of targets) {
          if (!canContinue()) throw new Error('ACTION_CANCELLED')
          const row = sessions.value.find(item => item.id === target.id)
          if (!row) { if (target.owns()) failed = true; continue }
          const owns = () => canContinue() && target.owns() && sessions.value.some(item => item.id === target.id
            && normalizePath(item.projectPath) === projectKey && makeSessionRenameOwnerKey(item) === target.key)
          if (!owns()) { failed = true; continue }
          try { await closeSession(target.id, owns, false) } catch { failed = true }
        }
        if (failed) throw new Error('PROJECT_CLOSE_PARTIAL')
      } finally {
        // 批次结束才选替补，避免短暂激活即将关闭的会话并清除其未读回执。
        if (selected && ownsSelection() && activeSessionId.value === null) {
          const remaining = sessions.value.find(row => row.id === selected.id)
          if (!remaining) await selectRemainingAfterClose(selected.id, projectKey, true, ownsSelection)
          else if (selected.owns() && makeSessionRenameOwnerKey(remaining) === selected.key) await activateSession(selected.id)
        }
      }
    }
  }

  function discardPreparation(id: string): Promise<void> {
    const creation = creations.get(id)
    if (!creation?.preparing || creation.row.processState !== 'failed') return Promise.reject(new Error('STALE_SESSION_ATTEMPT'))
    return closeSession(id)
  }

  function beginRename(id: string) {
    const row = sessions.value.find(value => value.id === id)
    if (!row || (row.opened !== false && activeSessionId.value !== id) || row.archived || row.renameState === 'saving' || (pendingRenames.get(id) ?? 0) > 0 || isPreparingSession(id)) return
    const current = renameOwners.get(id)
    if (!current || !ownsRename(current, row)) {
      current?.controller.abort()
      const owner = captureRenameOwner(row, 'editing')
      if (!ownsRename(owner, row)) { owner.controller.abort(); renameOwners.delete(id); return }
      renameOwners.set(id, owner)
    }
    sessions.value = sessions.value.map(row => row.id === id ? { ...row, renameState: 'editing' } : row)
  }
  function cancelRename(id: string) {
    if ((pendingRenames.get(id) ?? 0) > 0) return
    const owner = renameOwners.get(id)
    renameOwners.delete(id)
    owner?.controller.abort()
    sessions.value = sessions.value.map(row => row.id === id ? { ...row, renameState: 'idle' } : row)
  }
  function renameSession(id: string, title: string, requireEditor = false): Promise<'input-written' | 'display-name-saved' | 'native-title-saved' | void> {
    const session = requireSession(id)
    const adapter = adapterForRuntime(session.runtime)
    const current = renameOwners.get(id)
    if ((requireEditor && (!current || current.state !== 'editing' || (session.opened !== false && activeSessionId.value !== id))) || (current && !ownsRename(current, session))) {
      if (!current?.issued) {
        renameOwners.delete(id)
        current?.controller.abort()
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
      return adapter.renameSession(id, title, () => renameOwners.get(id) === owner && ownsRename(owner, sessions.value.find(row => row.id === id)), () => { owner.issued = true }, owner.controller.signal)
    }, async result => {
      try { await refresh(session.projectKey) }
      catch (failure) {
        // 原生已确认写回，后置读取失败只能提示同步问题，不能重新开放同一次保存。
        if (result === 'native-title-saved') throw new Error('RENAME_DISPLAY_SYNC_FAILED')
        throw failure
      }
    }).catch(failure => {
      // A failed owned editor save must keep the user's draft available for
      // retry; revoked/replaced attempts still close through normal cleanup.
      const nativeSaved = session.runtime === 'native-cli' && safeUserErrorCode(failure) === 'RENAME_DISPLAY_SYNC_FAILED'
      if (!nativeSaved && requireEditor && renameOwners.get(id) === owner && ownsRename(owner, sessions.value.find(row => row.id === id))) {
        owner.state = 'editing'
        owner.issued = false
        sessions.value = sessions.value.map(row => row.id === id ? { ...row, renameState: 'editing' } : row)
      }
      throw failure
    }).finally(() => {
      const remaining = (pendingRenames.get(id) ?? 1) - 1
      if (remaining) pendingRenames.set(id, remaining)
      else pendingRenames.delete(id)
      if (renameOwners.get(id) === owner && !remaining && owner.state === 'saving') cancelRename(id)
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
    if (!session.archived) return Promise.resolve()
    const key = makeSessionRenameOwnerKey(session)
    const existing = pendingArchiveRestores.get(key)
    if (existing) return existing
    const adapter = adapterForRuntime(session.runtime)
    pendingArchiveKeys.value = new Set([...pendingArchiveKeys.value, key])
    const task = enqueue(id, () => adapter.restoreArchivedSession(id), () => refresh(session.projectKey))
    pendingArchiveRestores.set(key, task)
    void task.finally(() => {
      if (pendingArchiveRestores.get(key) !== task) return
      pendingArchiveRestores.delete(key)
      const next = new Set(pendingArchiveKeys.value)
      next.delete(key)
      pendingArchiveKeys.value = next
    }).catch(() => undefined)
    return task
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
    resumeCatalogSession, isResumePending, removeMissingRecord, launchResume,
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
    captureProjectClose,
    clearUnread,
    completedClose,
    renameSession, beginRename, cancelRename, discardPreparation,
    archiveSession,
    restoreArchivedSession,
  }
})
