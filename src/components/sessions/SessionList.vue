<script setup lang="ts">
import SessionItem from './SessionItem.vue'
import type { SessionMenuAction, SessionMenuActionVisibility, SessionPrimaryAction, UnifiedSession } from '@/types/unifiedSession'

defineProps<{
  sessions: UnifiedSession[]
  selectedId?: string | null
  primaryActions?: Readonly<Record<string, SessionPrimaryAction | null>>
  menuActionVisibility?: SessionMenuActionVisibility
  menuTeleport?: boolean
}>()
const emit = defineEmits<{
  activate: [id: string]
  'primary-action': [id: string, action: SessionPrimaryAction]
  'menu-action': [id: string, action: SessionMenuAction]
  'rename-commit': [id: string, title: string]
  'rename-cancel': [id: string]
}>()
</script>

<template>
  <div class="session-list" role="group">
    <SessionItem v-for="session in sessions" :key="session.id" :session="session"
      :selected="session.id === selectedId" :primary-action="primaryActions?.[session.id]"
      :menu-action-visibility="menuActionVisibility" :menu-teleport="menuTeleport" @activate="emit('activate', $event)"
      @primary-action="(id, action) => emit('primary-action', id, action)"
      @menu-action="(id, action) => emit('menu-action', id, action)"
      @rename-commit="(id, title) => emit('rename-commit', id, title)"
      @rename-cancel="emit('rename-cancel', $event)" />
  </div>
</template>

<style scoped>
.session-list { display: flex; flex-direction: column; gap: 2px; }
</style>
