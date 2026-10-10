import { defineStore } from 'pinia'
import { reactive, ref } from 'vue'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'
import type { LaunchAction, NativeCliKind } from '@/types/cli'
import { parseU64 } from '@/utils/nativeIdentity'
import type { ObservationState } from '@/integrations/registry'
import { createNativeId } from '@/utils/nativeId'
import type { NativeObservationNotice, NativeObservationNoticeState } from '@/types/nativeObservationNotice'
import { useHookStore } from './hook'

export type NativeTabStatus =
  | 'stopped'
  | 'starting'
  | 'running'
  | 'unknown'
  | 'failed'
  | 'exited'

export interface NativeCliTab {
  /** Read-only history identity retained by an explicitly resumed catalog item. */
  sourceSessionKey?: string
  tabId: string
  cli: NativeCliKind
  projectId: string
  projectPath: string
  profileId: string
  profileRevision: string
  requestId: string
  runId: string
  generation: number
  action: LaunchAction
  status: NativeTabStatus
  errorCode: string | null
  launchRevision: string | null
  title: string
  createdAt: number
  lastActivityAt: number
  attentionState?: 'none' | 'needs-user'
  activityState?: ObservationState['activity']
  observationState?: ObservationState['observation']
  observationNotice?: NativeObservationNoticeState
}

export interface NativeAttemptIdentity {
  requestId: string
  runId: string
  generation: number
}

export interface NativeTabCreate {
  sourceSessionKey?: string
  cli: NativeCliKind
  projectId: string
  projectPath: string
  profileId: string
  profileRevision: string
  action: LaunchAction
  title?: string
}

export function captureNativeAttempt(
  tab: Pick<NativeCliTab, 'requestId' | 'runId' | 'generation'>,
): NativeAttemptIdentity {
  return {
    requestId: tab.requestId,
    runId: tab.runId,
    generation: tab.generation,
  }
}

export function matchesNativeAttempt(
  tab: Pick<NativeCliTab, 'requestId' | 'runId' | 'generation'> | undefined,
  attempt: NativeAttemptIdentity,
): boolean {
  return Boolean(
    tab
    && tab.requestId === attempt.requestId
    && tab.runId === attempt.runId
    && tab.generation === attempt.generation,
  )
}

function id(prefix: string): string {
  return createNativeId(prefix)
}

function text(value: string, code: string): string {
  if (!value || value.includes('\0')) throw new Error(code)
  return value
}

function revision(value: string): string {
  try {
    parseU64(value)
    return value
  } catch {
    throw new Error('INVALID_PROFILE_REVISION')
  }
}

function copyAction(action: LaunchAction): LaunchAction {
  switch (action.kind) {
    case 'new':
      return { kind: 'new' }
    case 'resume-picker':
      return { kind: 'resume-picker', scope: action.scope }
    case 'resume-id':
      return { kind: 'resume-id', nativeSessionId: action.nativeSessionId }
    case 'raw':
      return { kind: 'raw', argv: [...action.argv] }
  }
}

const noticeKinds = new Set<NativeObservationNotice['kind']>([
  'prompt-submitted', 'tool-started', 'tool-ended', 'tool-failed', 'reply-ended', 'reply-failed',
  'permission-requested', 'input-requested', 'subagent-started', 'subagent-ended', 'compaction-started', 'compaction-ended',
])
function emptyNotices(): NativeObservationNoticeState { return { recent: null, unreadReplyEnd: null } }
function copyNotice(notice: NativeObservationNotice): NativeObservationNotice {
  return { kind: notice.kind, eventId: notice.eventId, receivedAt: notice.receivedAt, runId: notice.runId, generation: notice.generation }
}
function copyNotices(state: NativeObservationNoticeState): NativeObservationNoticeState {
  return { recent: state.recent && copyNotice(state.recent), unreadReplyEnd: state.unreadReplyEnd && copyNotice(state.unreadReplyEnd) }
}
function validNotice(notice: NativeObservationNotice, attempt: NativeAttemptIdentity): boolean {
  return Boolean(notice && noticeKinds.has(notice.kind) && typeof notice.eventId === 'string'
    && notice.eventId.length <= 128 && /^[A-Za-z0-9._:-]+$/.test(notice.eventId)
    && Number.isSafeInteger(notice.receivedAt) && notice.receivedAt >= 0
    && notice.runId === attempt.runId && notice.generation === attempt.generation)
}

function snapshot(tab: NativeCliTab): NativeCliTab {
  return {
    ...tab,
    action: copyAction(tab.action),
    ...(tab.observationNotice ? { observationNotice: copyNotices(tab.observationNotice) } : {}),
  }
}

function statusFromLaunch(value: LaunchStatus): {
  status: NativeTabStatus
  errorCode: string | null
} {
  switch (value.phase) {
    case 'reserved':
    case 'starting':
      return { status: 'starting', errorCode: null }
    case 'running':
      return { status: 'running', errorCode: null }
    case 'indeterminate':
      return { status: 'unknown', errorCode: 'LAUNCH_STATE_UNKNOWN' }
    case 'failed':
      return { status: 'failed', errorCode: value.failure ?? 'LAUNCH_FAILED' }
    case 'cancelled':
      return { status: 'failed', errorCode: value.failure ?? 'LAUNCH_CANCELLED' }
    case 'exited':
      return { status: 'exited', errorCode: null }
  }
}

export const useNativeTabsStore = defineStore('native-cli-tabs', () => {
  const tabs = reactive(new Map<string, NativeCliTab>())
  const activeTabId = ref<string | null>(null)
  // Positive resource-scope proof only: false/absent receipts are not evidence
  // that launch was never submitted. Bind proof to the exact local attempt.
  const unstartedAttempts = reactive(new Map<string, NativeAttemptIdentity>())
  const frozenLaunchReceipts = new Map<string, string>()
  // At most one safe latest projection per starting tab, bound to the exact attempt.
  const pendingAttention = new Map<string, { attempt: NativeAttemptIdentity; state: ObservationState }>()
  const pendingNotices = new Map<string, { attempt: NativeAttemptIdentity; state: NativeObservationNoticeState }>()
  // Never evict IDs within an attempt: at the cap, fail closed rather than allow replay.
  const noticeLedgers = new Map<string, { attempt: NativeAttemptIdentity; seen: Map<string, NativeObservationNotice> }>()
  function clearNoticeProjection(tabId: string): void {
    pendingNotices.delete(tabId)
    const value = tabs.get(tabId)
    if (value) value.observationNotice = emptyNotices()
  }
  /** Receipt proof binds the private request identity omitted from the safe DTO. */
  function hasOwnedObservationNotice(tabId: string, attempt: NativeAttemptIdentity): boolean {
    const value = tabs.get(tabId), ledger = noticeLedgers.get(tabId)
    if (!value || value.status !== 'running' || value.cli !== 'claude' || value.action.kind === 'raw'
      || !matchesNativeAttempt(value, attempt) || !ledger || !matchesNativeAttempt(value, ledger.attempt)) return false
    const notices = [value.observationNotice?.recent, value.observationNotice?.unreadReplyEnd].filter(Boolean) as NativeObservationNotice[]
    return notices.length > 0 && notices.every(notice => {
      const accepted = ledger.seen.get(notice.eventId)
      return validNotice(notice, attempt) && !!accepted && accepted.kind === notice.kind && accepted.receivedAt === notice.receivedAt
    }) && (!value.observationNotice?.unreadReplyEnd || value.observationNotice.unreadReplyEnd.kind === 'reply-ended')
  }
  function applyObservationNotice(tabId: string, attempt: NativeAttemptIdentity, notice: NativeObservationNotice): boolean {
    const value = tabs.get(tabId)
    if (!value || !matchesNativeAttempt(value, attempt) || value.cli !== 'claude' || value.action.kind === 'raw'
      || !['starting', 'running'].includes(value.status) || !validNotice(notice, attempt)) return false
    let ledger = noticeLedgers.get(tabId)
    if (ledger && !matchesNativeAttempt(value, ledger.attempt)) { clearNoticeProjection(tabId); return false }
    if (!ledger) {
      ledger = { attempt: captureNativeAttempt(value), seen: new Map() }
      noticeLedgers.set(tabId, ledger)
    }
    if (ledger.seen.has(notice.eventId) || ledger.seen.size >= 1024) return false
    ledger.seen.set(notice.eventId, copyNotice(notice))
    const pending = pendingNotices.get(tabId)
    const previous = value.status === 'starting' ? pending && matchesNativeAttempt(value, pending.attempt) ? pending.state : emptyNotices()
      : value.observationNotice ?? emptyNotices()
    const state = { recent: copyNotice(notice), unreadReplyEnd: notice.kind === 'reply-ended' ? copyNotice(notice)
      : previous.unreadReplyEnd && copyNotice(previous.unreadReplyEnd) }
    if (value.status === 'starting') pendingNotices.set(tabId, { attempt: captureNativeAttempt(value), state })
    else { value.observationNotice = state; value.lastActivityAt = Date.now() }
    return true
  }
  function ackReplyEndNotice(tabId: string, attempt: NativeAttemptIdentity, eventId: string): boolean {
    const value = tabs.get(tabId)
    if (!value || !hasOwnedObservationNotice(tabId, attempt)) return false
    const state = value.observationNotice, unread = state?.unreadReplyEnd
    if (!unread || unread.kind !== 'reply-ended' || unread.eventId !== eventId || !validNotice(unread, attempt)) return false
    value.observationNotice = { recent: state.recent && copyNotice(state.recent), unreadReplyEnd: null }
    return true
  }
  const frozenIdentity = (tab: NativeCliTab) => JSON.stringify([tab.requestId, tab.runId, tab.generation,
    tab.cli, tab.profileId, tab.profileRevision, tab.projectId, tab.projectPath, tab.sourceSessionKey, tab.action])
  /** Positive receipt proof only; a locally assigned status is not admission evidence. */
  function hasFrozenLaunchReceipt(tabId: string): boolean {
    const tab = tabs.get(tabId)
    return !!tab && tab.status !== 'starting' && tab.launchRevision !== null
      && frozenLaunchReceipts.get(tabId) === frozenIdentity(tab)
  }
  function hasUnstartedAttempt(tabId: string): boolean {
    const proof = unstartedAttempts.get(tabId)
    return !!proof && matchesNativeAttempt(tabs.get(tabId), proof)
  }

  function create(input: NativeTabCreate): NativeCliTab {
    if (input.cli !== 'claude' && input.cli !== 'codex') {
      throw new Error('NATIVE_CLI_REQUIRED')
    }
    const tabId = id('tab')
    const value: NativeCliTab = {
      tabId,
      ...(input.sourceSessionKey ? { sourceSessionKey: input.sourceSessionKey } : {}),
      cli: input.cli,
      projectId: text(input.projectId, 'PROJECT_ID_REQUIRED'),
      projectPath: text(input.projectPath, 'PROJECT_PATH_REQUIRED'),
      profileId: text(input.profileId, 'PROFILE_ID_REQUIRED'),
      profileRevision: revision(input.profileRevision),
      requestId: id('request'),
      runId: id('run'),
      generation: 1,
      action: copyAction(input.action),
      status: 'stopped',
      errorCode: null,
      launchRevision: null,
      title: input.title?.trim() || (input.cli === 'claude' ? 'Claude Code' : 'Codex CLI'),
      createdAt: Date.now(),
      lastActivityAt: Date.now(),
      attentionState: 'none',
      activityState: 'unknown',
      observationState: 'off',
    }
    tabs.set(tabId, value)
    unstartedAttempts.set(tabId, captureNativeAttempt(value))
    activeTabId.value = tabId
    return snapshot(value)
  }

  function tab(tabId: string): NativeCliTab | undefined {
    return tabs.get(tabId)
  }

  function byProject(projectPath: string): NativeCliTab[] {
    return [...tabs.values()].filter(item => item.projectPath === projectPath)
  }

  function setActive(tabId: string | null): void {
    if (tabId !== null && !tabs.has(tabId)) throw new Error('TAB_NOT_FOUND')
    activeTabId.value = tabId
  }

  /** Leading-edge coalescing bounds output/input catalog publication to once per
   * second per exact attempt. No timer can make an idle session look active. */
  function touch(tabId: string, attempt: NativeAttemptIdentity, at = Date.now()): void {
    const value = tabs.get(tabId)
    if (!matchesNativeAttempt(value, attempt) || !value || !Number.isFinite(at)) return
    if (at - value.lastActivityAt >= 1000) value.lastActivityAt = at
  }

  function applyObservation(tabId: string, attempt: NativeAttemptIdentity, state: ObservationState): void {
    const value = tabs.get(tabId)
    if (!value || !matchesNativeAttempt(value, attempt)) return
    const projected: ObservationState = { observation: state.observation,
      activity: state.observation === 'active' ? state.activity : 'unknown' }
    if (value.status === 'starting') {
      pendingAttention.set(tabId, { attempt: captureNativeAttempt(value), state: projected })
      return
    }
    pendingAttention.delete(tabId)
    if (value.status !== 'running') return
    const attention = projected.activity === 'waiting' ? 'needs-user' : 'none'
    if ((value.attentionState ?? 'none') !== attention || value.activityState !== projected.activity
      || value.observationState !== projected.observation) {
      value.attentionState = attention
      value.activityState = projected.activity
      value.observationState = projected.observation
      value.lastActivityAt = Date.now()
    }
  }

  function rename(tabId: string, title: string): void {
    const value = tabs.get(tabId)
    if (!value) throw new Error('TAB_NOT_FOUND')
    const next = title.trim()
    if (!next || next.includes('\0')) throw new Error('SESSION_TITLE_REQUIRED')
    value.title = next
    value.lastActivityAt = Date.now()
  }

  function markStarting(tabId: string): void {
    const value = tabs.get(tabId)
    if (!value) throw new Error('TAB_NOT_FOUND')
    unstartedAttempts.delete(tabId)
    frozenLaunchReceipts.delete(tabId)
    pendingAttention.delete(tabId)
    clearNoticeProjection(tabId)
    if (value.status !== 'starting' || value.errorCode !== null) value.lastActivityAt = Date.now()
    value.status = 'starting'
    value.attentionState = 'none'
    value.activityState = 'unknown'
    value.observationState = 'off'
    value.errorCode = null
  }

  function markUnknown(tabId: string): void {
    const value = tabs.get(tabId)
    if (!value) throw new Error('TAB_NOT_FOUND')
    unstartedAttempts.delete(tabId)
    pendingAttention.delete(tabId)
    clearNoticeProjection(tabId)
    if (value.status !== 'unknown' || value.errorCode !== 'LAUNCH_STATE_UNKNOWN') value.lastActivityAt = Date.now()
    value.status = 'unknown'
    value.attentionState = 'none'
    value.activityState = 'unknown'
    value.observationState = 'off'
    value.errorCode = 'LAUNCH_STATE_UNKNOWN'
    useHookStore().invalidateObservation(value)
  }

  function markError(tabId: string, code: string): void {
    const value = tabs.get(tabId)
    if (!value) throw new Error('TAB_NOT_FOUND')
    unstartedAttempts.delete(tabId)
    pendingAttention.delete(tabId)
    clearNoticeProjection(tabId)
    const next = text(code, 'ERROR_CODE_REQUIRED')
    if (value.status !== 'failed' || value.errorCode !== next) value.lastActivityAt = Date.now()
    value.status = 'failed'
    value.attentionState = 'none'
    value.activityState = 'unknown'
    value.observationState = 'off'
    value.errorCode = next
    useHookStore().invalidateObservation(value)
  }

  function setDiagnostic(tabId: string, code: string): void {
    const value = tabs.get(tabId)
    if (!value) throw new Error('TAB_NOT_FOUND')
    const next = text(code, 'ERROR_CODE_REQUIRED')
    if (value.errorCode !== next) value.lastActivityAt = Date.now()
    value.errorCode = next
  }

  function applyLaunchStatus(tabId: string, launch: LaunchStatus): boolean {
    const value = tabs.get(tabId)
    if (!value || !matchesNativeAttempt(value, {
      requestId: launch.requestId,
      runId: launch.run.runId,
      generation: launch.run.generation,
    })) {
      return false
    }
    unstartedAttempts.delete(tabId)
    const next = statusFromLaunch(launch)
    const ledger = noticeLedgers.get(tabId)
    if (ledger && !matchesNativeAttempt(value, ledger.attempt)) clearNoticeProjection(tabId)
    const changed = value.launchRevision === null || value.status !== next.status
      || next.errorCode !== null && value.errorCode !== next.errorCode
    value.status = next.status
    const pending = pendingAttention.get(tabId)
    if (next.status === 'running' && pending && matchesNativeAttempt(value, pending.attempt)) {
      value.activityState = pending.state.activity
      value.observationState = pending.state.observation
      value.attentionState = pending.state.activity === 'waiting' ? 'needs-user' : 'none'
    } else if (next.status !== 'running') {
      value.attentionState = 'none'
      value.activityState = 'unknown'
      value.observationState = 'off'
      if (next.status !== 'starting') useHookStore().invalidateObservation(value)
    }
    if (next.status !== 'starting') pendingAttention.delete(tabId)
    const pendingNotice = pendingNotices.get(tabId)
    if (next.status === 'running' && pendingNotice && matchesNativeAttempt(value, pendingNotice.attempt)) {
      value.observationNotice = copyNotices(pendingNotice.state)
    } else if (next.status !== 'running' && next.status !== 'starting') clearNoticeProjection(tabId)
    if (next.status !== 'starting') pendingNotices.delete(tabId)
    // A repeated healthy poll cannot erase a transport diagnostic or count as activity.
    if (changed) value.errorCode = next.errorCode
    value.launchRevision = launch.revision
    if (['running', 'exited', 'failed', 'cancelled'].includes(launch.phase)) {
      frozenLaunchReceipts.set(tabId, frozenIdentity(value))
    }
    if (changed) value.lastActivityAt = Date.now()
    return true
  }

  function restart(
    tabId: string,
    profile: {
      profileId: string
      profileRevision: string
      cli?: NativeCliKind
    },
  ): NativeCliTab {
    const value = tabs.get(tabId)
    if (!value) throw new Error('TAB_NOT_FOUND')
    if (value.status === 'unknown') throw new Error('LAUNCH_STATE_UNKNOWN')
    if (value.status === 'running' || value.status === 'starting') {
      throw new Error('TAB_STILL_RUNNING')
    }
    if (profile.cli !== undefined && profile.cli !== value.cli) {
      throw new Error('PROFILE_CLI_MISMATCH')
    }
    if (value.generation >= 0xffffffff) throw new Error('GENERATION_EXHAUSTED')

    const nextProfileId = text(profile.profileId, 'PROFILE_ID_REQUIRED')
    const nextProfileRevision = revision(profile.profileRevision)
    useHookStore().clearSession(value.runId)
    value.profileId = nextProfileId
    value.profileRevision = nextProfileRevision
    value.requestId = id('request')
    value.runId = id('run')
    value.generation += 1
    value.status = 'stopped'
    value.attentionState = 'none'
    value.activityState = 'unknown'
    value.observationState = 'off'
    value.errorCode = null
    value.launchRevision = null
    pendingAttention.delete(tabId)
    clearNoticeProjection(tabId)
    noticeLedgers.delete(tabId)
    frozenLaunchReceipts.delete(tabId)
    value.lastActivityAt = Date.now()
    unstartedAttempts.set(tabId, captureNativeAttempt(value))
    activeTabId.value = tabId
    return snapshot(value)
  }

  function close(tabId: string): void {
    const value = tabs.get(tabId)
    if (value) useHookStore().clearSession(value.runId)
    pendingAttention.delete(tabId)
    pendingNotices.delete(tabId)
    noticeLedgers.delete(tabId)
    frozenLaunchReceipts.delete(tabId)
    unstartedAttempts.delete(tabId)
    if (!tabs.delete(tabId)) return
    if (activeTabId.value === tabId) {
      activeTabId.value = tabs.keys().next().value ?? null
    }
  }

  function clear(): void {
    for (const value of tabs.values()) useHookStore().clearSession(value.runId)
    pendingAttention.clear()
    pendingNotices.clear()
    noticeLedgers.clear()
    frozenLaunchReceipts.clear()
    unstartedAttempts.clear()
    tabs.clear()
    activeTabId.value = null
  }

  return {
    tabs,
    activeTabId,
    hasUnstartedAttempt,
    hasFrozenLaunchReceipt,
    hasOwnedObservationNotice,
    create,
    tab,
    byProject,
    setActive,
    markStarting,
    markUnknown,
    markError,
    setDiagnostic,
    touch,
    applyObservation,
    applyObservationNotice,
    ackReplyEndNotice,
    rename,
    applyLaunchStatus,
    restart,
    close,
    clear,
  }
})
