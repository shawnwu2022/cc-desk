import type { NativeCliKind } from './cli'
export type ScopeTarget =
  | { kind: 'profile'; profileId: string; expectedProfileRevision: string; projectIds: string[] }
  | { kind: 'run'; runId: string; generation: number }
export type ResourceKind = 'history' | 'messages' | 'search' | 'config' | 'mcp' | 'skills' | 'agents' | 'plugins' | 'instructions'
export interface SourceRef {
  scopeId: string; instanceId: string; cli: NativeCliKind; sourceRootKey: string; identityEpoch: string
  profileId: string; profileRevision: string; target: ScopeTarget; basis: 'configured-profile' | 'launch-environment'
}
export interface NativeRenameRequest { operationId: string; source: SourceRef; sessionKey: string; title: string }
export interface NativeRenameReceipt extends NativeRenameRequest { state: 'renamed' }
export interface ReadRequest {
  source: SourceRef; resourceKind: ResourceKind; requestEpoch: string
  query?: string | null; sessionId?: string | null; limit?: number; offset?: number
}
export type ResourceItem =
  | { type: 'session'; sessionKey: string; nativeSessionId: string; projectId?: string; title: string; titleUnknown?: boolean; titleSource?: 'prompt' | 'ai' | 'custom'; metadataIncomplete?: boolean; truncated: boolean; archived: boolean; cwd: string | null; updatedAt: string | null; createdAt?: string | null }
  | { type: 'message'; sessionKey: string; nativeSessionId: string; role: string; text: string; truncated: boolean }
  | { type: 'setting'; name: string; value: string; origin: string }
  | { type: 'mcp'; name: string; transport: string; origin: string }
  | { type: 'skill'; name: string; description: string; origin: string }
  | { type: 'agent'; name: string; description: string; model: string | null; origin: string }
  | { type: 'plugin'; id: string; name: string; version: string | null; enabled: boolean | null; installed: boolean | null; origin: string }
  | { type: 'document'; name: string; text: string; truncated: boolean; origin: string }
export interface ProjectionResult {
  source: SourceRef; resourceKind: ResourceKind; requestEpoch: string; observedAt: string
  state: 'ready' | 'unavailable'; reason: string | null; items: ResourceItem[]; hasMore: boolean
  historyMetadataIncomplete?: boolean
  historyReadFailures?: HistoryReadFailure[]
}
export type HistoryReadFailure = 'SOURCE_UNSUPPORTED' | 'SOURCE_INVALID' | 'SOURCE_INVALID_TEXT' | 'SOURCE_TOO_LARGE' | 'SOURCE_READ_FAILED'
/** 恢复 CLI 自身归档会话的最小请求；归档位置由 CLI 管理，Desk 只转达意图。 */
export interface UnarchiveRequest {
  requestId: string; cli: 'codex'; profileId: string; expectedProfileRevision: string
  projectPath: string; nativeSessionId: string
}
