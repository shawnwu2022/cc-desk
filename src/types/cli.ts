export type U64String = string

export type CliKind = 'claude' | 'codex' | 'shell'
export type NativeCliKind = Exclude<CliKind, 'shell'>
export type ResumeScope = 'current-project' | 'all'

export type LaunchAction =
  | { kind: 'new' }
  | { kind: 'resume-picker'; scope: ResumeScope }
  | { kind: 'resume-id'; nativeSessionId: string }
  | { kind: 'raw'; argv: string[] }

export interface LaunchRequest {
  requestId: string
  tabId: string
  runId: string
  generation: number
  profileId: string
  expectedProfileRevision: U64String
  cli: CliKind
  launchCwd: string
  action: LaunchAction
  extraArgs: string[]
  cols: number
  rows: number
}

export type ResolutionSource = 'launch' | 'official-event'

export type Resolution<T> =
  | { state: 'known'; value: T; source: ResolutionSource }
  | { state: 'unknown'; reason: string }

export interface RunPublicIdentity {
  runId: string
  generation: number
  cli: CliKind
  launchCwd: string
  effectiveCwd: Resolution<string>
  effectiveConfigRoot: Resolution<string>
  nativeSessionId: Resolution<string>
}

export interface NativeSessionRef {
  hostId: string
  cli: NativeCliKind
  sourceRootKey: string
  nativeSessionId: string
}

export interface SafeError {
  code: string
  field?: string
  index?: number
  retryable: boolean
}
