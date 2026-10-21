import type { HookEventPayload, NotificationData } from '@/types/hook'
import { validObservationAgentId, type ObservationEvent } from './registry'

function validAuthenticatedMetadata(
  payload: HookEventPayload, source: 'claude-hook' | 'codex-hook' | 'opencode-hook' = 'claude-hook',
): payload is HookEventPayload & {
  runId: string
  generation: number
  eventId: string
  observerSource: 'claude-hook' | 'codex-hook' | 'opencode-hook'
} {
  return (
    payload !== null && typeof payload === 'object' && !Array.isArray(payload) &&
    payload.detail !== null && typeof payload.detail === 'object' &&
    payload.detail.data !== null && typeof payload.detail.data === 'object' && !Array.isArray(payload.detail.data) &&
    payload.observerSource === source &&
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
    case 'preCompact':
    case 'postCompact':
      kind = 'working'
      break
    // Stop 是宿主可认证的 turn 结束证据；迟到的工具回执由静默超时约束。
    case 'stop':
      kind = 'turn-end'
      break
    case 'notification': {
      const type = (payload.detail.data as NotificationData).notificationType
      if (type === 'permission_prompt' || type === 'worker_permission_prompt' || type === 'idle_prompt') {
        kind = 'waiting'
      }
      break
    }
    default:
      kind = 'unknown'
  }

  if (payload.detail.type === 'subagentStart' || payload.detail.type === 'subagentStop') {
    return {
      kind: payload.detail.type === 'subagentStart' ? 'subagent-started' : 'subagent-stopped',
      runId: payload.runId, generation: payload.generation, eventId: payload.eventId,
      ...(validObservationAgentId(payload.detail.data.agentId) ? { agentId: payload.detail.data.agentId } : {}),
    }
  }

  return {
    kind,
    runId: payload.runId,
    generation: payload.generation,
    eventId: payload.eventId,
  }
}

/** Codex身份信号独立于Claude活动语义，只接受后端认证的主会话生命周期。 */
export function fromCodexHook(payload: HookEventPayload): ObservationEvent | null {
  if (!validAuthenticatedMetadata(payload, 'codex-hook') || !['sessionStart', 'userPromptSubmit', 'stop', 'sessionEnd'].includes(payload.detail.type)) return null
  return { kind: 'unknown', runId: payload.runId, generation: payload.generation, eventId: payload.eventId }
}

export function fromOpencodeHook(payload: HookEventPayload): ObservationEvent | null {
  if (!validAuthenticatedMetadata(payload, 'opencode-hook') || !/^o[0-9]{16}$/.test(payload.eventId)
    || typeof payload.sessionId !== 'string' || !/^ses_[A-Za-z0-9_]+$/.test(payload.sessionId) || payload.sessionId.length > 256) return null
  if (payload.detail.type !== 'notification' && payload.detail.type !== 'sessionStart') return null
  const raw = payload.detail.type === 'notification' ? payload.detail.data.notificationType : undefined
  const value = typeof raw === 'string' && raw.startsWith('opencode_') ? raw.slice('opencode_'.length) : 'unknown'
  const status = ['idle','busy','retry','permission','question','error'].includes(value ?? '') ? value as import('./registry').OpencodeStatus : 'unknown'
  return { kind:'opencode-status', status, runId:payload.runId, generation:payload.generation, eventId:payload.eventId,
    sourceSequence:payload.eventId.slice(1).replace(/^0+(?=\d)/,'') }
}
