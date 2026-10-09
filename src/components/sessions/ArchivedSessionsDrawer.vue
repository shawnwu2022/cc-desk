<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import AppDrawer from '@/components/ui/AppDrawer.vue'
import SessionList from './SessionList.vue'
import { normalizePath } from '@/utils/path'
import type { SessionMenuAction, SessionPrimaryAction, UnifiedProjectIdentity, UnifiedSession } from '@/types/unifiedSession'

const props = withDefaults(defineProps<{
  open: boolean
  sessions: UnifiedSession[]
  project?: UnifiedProjectIdentity | null
}>(), { project: null })
const emit = defineEmits<{
  'update:open': [open: boolean]
  'restore-request': [id: string]
  'menu-action': [id: string, action: SessionMenuAction]
  'rename-commit': [id: string, title: string]
  'rename-cancel': [id: string]
}>()
const { t } = useI18n()
const archived = computed(() => props.sessions.filter(session => session.archived
  && (!props.project || normalizePath(session.projectPath) === normalizePath(props.project.projectPath)))
  .sort((a, b) => b.lastActivityAt - a.lastActivityAt || a.id.localeCompare(b.id)))
function primaryAction(id: string, action: SessionPrimaryAction) {
  if (action === 'restore-archive' && archived.value.some(session => session.id === id)) emit('restore-request', id)
}
function menuAction(id: string, action: SessionMenuAction) {
  if (!archived.value.some(session => session.id === id)) return
  if (action === 'restore-archive') emit('restore-request', id)
  else emit('menu-action', id, action)
}
</script>

<template>
  <AppDrawer :open="open" :title="t('archivedSessions')" @update:open="emit('update:open', $event)">
    <div class="archived-sessions-content" role="tree" :aria-label="t('archivedSessions')">
      <SessionList v-if="archived.length" :sessions="archived" :menu-teleport="false" @primary-action="primaryAction" @menu-action="menuAction"
        @rename-commit="(id, title) => emit('rename-commit', id, title)" @rename-cancel="emit('rename-cancel', $event)" />
      <p v-else class="archived-empty">{{ t('noArchivedSessions') }}</p>
    </div>
  </AppDrawer>
</template>

<style scoped>
.archived-sessions-content { min-width: 0; }
.archived-empty { color: var(--text-secondary); font-size: 13px; text-align: center; padding: 16px 0; }
</style>
