<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppTooltip from '@/components/ui/AppTooltip.vue'
import type { SessionVisualState, SessionActivityState } from '@/types/unifiedSession'
import starting from '@/assets/icons/session-status/starting.svg?raw'
import running from '@/assets/icons/session-status/running.svg?raw'
import needsUser from '@/assets/icons/session-status/needs-user.svg?raw'
import confirming from '@/assets/icons/session-status/confirming.svg?raw'
import ended from '@/assets/icons/session-status/ended.svg?raw'
import failed from '@/assets/icons/session-status/failed.svg?raw'
import working from '@/assets/icons/session-status/working.svg?raw'
import permission from '@/assets/icons/session-status/permission.svg?raw'
import completed from '@/assets/icons/session-status/completed.svg?raw'
import stopped from '@/assets/icons/session-status/stopped.svg?raw'
import closed from '@/assets/icons/session-status/closed.svg?raw'
import unknown from '@/assets/icons/session-status/unknown.svg?raw'

import thinking from '@/assets/icons/session-status/thinking.svg?raw'
import toolExecuting from '@/assets/icons/session-status/tool-executing.svg?raw'
import subagent from '@/assets/icons/session-status/subagent.svg?raw'
import compacting from '@/assets/icons/session-status/compacting.svg?raw'
import waitingInput from '@/assets/icons/session-status/waiting-input.svg?raw'
import archived from '@/assets/icons/session-status/archived.svg?raw'

const props = defineProps<{ state: SessionVisualState; activityState?: SessionActivityState; archived?: boolean; transitionState?: SessionVisualState }>()
const { t } = useI18n()
const visuals: Record<SessionVisualState, { svg: string; labelKey: string }> = {
  starting: { svg: starting, labelKey: 'sessionStatusStarting' },
  running: { svg: running, labelKey: 'sessionStatusRunning' },
  'needs-user': { svg: needsUser, labelKey: 'sessionStatusNeedsUser' },
  confirming: { svg: confirming, labelKey: 'sessionStatusConfirming' },
  ended: { svg: ended, labelKey: 'sessionStatusEnded' },
  failed: { svg: failed, labelKey: 'sessionStatusFailed' },
  working: { svg: working, labelKey: 'sessionStatusWorking' },
  permission: { svg: permission, labelKey: 'sessionStatusPermission' },
  completed: { svg: completed, labelKey: 'sessionStatusCompleted' },
  error: { svg: failed, labelKey: 'sessionStatusError' },
  stopped: { svg: stopped, labelKey: 'sessionStatusStopped' },
  closed: { svg: closed, labelKey: 'sessionStatusClosed' },
  unknown: { svg: unknown, labelKey: 'sessionStatusUnknown' },
}
const details = {
  thinking: { svg: thinking, labelKey: 'sessionStatusThinking' },
  tool_executing: { svg: toolExecuting, labelKey: 'sessionStatusToolExecuting' },
  subagent_running: { svg: subagent, labelKey: 'sessionStatusSubagent' },
  compacting: { svg: compacting, labelKey: 'sessionStatusCompacting' },
}
const visual = computed(() => {
  if (props.archived) return { svg: archived, labelKey: 'sessionStatusArchived' }
  if (props.state === 'working' && props.activityState && props.activityState in details) return details[props.activityState as keyof typeof details]
  if (props.state === 'needs-user' && props.activityState === 'waiting_input') return { svg: waitingInput, labelKey: 'sessionStatusWaitingInput' }
  return visuals[props.state]
})
// Selection can suppress a badge without changing its underlying activity.
// Watch that independent projection, never remount/refresh or occurrence notices.
const entryMotion = ref<'permission' | 'needs-user' | 'completed' | null>(null)
const transitionState = computed(() => props.transitionState ?? props.state)
watch(() => JSON.stringify([transitionState.value, props.activityState ?? null, props.archived ?? false]), (next, previous) => {
  entryMotion.value = null
  if (next === previous || JSON.parse(previous)[0] === transitionState.value || props.archived || props.state !== transitionState.value
    || window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) return
  const state = transitionState.value
  if (state === 'permission' || state === 'needs-user' || state === 'completed') entryMotion.value = state
})
function finishEntryMotion(event: AnimationEvent) {
  if (!entryMotion.value || !['session-status-entry', 'session-completion-entry'].includes(event.animationName)) return
  const trigger = event.currentTarget
  if (!(trigger instanceof HTMLElement) || !(event.target instanceof Node) || !trigger.contains(event.target)) return
  entryMotion.value = null
}
const label = computed(() => t(visual.value.labelKey))
</script>

<template>
  <AppTooltip :text="label">
    <span class="session-status-icon" :class="[`session-status-icon--${state}`, { 'session-status-icon--thinking': state === 'working' && activityState === 'thinking' }, entryMotion && `session-status-icon--entry-${entryMotion}`]"
      :data-status-entry="entryMotion ?? undefined" @animationend="finishEntryMotion" @animationcancel="finishEntryMotion" role="img" :aria-label="label" tabindex="0">
      <!-- Only these allowlisted, bundled self-owned SVGs enter this sink; no caller markup. -->
      <span :key="visual.labelKey" class="session-status-icon__shape" aria-hidden="true" v-html="visual.svg" />
    </span>
  </AppTooltip>
</template>

<style scoped>
.session-status-icon {
  --session-status-info: #2a5082;
  --session-status-success: #367e63;
  --session-status-error: #c45c4a;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  flex: 0 0 16px;
  border-radius: 50%;
  color: var(--text-secondary);
  vertical-align: middle;
}
:global([data-theme="dark"]) .session-status-icon {
  --session-status-info: #82acdc;
  --session-status-success: #5dad8e;
  --session-status-error: #f28a78;
}
.session-status-icon__shape { display: inline-flex; width: 16px; height: 16px; transform-origin: center; }
.session-status-icon:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
.session-status-icon--starting { color: var(--session-status-info); }
.session-status-icon--running { color: var(--session-status-success); }
.session-status-icon--working, .session-status-icon--completed { color: var(--session-status-success); }
.session-status-icon--permission { color: var(--accent-gold-text); }
.session-status-icon--needs-user { color: var(--accent-gold-text); }
.session-status-icon--ended { color: var(--text-secondary); }
.session-status-icon--stopped, .session-status-icon--closed, .session-status-icon--unknown { color: var(--text-secondary); }
.session-status-icon--failed { color: var(--session-status-error); }
.session-status-icon--error { color: var(--session-status-error); }
.session-status-icon--starting .session-status-icon__shape { animation: session-status-start 2.4s ease-in-out infinite; }
.session-status-icon--working .session-status-icon__shape { animation: session-status-pulse 1.8s ease-in-out infinite; }
.session-status-icon--thinking .session-status-icon__shape { animation: none; }
.session-status-icon--working .session-status-icon__shape :deep([data-thinking-dot]) { animation: session-thinking-dot 1.2s ease-in-out infinite; }
.session-status-icon__shape :deep([data-thinking-dot="1"]) { animation-delay: 180ms; }
.session-status-icon__shape :deep([data-thinking-dot="2"]) { animation-delay: 360ms; }
.session-status-icon--entry-permission .session-status-icon__shape,
.session-status-icon--entry-needs-user .session-status-icon__shape { animation: session-status-entry 280ms ease-out 1; }
.session-status-icon--entry-completed .session-status-icon__shape :deep([data-completion-mark]) { animation: session-completion-entry 240ms ease-out 1; }
@keyframes session-status-start { 50% { transform: scale(.94); } }
@keyframes session-status-pulse { 50% { transform: scale(.96); } }
@keyframes session-thinking-dot { 0%, 65%, 100% { opacity: .8; } 25% { opacity: 1; } }
@keyframes session-status-entry { 50% { transform: scale(1.1); } }
@keyframes session-completion-entry { from { stroke-dasharray: 12; stroke-dashoffset: 12; } to { stroke-dasharray: 12; stroke-dashoffset: 0; } }
@media (prefers-reduced-motion: reduce) {
  .session-status-icon .session-status-icon__shape,
  .session-status-icon .session-status-icon__shape :deep([data-thinking-dot]),
  .session-status-icon .session-status-icon__shape :deep([data-completion-mark]) { animation: none; }
}
</style>
