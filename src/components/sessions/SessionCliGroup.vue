<script setup lang="ts">
import { computed, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import SessionList from './SessionList.vue'
import { summarizeSessionStatus } from '@/utils/sessionStatusBadge'
import type {
  SessionMenuAction, SessionPrimaryAction, UnifiedCliKind, UnifiedSession,
} from '@/types/unifiedSession'

const props = withDefaults(defineProps<{
  cli: UnifiedCliKind
  sessions: UnifiedSession[]
  /** false 时整个组头不渲染（单 CLI 项目零视觉变化）。 */
  showHeader: boolean
  /** 搜索或本组改名期间强制展开并禁用开关。 */
  toggleDisabled: boolean
  /** 用户的收起意图；是否真的隐藏还要与 showHeader / toggleDisabled 相与。 */
  collapsed: boolean
  selectedId?: string | null
  surfaceActive?: boolean
}>(), { selectedId: null, surfaceActive: true })
const emit = defineEmits<{
  toggle: []
  activate: [id: string]
  'primary-action': [id: string, action: SessionPrimaryAction]
  'menu-action': [id: string, action: SessionMenuAction]
  'rename-commit': [id: string, title: string]
  'rename-cancel': [id: string]
}>()
const { t } = useI18n()
// 与 CliAppIcon / NewSessionMenu 同一约定：显示名是硬编码产品名，不进 i18n。
const CLI_NAMES: Record<UnifiedCliKind, string> = { claude: 'Claude Code', codex: 'Codex CLI' }
// aria-controls 必须落在常驻 DOM 的 SessionList 根元素上，所以 id 加在列表而非包裹层。
const listId = useId()
const hidden = computed(() => props.showHeader && !props.toggleDisabled && props.collapsed)
const badge = computed(() => summarizeSessionStatus(props.sessions))
</script>

<template>
  <div class="cli-group" :data-cli-group="cli">
    <!--
      组头是一条「居中分隔标签」，不是会话行：两侧细线把标题夹在中间，字号比会话标题更小、
      字重更轻，避免与会话行或项目名抢层级。左侧箭头复用项目行的折叠语义。
    -->
    <AppButton v-if="showHeader" class="cli-group-header" variant="ghost" size="compact"
      :data-cli-group-header="cli" :aria-expanded="!hidden" :aria-controls="listId" :disabled="toggleDisabled"
      @click="emit('toggle')">
      <span class="cli-group-rule" aria-hidden="true" />
      <span class="cli-group-mark">
        <svg class="cli-group-chevron" :class="{ expanded: !hidden }" aria-hidden="true" focusable="false"
          width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3"><polyline points="9 6 15 12 9 18" /></svg>
        <span class="cli-group-name">{{ CLI_NAMES[cli] }}</span>
        <!-- 收起时用状态徽章取代会话数：两者都是「这个组还有多少东西」，
             同时显示会出现两个相邻的相同数字。 -->
        <span v-if="!hidden || !badge" class="cli-group-count" aria-hidden="true">{{ sessions.length }}</span>
        <span v-else class="cli-group-attention" :class="`cli-group-attention--${badge.kind}`"
          data-cli-group-attention :data-status-kind="badge.kind" role="img"
          :aria-label="t(badge.labelKey, { count: badge.count })">{{ badge.count > 99 ? '99+' : badge.count }}</span>
      </span>
      <span class="cli-group-rule" aria-hidden="true" />
    </AppButton>
    <SessionList v-show="!hidden" :id="listId" :sessions="sessions" :selected-id="selectedId" :surface-active="surfaceActive"
      @activate="emit('activate', $event)" @primary-action="(id, action) => emit('primary-action', id, action)"
      @menu-action="(id, action) => emit('menu-action', id, action)"
      @rename-commit="(id, title) => emit('rename-commit', id, title)" @rename-cancel="emit('rename-cancel', $event)" />
  </div>
</template>

<style scoped>
.cli-group { min-width: 0; }
.cli-group-header { width: 100%; justify-content: center; gap: 8px; padding: 0 4px;
  border-radius: var(--radius-sm); color: var(--text-tertiary); }
.cli-group-header:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
.cli-group-rule { flex: 1 1 0; min-width: 12px; height: 1px; background: var(--border-color); }
.cli-group-mark { display: inline-flex; align-items: center; gap: 4px; flex: 0 0 auto; }
.cli-group-chevron { flex: 0 0 8px; transition: transform 0.15s ease; }
.cli-group-chevron.expanded { transform: rotate(90deg); }
/* 比会话标题（13px/500）更小更轻：组头是分节标签，不该压过它分隔的行。 */
.cli-group-name { font-size: 10px; font-weight: 600; letter-spacing: 0.08em; text-transform: uppercase; }
.cli-group-count { font-size: 10px; font-weight: 500; opacity: 0.7; }
.cli-group-attention { min-width: 16px; height: 16px; padding: 0 4px; border-radius: 8px;
  display: inline-flex; align-items: center; justify-content: center; font-size: 10px; font-weight: 600;
  color: var(--accent-gold-text); }
.cli-group-attention--error { color: var(--status-error); }
.cli-group-attention--completed, .cli-group-attention--running, .cli-group-attention--unread-reply { color: var(--status-success); }
.cli-group-header + .session-list { margin-top: 2px; }
@media (prefers-reduced-motion: reduce) { .cli-group-chevron { transition: none; } }
</style>
