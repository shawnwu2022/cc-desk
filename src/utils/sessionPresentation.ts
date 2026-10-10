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

export function deriveSessionVisualState(session: UnifiedSession, selected = false): SessionVisualState {
  if (session.processState === 'failed') return 'failed'
  if (session.processState === 'starting') return 'starting'
  if (session.processState === 'unknown') return 'confirming'
  if (session.processState === 'stopped') return selected && session.opened === true ? 'stopped' : 'closed'
  const activity = session.activityState ?? 'unknown'
  // Preserve the historical working/error/permission/completed/pending priority.
  if (['working', 'thinking', 'tool_executing', 'subagent_running', 'compacting'].includes(activity)) return 'working'
  if (session.attentionKind === 'error' || activity === 'error') return 'error'
  if (session.attentionKind === 'permission' || activity === 'waiting_permission') return selected ? 'running' : 'permission'
  if (session.attentionKind === 'completed') return selected ? 'running' : 'completed'
  if (session.attentionState === 'needs-user' || activity === 'waiting' || activity === 'waiting_input') return selected ? 'running' : 'needs-user'
  return activity === 'idle' ? 'running' : 'unknown'
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
export function selectSessionObservationNotice(session: UnifiedSession): { labelKey: string; unread: boolean } | null {
  if (session.runtime !== 'native-cli' || session.cli !== 'claude' || session.processState !== 'running' || session.archived) return null
  const notice = session.observationNotice
  if (notice?.unreadReplyEnd?.kind === 'reply-ended') return { labelKey: 'nativeNoticeReplyEndUnread', unread: true }
  const labelKey = notice?.recent && noticeLabelKeys[notice.recent.kind]
  return labelKey ? { labelKey, unread: false } : null
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
const isOpened = (session: UnifiedSession) => !session.archived && session.opened === true
const isState = (...states: UnifiedSession['processState'][]) =>
  (session: UnifiedSession) => !session.archived && states.includes(session.processState)

/** Both menu entry points consume this one allowlisted action model. */
export const SESSION_MENU_ACTION_DEFINITIONS: readonly MenuActionRule[] = [
  { id: 'rename', labelKey: 'sessionActionRename', visible: (session) => !session.preparationState },
  { id: 'cancel-start', labelKey: 'sessionActionCancelStart', danger: true, visible: isState('starting') },
  { id: 'stop', labelKey: 'sessionActionStop', danger: true, visible: () => false },
  { id: 'confirm-status', labelKey: 'sessionActionConfirmStatus', visible: isState('unknown') },
  { id: 'resume', labelKey: 'sessionActionResume', visible: (session) => isState('stopped')(session) && session.resumable },
  { id: 'retry', labelKey: 'sessionActionRetry', visible: isState('failed') },
  { id: 'restart', labelKey: 'sessionActionRestart', danger: true, visible: (session) => isOpened(session) && isState('running', 'stopped', 'failed')(session) },
  { id: 'close', labelKey: 'sessionActionClose', danger: true, visible: () => false },
  { id: 'discard-creation', labelKey: 'sessionActionDiscardCreation', danger: true, visible: (session) => !session.archived && !session.opened && session.preparationState === 'failed' && session.processState === 'failed' },
  { id: 'archive', labelKey: 'sessionActionArchive', danger: true, visible: () => false },
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
