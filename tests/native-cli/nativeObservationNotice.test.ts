import { describe, expect, it } from 'vitest'
import { fromNativeObservationNotice } from '@/integrations/nativeObservationNotice'
import { fromClaudeHook } from '@/integrations/claudeObserver'
import { createObservationReducer } from '@/integrations/registry'
import type { HookEventDetail, HookEventPayload } from '@/types/hook'

function payload(detail: HookEventDetail): HookEventPayload {
  return { ptyId: null, sessionId: 'provider-session', eventName: 'untrusted display text', state: 'unknown',
    timestamp: 999999, runId: 'owned-run', generation: 2, eventId: 'receipt-1', observerSource: 'claude-hook', detail }
}
describe('Authenticated Native event receipt, separate from current activity', () => {
  it.each([
    ['userPromptSubmit', 'prompt-submitted'], ['preToolUse', 'tool-started'], ['postToolUse', 'tool-ended'],
    ['postToolUseFailure', 'tool-failed'], ['stop', 'reply-ended'], ['stopFailure', 'reply-failed'],
    ['subagentStart', 'subagent-started'], ['subagentStop', 'subagent-ended'],
    ['preCompact', 'compaction-started'], ['postCompact', 'compaction-ended'],
  ] as const)('Notice_ExplicitKindOnly_001_%s', (type, kind) => {
    const input = payload({ type, data: { prompt: 'SECRET', message: 'SECRET', lastAssistantMessage: 'SECRET', error: '/private/SECRET' } } as HookEventDetail)
    expect(fromNativeObservationNotice(input, 1234)).toEqual({ kind, eventId: 'receipt-1', receivedAt: 1234, runId: 'owned-run', generation: 2 })
  })
  it.each([
    ['idle_prompt', 'input-requested'], ['permission_prompt', 'permission-requested'], ['worker_permission_prompt', 'permission-requested'],
  ] as const)('Notice_AllowlistedNotification_002_%s', (notificationType, kind) => {
    expect(fromNativeObservationNotice(payload({ type: 'notification', data: { notificationType, message: 'SECRET' } }), 1234)?.kind).toBe(kind)
  })
  it.each(['auth_success', 'arbitrary', '__proto__', 'constructor', ''])('Notice_UnknownReasonExcluded_003_%s', notificationType => {
    expect(fromNativeObservationNotice(payload({ type: 'notification', data: { notificationType } }), 1234)).toBeNull()
  })
  it.each([
    { runId: undefined }, { runId: '' }, { generation: 0 }, { generation: 1.5 }, { eventId: '' },
    { eventId: 'invalid\n' }, { eventId: 'x'.repeat(129) }, { observerSource: 'unknown' },
    { detail: { type: 'stop', data: [] } }, { runId: undefined, ptyId: 'legacy-pty' },
  ])('Notice_RequiresAuthenticatedEnvelope_004_%j', patch => {
    expect(fromNativeObservationNotice({ ...payload({ type: 'stop', data: {} }), ...patch } as HookEventPayload, 1234)).toBeNull()
  })
  it.each([NaN, Infinity, -1, Number.MAX_SAFE_INTEGER + 1])('Notice_ReceiptTimeIsBounded_005_%s', receivedAt => {
    expect(fromNativeObservationNotice(payload({ type: 'stop', data: {} }), receivedAt)).toBeNull()
  })
  it('Notice_ReplyEndCannotBecomeCurrentCompleted_006', () => {
    const input = payload({ type: 'stop', data: { stopHookActive: true, lastAssistantMessage: 'SECRET' } })
    const reducer = createObservationReducer({ runId: 'owned-run', generation: 2 })
    reducer.accept(fromClaudeHook(input)!)
    expect(reducer.state()).toEqual({ observation: 'active', activity: 'unknown' })
    expect(fromNativeObservationNotice(input, 1234)?.kind).toBe('reply-ended')
    expect(JSON.stringify(fromNativeObservationNotice(input, 1234))).not.toContain('SECRET')
  })
})
