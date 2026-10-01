<script setup lang="ts">
import type {
  ProjectActionRequest, SessionMenuAction, SessionPrimaryAction, SessionTreeConfirmationRequest,
  UnifiedProjectGroup, UnifiedSession, NewSessionRequest,
} from '@/types/unifiedSession'
import SessionsPanel from '../sessions/SessionsPanel.vue'

defineOptions({ inheritAttrs: false })
withDefaults(defineProps<{
  visible?: boolean
  active?: boolean
  projectGroups?: UnifiedProjectGroup[]
  archivedSessions?: UnifiedSession[]
  selectedId?: string | null
  currentProjectPath?: string | null
  loading?: boolean
}>(), { visible: true, active: true })
const emit = defineEmits<{
  close: []
  'add-project': []
  refresh: []
  'toggle-expand': [projectKey: string]
  'new-session-request': [project: NewSessionRequest]
  'project-action': [request: ProjectActionRequest]
  activate: [id: string]
  'primary-action': [id: string, action: SessionPrimaryAction]
  'menu-action': [id: string, action: SessionMenuAction]
  'rename-commit': [id: string, title: string]
  'rename-cancel': [id: string]
  'confirmation-request': [request: SessionTreeConfirmationRequest]
  'restore-request': [id: string]
}>()
</script>

<template>
  <div v-show="visible" class="sidebar-panel">
    <SessionsPanel :active="active && visible" :project-groups="projectGroups" :archived-sessions="archivedSessions"
      :selected-id="selectedId" :current-project-path="currentProjectPath" :loading="loading"
      @close="emit('close')" @add-project="emit('add-project')" @refresh="emit('refresh')"
      @toggle-expand="emit('toggle-expand', $event)" @new-session-request="emit('new-session-request', $event)"
      @project-action="emit('project-action', $event)" @activate="emit('activate', $event)"
      @primary-action="(id, action) => emit('primary-action', id, action)"
      @menu-action="(id, action) => emit('menu-action', id, action)"
      @rename-commit="(id, title) => emit('rename-commit', id, title)" @rename-cancel="emit('rename-cancel', $event)"
      @confirmation-request="emit('confirmation-request', $event)" @restore-request="emit('restore-request', $event)" />
  </div>
</template>

<style scoped>
.sidebar-panel { display: flex; flex-direction: column; flex: 1; width: 100%; min-width: 0; min-height: 0; overflow: hidden; }
</style>
