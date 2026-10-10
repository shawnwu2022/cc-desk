/** Authenticated event receipts describe an occurrence, not current CLI activity. */
export type NativeObservationNoticeKind =
  | 'prompt-submitted' | 'tool-started' | 'tool-ended' | 'tool-failed'
  | 'reply-ended' | 'reply-failed' | 'permission-requested' | 'input-requested'
  | 'subagent-started' | 'subagent-ended' | 'compaction-started' | 'compaction-ended'

export interface NativeObservationNotice {
  kind: NativeObservationNoticeKind
  eventId: string
  receivedAt: number
  runId: string
  generation: number
}

export interface NativeObservationNoticeState {
  recent: NativeObservationNotice | null
  unreadReplyEnd: NativeObservationNotice | null
}
