import type { HookEventPayload, NotificationData } from '@/types/hook'
import type { ObservationEvent } from './registry'

function validAuthenticatedMetadata(
  payload: HookEventPayload,
): payload is HookEventPayload & {
  runId: string
  generation: number
  eventId: string
  observerSource: 'claude-hook'
} {
  return (
    payload !== null && typeof payload === 'object' && !Array.isArray(payload) &&
    payload.detail !== null && typeof payload.detail === 'object' &&
    payload.detail.data !== null && typeof payload.detail.data === 'object' && !Array.isArray(payload.detail.data) &&
    payload.observerSource === 'claude-hook' &&
    typeof payload.runId === 'string' &&
    payload.runId.length > 0 && payload.runId.length <= 128 &&
    !/[\u0000-\u001f\u007f]/.test(payload.runId) &&
    Number.isInteger(payload.generation) &&
    payload.generation! > 0 && payload.generation! <= 0xffffffff &&
    typeof payload.eventId === 'string' &&
    payload.eventId.length > 0 &&
    payload.eventId.length <= 128 &&
    /^[A-Za-z0-9._:-]+$/.test(payload.eventId)
  )
}

/**
 * Converts only backend-authenticated Claude observer envelopes. Legacy ptyId
 * events stay on the legacy hook bus and are never promoted to run authority.
 */
export function fromClaudeHook(payload: HookEventPayload): ObservationEvent | null {
  if (!validAuthenticatedMetadata(payload)) return null

  let kind: ObservationEvent['kind'] = 'unknown'
  switch (payload.detail.type) {
    case 'userPromptSubmit':
    case 'preToolUse':
    case 'postToolUse':
    case 'postToolUseFailure':
    case 'subagentStart':
    case 'subagentStop':
    case 'preCompact':
    case 'postCompact':
      kind = 'working'
      break
    case 'notification': {
      const type = (payload.detail.data as NotificationData).notificationType
      if (type === 'permission_prompt' || type === 'worker_permission_prompt') {
        kind = 'waiting'
      }
      break
    }
    default:
      kind = 'unknown'
  }

  return {
    kind,
    runId: payload.runId,
    generation: payload.generation,
    eventId: payload.eventId,
  }
}
