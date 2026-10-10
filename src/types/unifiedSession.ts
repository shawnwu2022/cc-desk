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
  | 'copy-session-id' | 'open-project-directory' | 'view-diagnostics'
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
  processState: SessionProcessState
  attentionState: SessionAttentionState
  /** Missing/unordered observation means unknown, never idle or completed. */
  activityState?: SessionActivityState
  observationState?: SessionObservationState
  /** An explicit attention cause; completion must come from a completion event. */
  attentionKind?: SessionAttentionKind
  /** Authenticated recent/unread event receipts, never a current activity claim. */
  observationNotice?: NativeObservationNoticeState
  lastActivityAt: number
  /** New/reopened/restored time; selecting an already open terminal preserves it. */
  lastOpenedAt?: number
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
  launchConfigId?: string | null
  nativeOrigin?: NativeSessionOrigin
  safeErrorCode?: string | null
  /** Allowlisted preflight code only; never raw errors or configuration values. */
  preparationIssueCode?: string | null
  renameState?: SessionRenameState
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

export interface SessionAdapter {
  readonly runtime: SessionRuntimeKind
  /** Freeze process ownership at admission, before a queued async action starts. */
  captureOwnership?(id: string, operation?: 'close' | 'archive'): () => boolean
  listSessions(projectKey?: string): Promise<UnifiedSession[]>
  createSession(input: CreateUnifiedSessionInput, canAdmit?: () => boolean): Promise<UnifiedSession>
  resumeSession(input: ResumeUnifiedSessionInput, canAdmit?: () => boolean): Promise<UnifiedSession>
  activateSession(id: string): Promise<void> | void
  stopSession(id: string): Promise<void>
  restartSession(id: string, canContinue?: () => boolean): Promise<UnifiedSession>
  closeSession(id: string, canContinue?: () => boolean): Promise<void>
  renameSession(id: string, title: string, canContinue?: () => boolean, onIssued?: () => void): Promise<void>
  archiveSession(id: string, canContinue?: () => boolean): Promise<void>
  restoreArchivedSession(id: string): Promise<void>
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
export type ProjectMenuAction = 'pin' | 'unpin' | 'rename' | 'view-archive' | 'open-project-directory' | 'remove-project'
export interface ProjectActionRequest extends UnifiedProjectIdentity {
  action: ProjectMenuAction
}
export interface SessionTreeConfirmationRequest extends UnifiedProjectIdentity {
  kind: 'stop-and-archive'
  sessionId: string
}

/** A menu selection; an omitted intent opens the advanced form. */
export interface NewSessionRequest extends UnifiedProjectIdentity { intent?: UnifiedCliKind | 'restore' | 'options' }
