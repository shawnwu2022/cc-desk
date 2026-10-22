import type {
  SessionCatalogIdentity,
  SessionMenuAction,
  SessionMenuActionDefinition,
  SessionMenuActionVisibility,
  SessionPrimaryAction,
  SessionVisualState,
  UnifiedSession,
} from '@/types/unifiedSession'
import type { NativeObservationNoticeKind } from '@/types/nativeObservationNotice'
import { nativeNoticeReasonCode, type NativeObservationReasonCode } from '@/types/nativeObservationNotice'

function requiredIdentity(value: string, code: string): string {
  const text = value.trim()
  if (!text) throw new Error(`${code}_REQUIRED`)
  if (text.includes('\0')) throw new Error(`${code}_INVALID`)
  return text
}

function normalizeProjectPath(value: string): string {
  let path = requiredIdentity(value, 'PROJECT_PATH').replace(/\\/g, '/')
  const unc = path.startsWith('//')
  path = path.replace(/\/{2,}/g, '/')
  if (unc) path = `/${path}`
  const isDriveRoot = /^[A-Za-z]:\/$/.test(path)
  if (path.length > 1 && !isDriveRoot) path = path.replace(/\/+$/, '')
  const windowsLike = /^[A-Za-z]:\//.test(path) || path.startsWith('//')
  return windowsLike ? path.toLocaleLowerCase('en-US') : path
}

export function makeSessionCatalogKey(identity: SessionCatalogIdentity): string {
  const projectPath = normalizeProjectPath(identity.projectPath)
  const adapterSessionId = requiredIdentity(identity.adapterSessionId, 'ADAPTER_SESSION_ID')
  const nativeSessionId = identity.nativeSessionId == null
    ? ''
    : requiredIdentity(identity.nativeSessionId, 'NATIVE_SESSION_ID')

  return JSON.stringify([
    'cc-desk-session-v1',
    identity.runtime,
    identity.cli,
    projectPath,
    adapterSessionId,
    nativeSessionId,
  ])
}

/** UI editing belongs to this exact source, independently of refreshed display/status fields. */
export function makeSessionRenameOwnerKey(session: UnifiedSession): string {
  const origin = session.nativeOrigin
  return JSON.stringify([session.id, makeSessionCatalogKey(session), session.launchConfigId ?? null, session.archived,
    origin ? [origin.cli, origin.profileId, origin.profileRevision, origin.projectId, normalizeProjectPath(origin.projectPath)] : null])
}

/** 配置可以共享物理记录；折叠投影后仍由保留行原有 scope 授权。 */
export function nativeHistoryRecordKey(session: UnifiedSession): string | null {
  if (session.runtime !== 'native-cli' || !session.nativeRootKey || !session.nativeSessionId
    || !session.nativeHistorySessionKey || !session.nativeOrigin) return null
  return JSON.stringify([session.cli, session.nativeRootKey, session.nativeOrigin.projectId,
    normalizeProjectPath(session.projectPath), session.nativeSessionId, session.nativeHistorySessionKey])
}

export function deriveSessionVisualState(session: UnifiedSession, selected = false): SessionVisualState {
  if (session.processState === 'failed') return 'failed'
  if (session.processState === 'starting') return 'starting'
  if (session.processState === 'unknown') return 'confirming'
  if (session.processState === 'stopped') return selected && session.opened === true ? 'stopped' : 'closed'
  const activity = session.activityState ?? 'unknown'
  // Selection changes viewing ownership, never the observed turn status.
  // A causal subagent lifecycle is a narrower hint than an explicit blocked/error cause.
  if (activity === 'subagent_running' && (session.attentionKind === 'permission' || session.attentionKind === 'error')) return session.attentionKind
  if (['working', 'thinking', 'tool_executing', 'subagent_running', 'compacting'].includes(activity)) return 'working'
  if (session.attentionKind === 'error' || activity === 'error') return 'error'
  if (session.attentionKind === 'permission' || activity === 'waiting_permission') return 'permission'
  if (session.attentionKind === 'completed') return 'completed'
  if (session.attentionState === 'needs-user' || activity === 'waiting' || activity === 'waiting_input') return 'needs-user'
  if (activity === 'idle') return 'running'
  if (activity === 'unknown' && session.runtime === 'native-cli'
    && session.observationState && session.observerReasonCode !== 'OBSERVER_UNSUPPORTED') {
    return session.observationState === 'connecting' ? 'confirming' : 'unknown'
  }
  // 不支持观察通道（raw）时无从得知 turn 状态；进程仍在
  // 运行,显示中性运行态而不是看似异常的"活动未知"。
  if (activity === 'unknown' && session.observationState !== 'active') return 'running'
  return 'unknown'
}

export function sessionVisualLabelKey(state: SessionVisualState, activity?: UnifiedSession['activityState'], archived = false): string {
  if (archived) return 'sessionStatusArchived'
  if (state === 'working') {
    const details = { thinking: 'sessionStatusThinking', tool_executing: 'sessionStatusToolExecuting', subagent_running: 'sessionStatusSubagent', compacting: 'sessionStatusCompacting' }
    if (activity && activity in details) return details[activity as keyof typeof details]
  }
  if (state === 'needs-user' && activity === 'waiting_input') return 'sessionStatusWaitingInput'
  const keys: Record<SessionVisualState, string> = { starting: 'sessionStatusStarting', running: 'sessionStatusRunning', working: 'sessionStatusWorking', 'needs-user': 'sessionStatusNeedsUser', permission: 'sessionStatusPermission', completed: 'sessionStatusCompleted', error: 'sessionStatusError', confirming: 'sessionStatusConfirming', unknown: 'sessionStatusUnknown', stopped: 'sessionStatusStopped', closed: 'sessionStatusClosed', ended: 'sessionStatusEnded', failed: 'sessionStatusFailed' }
  return keys[state]
}

const noticeLabelKeys: Record<NativeObservationNoticeKind, string> = {
  'prompt-submitted': 'nativeNoticePromptSubmitted', 'tool-started': 'nativeNoticeToolStarted',
  'tool-ended': 'nativeNoticeToolEnded', 'tool-failed': 'nativeNoticeToolFailed',
  'reply-ended': 'nativeNoticeReplyEnded', 'reply-failed': 'nativeNoticeReplyFailed',
  'permission-requested': 'nativeNoticePermissionRequested', 'input-requested': 'nativeNoticeInputRequested',
  'subagent-started': 'nativeNoticeSubagentStarted', 'subagent-ended': 'nativeNoticeSubagentEnded',
  'compaction-started': 'nativeNoticeCompactionStarted', 'compaction-ended': 'nativeNoticeCompactionEnded',
}
/** Occurrence metadata stays separate from the current activity/status icon. */
const noticeReasonLabelKeys: Record<NativeObservationReasonCode,string> = {
  rate_limit:'nativeNoticeReasonRateLimit', overloaded:'nativeNoticeReasonOverloaded', authentication_failed:'nativeNoticeReasonAuthentication',
  oauth_org_not_allowed:'nativeNoticeReasonOrganization', account_on_hold:'nativeNoticeReasonAccount', billing_error:'nativeNoticeReasonBilling',
  invalid_request:'nativeNoticeReasonInvalidRequest', model_not_found:'nativeNoticeReasonModel', server_error:'nativeNoticeReasonServer',
  max_output_tokens:'nativeNoticeReasonOutputLimit', cloud_credential_error:'nativeNoticeReasonCloudCredential', unknown:'nativeNoticeReasonUnknown',
  permission_prompt:'nativeNoticeReasonPermission', worker_permission_prompt:'nativeNoticeReasonWorkerPermission', idle_prompt:'nativeNoticeReasonInput',
}
export function selectSessionObservationNotice(session: UnifiedSession): { labelKey: string; unread: boolean; detailLabelKey?: string } | null {
  if (session.runtime !== 'native-cli' || session.cli !== 'claude' || !['running', 'stopped'].includes(session.processState)) return null
  const notice = session.observationNotice
  // 未读提示只描述其自身回执；后续失败原因在确认未读后随最近回执展示。
  if (notice?.unreadReplyEnd?.kind === 'reply-ended') return { labelKey: 'nativeNoticeReplyEndUnread', unread: true }
  const reason=notice?.recent && nativeNoticeReasonCode(notice.recent.kind,notice.recent.reasonCode)
  const details=reason ? {detailLabelKey:noticeReasonLabelKeys[reason]} : {}
  const labelKey = notice?.recent && noticeLabelKeys[notice.recent.kind]
  return labelKey ? { labelKey, unread: false, ...details } : null
}

export function selectSessionPrimaryAction(session: UnifiedSession): SessionPrimaryAction | null {
  if (session.renameState === 'editing' || session.renameState === 'saving') return 'save-rename'
  if (session.archived) return 'restore-archive'
  if (session.opened) return 'close'

  switch (session.processState) {
    case 'starting':
      return 'cancel-start'
    case 'running':
      return null
    case 'unknown':
      return 'confirm-status'
    case 'failed':
      return 'retry'
    case 'stopped':
      return session.resumable ? 'resume' : null
  }
}

interface MenuActionRule extends SessionMenuActionDefinition {
  visible: (session: UnifiedSession) => boolean
}
export function canRenameSession(session: UnifiedSession): boolean {
  return session.renameAvailable === true && !session.preparationState && !session.archived
}
const isOpened = (session: UnifiedSession) => !session.archived && session.opened === true
const isState = (...states: UnifiedSession['processState'][]) =>
  (session: UnifiedSession) => !session.archived && states.includes(session.processState)

/** Both menu entry points consume this one allowlisted action model. */
export const SESSION_MENU_ACTION_DEFINITIONS: readonly MenuActionRule[] = [
  { id: 'clear-unread', labelKey: 'sessionActionClearUnread', visible: session => !session.opened && !!session.observationNotice?.unreadReplyEnd },
  { id: 'rename', labelKey: 'sessionActionRename', visible: canRenameSession },
  { id: 'cancel-start', labelKey: 'sessionActionCancelStart', danger: true, visible: isState('starting') },
  { id: 'stop', labelKey: 'sessionActionStop', danger: true, visible: () => false },
  { id: 'confirm-status', labelKey: 'sessionActionConfirmStatus', visible: isState('unknown') },
  { id: 'resume', labelKey: 'sessionActionResume', visible: (session) => isState('stopped')(session) && session.resumable },
  { id: 'retry', labelKey: 'sessionActionRetry', visible: isState('failed') },
  { id: 'restart', labelKey: 'sessionActionRestart', danger: true, visible: (session) => isOpened(session) && isState('running', 'stopped', 'failed')(session) },
  { id: 'close', labelKey: 'sessionActionClose', danger: true, visible: () => false },
  { id: 'discard-creation', labelKey: 'sessionActionDiscardCreation', danger: true, visible: (session) => !session.archived && !session.opened && session.preparationState === 'failed' && session.processState === 'failed' },
  { id: 'archive', labelKey: 'sessionActionArchive', danger: true, visible: session => !!selectSessionArchiveAction(session)
    && (session.opened === true || selectSessionPrimaryAction(session) === 'retry') },
  { id: 'restore-archive', labelKey: 'sessionActionRestoreArchive', visible: (session) => session.archived },
  { id: 'copy-session-id', labelKey: 'sessionActionCopyId', visible: () => true },
  { id: 'open-project-directory', labelKey: 'sessionActionOpenProject', visible: () => true },
  { id: 'view-diagnostics', labelKey: 'sessionActionDiagnostics', visible: () => true },
]
export function sessionActionLabelKey(action: SessionMenuAction | SessionPrimaryAction, session: UnifiedSession): string {
  if (action === 'save-rename') return 'sessionActionSaveRename'
  if (action === 'archive' && session.processState === 'running') return 'sessionActionStopAndArchive'
  return SESSION_MENU_ACTION_DEFINITIONS.find((definition) => definition.id === action)!.labelKey
}
/** Archive is a trailing quick action, with the existing ended-session admission policy. */
export function selectSessionArchiveAction(session: UnifiedSession, visibility: SessionMenuActionVisibility = {}): SessionMenuActionDefinition | null {
  if (visibility.archive === false || session.preparationState || !isState('stopped', 'failed')(session)) return null
  return { id: 'archive', labelKey: 'sessionActionArchive', danger: true, disabled: session.renameState === 'saving' }
}
export function selectSessionMenuActions(session: UnifiedSession, visibility: SessionMenuActionVisibility = {}): SessionMenuActionDefinition[] {
  return SESSION_MENU_ACTION_DEFINITIONS
    .filter((definition) => definition.visible(session) && visibility[definition.id] !== false
      && !((session.archived || session.opened !== true) && ['resume', 'retry', 'restore-archive'].includes(definition.id) && definition.id === selectSessionPrimaryAction(session)))
    .map(({ id, danger }) => ({ id, labelKey: sessionActionLabelKey(id, session), danger,
      disabled: session.renameState === 'saving' || (id === 'copy-session-id' && !session.nativeSessionId),
    }))
    .sort((a, b) => Number(!!a.danger) - Number(!!b.danger))
}
