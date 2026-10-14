<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppTooltip from '@/components/ui/AppTooltip.vue'
import CliAppIcon from './CliAppIcon.vue'
import SessionStatusIcon from './SessionStatusIcon.vue'
import { sessionVisualLabelKey } from '@/utils/sessionPresentation'
import type { SessionActivityState, SessionVisualState, UnifiedCliKind } from '@/types/unifiedSession'

const props = defineProps<{ cli: UnifiedCliKind; state: SessionVisualState; activityState?: SessionActivityState; archived?: boolean; notice?: { labelKey: string; unread: boolean } | null; receiptId?: string }>()
const { t } = useI18n()
const statusLabel = computed(() => t(sessionVisualLabelKey(props.state, props.activityState, props.archived)))
const label = computed(() => [props.cli === 'claude' ? 'Claude Code' : 'Codex CLI', statusLabel.value, props.notice && t(props.notice.labelKey)].filter(Boolean).join('; '))
const badge = computed(() => {
  // 关闭仅由应用图标本身的明暗区分；回执仍保留在可访问名称和 Tooltip 中。
  if (props.state === 'closed') return null
  if (props.archived) return 'archived'
  // 未读是历史发生事实；已知的新活动、权限、错误及启动状态优先。
  if (props.notice?.unread && ['unknown', 'running', 'closed', 'stopped', 'ended', 'completed'].includes(props.state)) return 'completed'
  return ['running', 'closed'].includes(props.state) ? null : props.state
})
const completionEntry = ref(false)
watch(() => [props.state, props.receiptId, props.archived] as const, (next, previous) => {
  completionEntry.value = false
  if (props.archived || window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) return
  if ((next[0] === 'completed' && previous[0] !== 'completed') || (next[1] && next[1] !== previous[1])) completionEntry.value = badge.value === 'completed'
})
function finishEntry(event: AnimationEvent) {
  if (event.animationName === 'session-app-completion') completionEntry.value = false
}
function preserveNoticeInspection(event: MouseEvent) {
  if (props.notice) event.stopPropagation()
}
</script>

<template>
  <AppTooltip :text="label">
    <span class="session-app-icon" :class="[`session-app-icon--${cli}`, { 'session-app-icon--closed': ['closed', 'stopped', 'ended'].includes(state) || archived, 'session-app-icon--working': state === 'working' && !archived, 'session-app-icon--completion-entry': completionEntry }]"
      role="img" :aria-label="label" tabindex="0" :data-completion-entry="completionEntry || undefined"
      :data-native-observation-notice="notice ? '' : undefined" :data-unread="notice ? String(notice.unread) : undefined"
      @animationend="finishEntry" @animationcancel="finishEntry" @click="preserveNoticeInspection" @dblclick="preserveNoticeInspection">
      <CliAppIcon :cli="cli" decorative class="session-app-icon__app" />
      <span v-if="badge" class="session-app-icon__badge" :data-session-badge="badge" aria-hidden="true">
        <SessionStatusIcon :state="badge === 'archived' ? state : badge" :activity-state="badge === 'completed' ? undefined : activityState" :archived="archived" decorative />
      </span>
    </span>
  </AppTooltip>
</template>

<style scoped>
.session-app-icon { display: inline-flex; align-items: center; justify-content: center; position: relative; width: 20px; height: 20px; flex: 0 0 20px; border-radius: var(--radius-sm); }
.session-app-icon:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
.session-app-icon__app { display: inline-flex; }
.session-app-icon--closed .session-app-icon__app { filter: grayscale(1); opacity: .72; }
/* 黑白 Codex 图形不受灰度影响，单独拉开明暗，保留品牌形状与主题反色。 */
.session-app-icon--codex.session-app-icon--closed .session-app-icon__app { opacity: var(--session-codex-closed-opacity); }
.session-app-icon__badge { display: flex; position: absolute; right: -1px; bottom: -1px; width: 10px; height: 10px; background: var(--bg-primary); border-radius: var(--radius-lg); }
.session-app-icon__badge :deep(.session-status-icon), .session-app-icon__badge :deep(.session-status-icon__shape), .session-app-icon__badge :deep(svg) { width: 10px; height: 10px; flex-basis: 10px; }
.session-app-icon__badge :deep(.session-status-icon) { --session-status-info: var(--status-info); --session-status-success: var(--status-success); --session-status-error: var(--status-error); }
.session-app-icon__badge :deep(.session-status-icon--permission) { color: var(--accent-gold-text); }
.session-app-icon__badge :deep(.session-status-icon--needs-user) { color: var(--status-info); }
.session-app-icon__badge :deep(.session-status-icon__shape), .session-app-icon__badge :deep([data-thinking-dot]), .session-app-icon__badge :deep([data-completion-mark]) { animation: none; }
.session-app-icon--working .session-app-icon__app { animation: session-app-breathe 2.6s ease-in-out infinite; }
.session-app-icon--completion-entry .session-app-icon__app { animation: session-app-completion 650ms ease-out 1; }
@keyframes session-app-breathe { 50% { opacity: .78; } }
@keyframes session-app-completion { from { opacity: .72; } to { opacity: 1; } }
@media (prefers-reduced-motion: reduce) {
  .session-app-icon .session-app-icon__app { animation: none; }
}
</style>
