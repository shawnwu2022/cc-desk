<script setup lang="ts">
import { computed, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import CliAppIcon from './CliAppIcon.vue'
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
    <AppButton v-if="showHeader" class="cli-group-header" variant="ghost" size="compact"
      :data-cli-group-header="cli" :aria-expanded="!hidden" :aria-controls="listId" :disabled="toggleDisabled"
      @click="emit('toggle')">
      <CliAppIcon :cli="cli" decorative />
      <span class="cli-group-name">{{ CLI_NAMES[cli] }}</span>
      <span class="cli-group-count">{{ sessions.length }}</span>
      <span v-if="hidden && badge" class="cli-group-attention" :class="`cli-group-attention--${badge.kind}`"
        data-cli-group-attention :data-status-kind="badge.kind" role="img"
        :aria-label="t(badge.labelKey, { count: badge.count })">{{ badge.count > 99 ? '99+' : badge.count }}</span>
    </AppButton>
    <SessionList v-show="!hidden" :id="listId" :sessions="sessions" :selected-id="selectedId" :surface-active="surfaceActive"
      @activate="emit('activate', $event)" @primary-action="(id, action) => emit('primary-action', id, action)"
      @menu-action="(id, action) => emit('menu-action', id, action)"
      @rename-commit="(id, title) => emit('rename-commit', id, title)" @rename-cancel="emit('rename-cancel', $event)" />
  </div>
</template>

<style scoped>
.cli-group { min-width: 0; }
.cli-group-header { width: 100%; justify-content: flex-start; gap: 6px; padding: 0 8px; }
.cli-group-header:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
.cli-group-name { color: var(--text-secondary); font-size: 12px; font-weight: 500; }
.cli-group-count { color: var(--text-tertiary); font-size: 11px; }
.cli-group-attention { margin-left: auto; min-width: 16px; height: 16px; padding: 0 4px; border-radius: 8px;
  display: inline-flex; align-items: center; justify-content: center; font-size: 10px; font-weight: 600; color: var(--accent-gold-text); }
.cli-group-attention--error { color: var(--status-error); }
.cli-group-attention--completed, .cli-group-attention--running, .cli-group-attention--unread-reply { color: var(--status-success); }
.cli-group-header + .session-list { margin-top: 2px; }
</style>
