<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useProjectsStateStore } from '@/stores/projectsState'
import { matchProjectQuery, projectBasename } from '@/utils/displayName'
import { normalizePath, sameProjectPath } from '@/utils/path'
import AppInput from '@/components/ui/AppInput.vue'
import AppButton from '@/components/ui/AppButton.vue'
import IconButton from '@/components/ui/IconButton.vue'
import PanelHeader from '../sidebar/PanelHeader.vue'
import ProjectNode from './ProjectNode.vue'
import ArchivedSessionsDrawer from './ArchivedSessionsDrawer.vue'
import type {
  ProjectActionRequest, SessionMenuAction, SessionPrimaryAction, SessionTreeConfirmationRequest,
  UnifiedProjectGroup, UnifiedProjectIdentity, UnifiedSession, NewSessionRequest,
} from '@/types/unifiedSession'

// Adapter setup, initial reads and runtime dispatch belong to the workspace container.
// Props permit that container to provide an already-loaded unified projection.
defineOptions({ inheritAttrs: false })
const props = withDefaults(defineProps<{
  active?: boolean
  projectGroups?: UnifiedProjectGroup[]
  archivedSessions?: UnifiedSession[]
  selectedId?: string | null
  currentProjectPath?: string | null
  loading?: boolean
}>(), { active: true })
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
const { t } = useI18n()
const store = useUnifiedSessionsStore()
const projects = useProjectsStateStore()
const searchQuery = ref('')
const expandedKeys = ref(new Set<string>())
const archivedOpen = ref(false)
const archivedProject = ref<UnifiedProjectIdentity | null>(null)
// A persistent tree is not an active surface after navigation/collapse. Its
// teleported modal must release focus, without resetting search or expansion.
watch(() => props.active, active => {
  if (!active) {
    archivedOpen.value = false
    archivedProject.value = null
  }
}, { flush: 'sync' })
const normalGroups = computed(() => props.projectGroups ?? store.projectGroups)
const archived = computed(() => (props.archivedSessions ?? store.sessions).filter(session => session.archived))
const selectedId = computed(() => props.selectedId === undefined ? store.activeSessionId : props.selectedId)
const loading = computed(() => props.loading ?? store.loading)
const stateReady = computed(() => props.projectGroups !== undefined || projects.loaded)
const stateError = computed(() => props.projectGroups === undefined && projects.error)
const searching = computed(() => !!searchQuery.value.trim())
const allGroups = computed<UnifiedProjectGroup[]>(() => {
  const groups = normalGroups.value.filter(group => !group.hidden).map(group => ({
    ...group, sessions: group.sessions.filter(session => !session.archived),
  }))
  const seen = new Set(normalGroups.value.map(group => normalizePath(group.projectPath)))
  const pinned = new Set(projects.pinnedProjects.map(normalizePath))
  // Task 5 intentionally omits all-archived projects. Keep one empty project shell
  // for their archive menu, plus a global drawer entry that survives search.
  for (const session of archived.value) {
    const key = normalizePath(session.projectPath)
    if (seen.has(key)) continue
    seen.add(key)
    groups.push({ projectKey: key, projectPath: session.projectPath,
      name: projects.displayNames.get(key) ?? projectBasename(session.projectPath),
      sessions: [], pinned: pinned.has(key), hidden: false, runningCount: 0,
      needsUserCount: 0, lastActivityAt: session.lastActivityAt })
  }
  return groups.sort((a, b) => Number(b.pinned) - Number(a.pinned)
    || b.lastActivityAt - a.lastActivityAt || a.name.localeCompare(b.name))
})
const displayedGroups = computed(() => {
  const query = searchQuery.value.trim().toLowerCase()
  if (!query) return allGroups.value
  return allGroups.value.flatMap(group => {
    if (matchProjectQuery(group.name, projectBasename(group.projectPath), group.projectPath, query)) return [group]
    const sessions = group.sessions.filter(session => session.title.toLowerCase().includes(query))
    return sessions.length ? [{ ...group, sessions }] : []
  })
})
function newSessionRequest(request: NewSessionRequest) {
  if (request.intent === 'claude' || request.intent === 'codex') expandedKeys.value = new Set([...expandedKeys.value, request.projectKey])
  emit('new-session-request', request)
}
function toggleExpand(key: string) {
  if (searching.value) return
  const next = new Set(expandedKeys.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  expandedKeys.value = next
  emit('toggle-expand', key)
}
function showArchived(project: UnifiedProjectIdentity | null = null) {
  if (!props.active) return
  archivedProject.value = project
  archivedOpen.value = true
}
function projectAction(request: ProjectActionRequest) {
  if (request.action === 'view-archive') showArchived({ projectKey: request.projectKey, projectPath: request.projectPath })
  emit('project-action', request)
}
function onKeydown(event: KeyboardEvent) {
  if (!props.active || event.defaultPrevented || archivedOpen.value) return
  if (event.key === 'Escape') emit('close')
}
onMounted(() => { window.addEventListener('keydown', onKeydown) })
onUnmounted(() => { window.removeEventListener('keydown', onKeydown) })
</script>

<template>
  <div class="sessions-panel">
    <PanelHeader :title="t('sessions')" @close="emit('close')">
      <template #actions>
        <IconButton :label="t('addProject')" @click="emit('add-project')">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14" /></svg>
        </IconButton>
        <IconButton data-view-archived :label="t('archivedSessions')" @click="showArchived()">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 8h16v12H4zM3 4h18v4H3zm6 8h6" /></svg>
        </IconButton>
        <IconButton :label="t('refreshSessions')" :disabled="loading" @click="emit('refresh')">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20 7v5h-5m5 0a8 8 0 1 0-2 6" /></svg>
        </IconButton>
      </template>
    </PanelHeader>
    <div class="search-box">
      <AppInput v-model="searchQuery" class="search-input" size="compact" :aria-label="t('searchSessions')" :placeholder="t('searchSessions')" />
      <IconButton v-if="searchQuery" :label="t('clearSearch')" @click="searchQuery = ''">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m6 6 12 12M18 6 6 18" /></svg>
      </IconButton>
    </div>
    <div class="panel-content" role="tree" :aria-label="t('sessions')">
      <div v-if="stateError" class="state-hint">
        <span>{{ t('projectsLoadFailed') }}</span>
        <AppButton size="compact" @click="emit('refresh')">{{ t('retry') }}</AppButton>
      </div>
      <div v-else-if="!stateReady" class="loading-indicator">{{ t('loading') }}</div>
      <template v-else>
        <ProjectNode v-for="group in displayedGroups" :key="group.projectKey" :project="group" :surface-active="active"
          :expanded="searching || expandedKeys.has(group.projectKey)" :disable-toggle="searching"
          :is-current="sameProjectPath(group.projectPath, currentProjectPath ?? '')" :selected-id="selectedId"
          @toggle-expand="toggleExpand" @new-session-request="newSessionRequest" @project-action="projectAction"
          @activate="emit('activate', $event)" @primary-action="(id, action) => emit('primary-action', id, action)"
          @menu-action="(id, action) => emit('menu-action', id, action)"
          @rename-commit="(id, title) => emit('rename-commit', id, title)" @rename-cancel="emit('rename-cancel', $event)"
          @confirmation-request="emit('confirmation-request', $event)" />
        <div v-if="displayedGroups.length === 0 && !loading" class="empty-hint">{{ t(searching ? 'noSessionsFound' : 'noProjectsYet') }}</div>
        <div v-if="loading" class="loading-indicator">{{ t('loading') }}</div>
      </template>
    </div>
    <ArchivedSessionsDrawer v-model:open="archivedOpen" :sessions="archived" :project="archivedProject"
      @restore-request="emit('restore-request', $event)" @menu-action="(id, action) => emit('menu-action', id, action)"
      @rename-commit="(id, title) => emit('rename-commit', id, title)" @rename-cancel="emit('rename-cancel', $event)" />
  </div>
</template>

<style scoped>
.sessions-panel { display: flex; flex-direction: column; height: 100%; min-width: 0; }
.search-box { display: flex; align-items: center; gap: 4px; padding: 8px 12px; }
.search-box :deep(.ui-field) { flex: 1; min-width: 0; }
.search-box :deep(.search-input) { width: 100%; }
.panel-content { flex: 1; overflow-y: auto; padding: 0 8px; min-height: 0; min-width: 0; }
.empty-hint, .loading-indicator { text-align: center; padding: 12px 8px; font-size: 12px; color: var(--text-secondary); }
.state-hint { display: flex; flex-direction: column; align-items: center; gap: 8px; padding: 12px; color: var(--text-secondary); }
</style>
