/** Authenticated event receipts describe an occurrence, not current CLI activity. */
export type NativeObservationNoticeKind =
  | 'prompt-submitted' | 'tool-started' | 'tool-ended' | 'tool-failed'
  | 'reply-ended' | 'reply-failed' | 'permission-requested' | 'input-requested'
  | 'subagent-started' | 'subagent-ended' | 'compaction-started' | 'compaction-ended'

const failureReasons = ['rate_limit', 'overloaded', 'authentication_failed', 'oauth_org_not_allowed', 'account_on_hold', 'billing_error',
  'invalid_request', 'model_not_found', 'server_error', 'max_output_tokens', 'cloud_credential_error', 'unknown'] as const
export type NativeObservationReasonCode = typeof failureReasons[number] | 'permission_prompt' | 'worker_permission_prompt' | 'idle_prompt'
export function nativeNoticeReasonCode(kind: NativeObservationNoticeKind, value: unknown): NativeObservationReasonCode | undefined {
  if (typeof value !== 'string') return undefined
  if (kind === 'reply-failed' && (failureReasons as readonly string[]).includes(value)) return value as NativeObservationReasonCode
  if (kind === 'permission-requested' && (value === 'permission_prompt' || value === 'worker_permission_prompt')) return value
  if (kind === 'input-requested' && value === 'idle_prompt') return value
  return undefined
}

export interface NativeObservationNotice {
  kind: NativeObservationNoticeKind
  eventId: string
  receivedAt: number
  runId: string
  generation: number
  /** 固定的发生原因，不是当前活动状态的证明。 */
  reasonCode?: NativeObservationReasonCode
}

export interface NativeObservationNoticeState {
  recent: NativeObservationNotice | null
  unreadReplyEnd: NativeObservationNotice | null
  /** 绑定 attempt 的首问线索，后续工具回执不能将它覆盖。 */
  lastPromptSubmitted?: NativeObservationNotice | null
}
