export type UnifiedCliKind = 'claude' | 'codex'
export type SessionRuntimeKind = 'legacy-claude' | 'native-cli'
export type SessionProcessState = 'starting' | 'running' | 'unknown' | 'stopped' | 'failed'
export type SessionAttentionState = 'none' | 'needs-user'
export type SessionVisualState = 'starting' | 'running' | 'needs-user' | 'confirming' | 'ended' | 'failed'
export type SessionPrimaryAction =
  | 'cancel-start'
  | 'stop'
  | 'confirm-status'
  | 'resume'
  | 'retry'
  | 'restore-archive'
  | 'save-rename'
export type SessionRenameState = 'idle' | 'editing' | 'saving'

export interface UnifiedSession {
  id: string
  projectKey: string
  projectPath: string
  cli: UnifiedCliKind
  runtime: SessionRuntimeKind
  title: string
  processState: SessionProcessState
  attentionState: SessionAttentionState
  lastActivityAt: number
  archived: boolean
  resumable: boolean
  adapterSessionId: string
  nativeSessionId?: string | null
  launchConfigId?: string | null
  safeErrorCode?: string | null
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
  action?: UnifiedLaunchAction
  title?: string
}

export interface ResumeUnifiedSessionInput {
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
  listSessions(projectKey?: string): Promise<UnifiedSession[]>
  createSession(input: CreateUnifiedSessionInput): Promise<UnifiedSession>
  resumeSession(input: ResumeUnifiedSessionInput): Promise<UnifiedSession>
  activateSession(id: string): Promise<void> | void
  stopSession(id: string): Promise<void>
  restartSession(id: string): Promise<UnifiedSession>
  closeSession(id: string): Promise<void>
  renameSession(id: string, title: string): Promise<void>
  archiveSession(id: string): Promise<void>
  restoreArchivedSession(id: string): Promise<void>
}

export interface SessionCatalogIdentity {
  runtime: SessionRuntimeKind
  cli: UnifiedCliKind
  projectPath: string
  adapterSessionId: string
  nativeSessionId?: string | null
}
