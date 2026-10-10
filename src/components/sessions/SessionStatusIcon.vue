<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import AppTooltip from '@/components/ui/AppTooltip.vue'
import type { SessionVisualState } from '@/types/unifiedSession'
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

const props = defineProps<{ state: SessionVisualState }>()
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
const visual = computed(() => visuals[props.state])
const label = computed(() => t(visual.value.labelKey))
</script>

<template>
  <AppTooltip :text="label">
    <span class="session-status-icon" :class="`session-status-icon--${state}`" role="img" :aria-label="label" tabindex="0">
      <!-- Only these allowlisted, bundled self-owned SVGs enter this sink; no caller markup. -->
      <span :key="state" class="session-status-icon__shape" aria-hidden="true" v-html="visual.svg" />
    </span>
  </AppTooltip>
</template>

<style scoped>
.session-status-icon {
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
.session-status-icon__shape { display: inline-flex; width: 16px; height: 16px; transform-origin: center; }
.session-status-icon:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
.session-status-icon--starting { color: var(--status-info); }
.session-status-icon--running { color: var(--status-success); }
.session-status-icon--working, .session-status-icon--completed { color: var(--status-success); }
.session-status-icon--permission { color: var(--accent-gold-text); }
.session-status-icon--needs-user { color: var(--accent-gold-text); }
.session-status-icon--ended { color: var(--text-tertiary); }
.session-status-icon--stopped, .session-status-icon--closed, .session-status-icon--unknown { color: var(--text-tertiary); }
.session-status-icon--failed { color: var(--status-error); }
.session-status-icon--error { color: var(--status-error); }
.session-status-icon--starting .session-status-icon__shape { animation: session-status-spin 2.4s linear infinite; }
.session-status-icon--confirming .session-status-icon__shape { animation: session-status-breathe 2.4s ease-in-out infinite; }
.session-status-icon--needs-user .session-status-icon__shape { animation: session-status-attention 450ms ease-out 1; }
.session-status-icon--working .session-status-icon__shape,
.session-status-icon--permission .session-status-icon__shape { animation: session-status-pulse 1.8s ease-in-out infinite; }
@keyframes session-status-spin { to { transform: rotate(360deg); } }
@keyframes session-status-breathe { 50% { opacity: .75; } }
@keyframes session-status-attention { 50% { transform: scale(1.12); } }
@keyframes session-status-pulse { 50% { opacity: .55; transform: scale(.92); } }
@media (prefers-reduced-motion: reduce) {
  .session-status-icon .session-status-icon__shape { animation: none; }
}
</style>
