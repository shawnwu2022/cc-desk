import type { NativeObservationNoticeState } from './nativeObservationNotice'

export type UnifiedCliKind = 'claude' | 'codex'
export type SessionRuntimeKind = 'legacy-claude' | 'native-cli'
export type SessionProcessState = 'starting' | 'running' | 'unknown' | 'stopped' | 'failed'
export type SessionAttentionState = 'none' | 'needs-user'
/** Turn activity is independent of process ownership and terminal visibility. */
export type SessionActivityState =
  | 'idle' | 'working' | 'thinking' | 'tool_executing' | 'waiting' | 'waiting_permission'
  | 'waiting_input' | 'subagent_running' | 'compacting' | 'error' | 'unknown'
export type SessionAttentionKind = 'error' | 'permission' | 'completed'
export type SessionObservationState = 'off' | 'connecting' | 'active' | 'unavailable'
export type SessionVisualState =
  | 'starting' | 'running' | 'working' | 'needs-user' | 'permission' | 'completed' | 'error'
  | 'confirming' | 'unknown' | 'stopped' | 'closed' | 'ended' | 'failed'
export type SessionPrimaryAction =
  | 'close'
  | 'cancel-start'
  | 'stop'
  | 'confirm-status'
  | 'resume'
  | 'retry'
  | 'restore-archive'
  | 'save-rename'
export type SessionRenameState = 'idle' | 'editing' | 'saving'
export type SessionMenuAction =
  | Exclude<SessionPrimaryAction, 'save-rename'>
  | 'rename' | 'restart' | 'close' | 'archive' | 'discard-creation'
  | 'copy-session-id' | 'open-project-directory' | 'view-diagnostics' | 'clear-unread'
/** Omitted entries use state defaults; false hides an unsupported capability. */
export type SessionMenuActionVisibility = Partial<Record<SessionMenuAction, boolean>>
export interface SessionMenuActionDefinition {
  id: SessionMenuAction
  labelKey: string
  danger?: boolean
  disabled?: boolean
}

/** Frozen authenticated history origin; never inferred from current UI selection. */
export interface NativeSessionOrigin {
  cli: UnifiedCliKind
  profileId: string
  profileRevision: string
  projectId: string
  projectPath: string
}

export interface ResumeHistoryQuery {
  projectPath: string
  scope: 'current-project' | 'all'
  cli?: UnifiedCliKind
  query?: string
  since?: number
}
export type ResumeDialogRequest = {
  project: UnifiedProjectIdentity
  cli?: UnifiedCliKind
  launchConfigId?: string
  launchConfigRevision?: string
} & (
  | { mode: 'session'; sessionId: string }
  | { mode: 'history' | 'resume-picker' | 'resume-id'; sessionId?: string }
)

export interface UnifiedSession {
  id: string
  projectKey: string
  projectPath: string
  cli: UnifiedCliKind
  runtime: SessionRuntimeKind
  title: string
  /** adapter 按精确元数据身份解析的 Desk 覆盖名；仅供显示，不能据此推断其他来源。 */
  savedDisplayName?: string
  processState: SessionProcessState
  attentionState: SessionAttentionState
  /** Missing/unordered observation means unknown, never idle or completed. */
  activityState?: SessionActivityState
  observationState?: SessionObservationState
  observerReasonCode?: import('@/api/observer').ObserverReasonCode
  /** An explicit attention cause; completion must come from a completion event. */
  attentionKind?: SessionAttentionKind
  /** Authenticated recent/unread event receipts, never a current activity claim. */
  observationNotice?: NativeObservationNoticeState
  lastActivityAt: number
  /** New/reopened/restored time; selecting an already open terminal preserves it. */
  lastOpenedAt?: number
  /** CLI 会话创建时间（毫秒）；创建序排序的稳定键，缺失时 UI 回落首见近似。 */
  createdAt?: number
  archived: boolean
  /** UI projection only; runtime actions must still check the owning store. */
  opened?: boolean
  /** Local creation projection; failed proves admission was never attempted. */
  preparationState?: 'pending' | 'failed' | 'unknown'
  /** Presentation-only explicit resume/archive restore is pending; not process evidence. */
  resumePending?: boolean
  resumable: boolean
  adapterSessionId: string
  nativeSessionId?: string | null
  /** 已授权历史行的明确读取诊断；不改变进程/活动状态。 */
  historyReadFailure?: import('./nativeProjection').HistoryReadFailure
  /** native 来源根身份（SourceRef.sourceRootKey）；仅 native 行携带。折叠重复历史行前必须与 legacy 固定根比较，相等才证明同源。 */
  nativeRootKey?: string
  /** 来自精确历史投影的记录键，仅用于同物理记录的显示去重，不授予操作权限。 */
  nativeHistorySessionKey?: string
  /** legacy 终端本次启动冻结的根证明：为真才可与默认根同 UUID 历史互为孪生。未知视为不可证明。 */
  usesDefaultClaudeRoot?: boolean
  launchConfigId?: string | null
  nativeOrigin?: NativeSessionOrigin
  safeErrorCode?: string | null
  /** Allowlisted preflight code only; never raw errors or configuration values. */
  preparationIssueCode?: string | null
  renameState?: SessionRenameState
  /** 仅标明编辑目标；不能证明当前终端仍停留在恢复来源会话。 */
  renameTarget?: 'resume-source' | 'history-record'
}

export interface UnifiedProjectGroup {
  projectKey: string
  projectPath: string
  name: string
  sessions: UnifiedSession[]
  pinned: boolean
  hidden: boolean
  runningCount: number
  needsUserCount: number
  workingCount?: number
  errorCount?: number
  permissionCount?: number
  completedCount?: number
  unreadReplyCount?: number
  lastActivityAt: number
}

export type UnifiedLaunchAction =
  | { kind: 'new' }
  | { kind: 'resume-picker'; scope: 'current-project' | 'all' }
  | { kind: 'resume-id'; nativeSessionId: string }
  | { kind: 'raw'; argv: string[] }

export interface CreateUnifiedSessionInput {
  projectKey: string
  projectPath: string
  cli: UnifiedCliKind
  launchConfigId?: string | null
  /** Explicit registered project selected for a direct restore. */
  registeredProjectId?: string
  /** Frozen configuration identity, never displayed in the normal flow. */
  launchConfigRevision?: string
  action?: UnifiedLaunchAction
  title?: string
}

export interface ResumeUnifiedSessionInput {
  nativeOrigin?: NativeSessionOrigin
  /** Historical origin, required when adapter identities are ambiguous. */
  runtime?: SessionRuntimeKind
  projectKey: string
  projectPath: string
  cli: UnifiedCliKind
  adapterSessionId: string
  nativeSessionId?: string | null
  launchConfigId?: string | null
  title?: string
}

export type SessionRenameResult = 'input-written' | 'display-name-saved' | 'native-title-saved'

export interface SessionAdapter {
  readonly runtime: SessionRuntimeKind
  /** Freeze process ownership at admission, before a queued async action starts. */
  captureOwnership?(id: string, operation?: 'close' | 'archive'): () => boolean
  listSessions(projectKey?: string, cli?: UnifiedCliKind): Promise<UnifiedSession[]>
  /** 新建独立运行 owner 后可提前交接显示行；回调不代表 CLI 成功，也不授予操作权限。 */
  createSession(input: CreateUnifiedSessionInput, canAdmit?: () => boolean, onAdmitted?: (session: UnifiedSession) => void): Promise<UnifiedSession>
  resumeSession(input: ResumeUnifiedSessionInput, canAdmit?: () => boolean): Promise<UnifiedSession>
  activateSession(id: string): Promise<void> | void
  stopSession(id: string): Promise<void>
  restartSession(id: string, canContinue?: () => boolean): Promise<UnifiedSession>
  closeSession(id: string, canContinue?: () => boolean): Promise<void>
  renameSession(id: string, title: string, canContinue?: () => boolean, onIssued?: () => void, signal?: AbortSignal): Promise<SessionRenameResult | void>
  archiveSession(id: string, canContinue?: () => boolean): Promise<void>
  restoreArchivedSession(id: string): Promise<void>
  /** 仅显式清除与调用方快照匹配的关闭未读；不得清除活动运行的通知。 */
  clearUnread?(id: string, expected: UnifiedSession): Promise<void> | void
  /** Recheck absence without deleting CLI history files. */
  verifyMissingSession?(input: ResumeUnifiedSessionInput): Promise<void>
}

export interface SessionCatalogIdentity {
  runtime: SessionRuntimeKind
  cli: UnifiedCliKind
  projectPath: string
  adapterSessionId: string
  nativeSessionId?: string | null
}

/** Tree requests carry catalog/project identity; the workspace owns runtime dispatch. */
export type UnifiedProjectIdentity = Pick<UnifiedProjectGroup, 'projectKey' | 'projectPath'>
export type ProjectMenuAction = 'pin' | 'unpin' | 'rename' | 'view-archive' | 'open-project-directory' | 'close-project-sessions' | 'remove-project'
export interface ProjectActionRequest extends UnifiedProjectIdentity {
  action: ProjectMenuAction
}
export interface SessionTreeConfirmationRequest extends UnifiedProjectIdentity {
  kind: 'stop-and-archive'
  sessionId: string
}

/** A menu selection; an omitted intent opens the advanced form. */
export interface NewSessionRequest extends UnifiedProjectIdentity { intent?: UnifiedCliKind | 'restore' | 'options' }
