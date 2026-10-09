import type { HookEventPayload } from '@/types/hook'
import type { NativeObservationNotice, NativeObservationNoticeKind } from '@/types/nativeObservationNotice'
import { fromClaudeHook } from './claudeObserver'

const kinds: Readonly<Record<string, NativeObservationNoticeKind>> = {
  userPromptSubmit: 'prompt-submitted', preToolUse: 'tool-started', postToolUse: 'tool-ended',
  postToolUseFailure: 'tool-failed', stop: 'reply-ended', stopFailure: 'reply-failed',
  subagentStart: 'subagent-started', subagentStop: 'subagent-ended',
  preCompact: 'compaction-started', postCompact: 'compaction-ended',
}

/** Receipt evidence only. No provider ordering, activity or completion claim.
 * The native bus is backend-authenticated; reuse its exact-run envelope checks
 * and retain only fixed kinds/IDs, never arbitrary CLI messages or errors. */
export function fromNativeObservationNotice(payload: HookEventPayload, receivedAt = Date.now()): NativeObservationNotice | null {
  const event = fromClaudeHook(payload)
  if (!event || !Number.isSafeInteger(receivedAt) || receivedAt < 0) return null
  let kind: NativeObservationNoticeKind | undefined
  if (payload.detail.type === 'notification') {
    const reason = payload.detail.data.notificationType
    if (reason === 'permission_prompt' || reason === 'worker_permission_prompt') kind = 'permission-requested'
    else if (reason === 'idle_prompt') kind = 'input-requested'
  } else if (Object.prototype.hasOwnProperty.call(kinds, payload.detail.type)) kind = kinds[payload.detail.type]
  if (!kind) return null
  return { kind, eventId: event.eventId!, receivedAt, runId: event.runId, generation: event.generation }
}
