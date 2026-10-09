<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useProjectManagementStore } from '@/stores/projectManagement'
import { matchProjectQuery, projectBasename } from '@/utils/displayName'
import AppButton from '@/components/ui/AppButton.vue'
import AppInput from '@/components/ui/AppInput.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import EmptyState from '@/components/ui/EmptyState.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import ProjectRow from './ProjectRow.vue'
import type { UnifiedProjectIdentity, ProjectActionRequest, NewSessionRequest } from '@/types/unifiedSession'
const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })
const emit = defineEmits<{ 'add-project': []; open: [project: UnifiedProjectIdentity]; 'new-session-request': [project: NewSessionRequest]; 'project-action': [request: ProjectActionRequest] }>()
const { t } = useI18n()
const management = useProjectManagementStore()
const query = ref('')
const showHidden = ref(false)
const sort = ref('recent')
const rows = computed(() => management.groups.filter(project => (showHidden.value || !project.hidden)
  && matchProjectQuery(project.name, projectBasename(project.projectPath), project.projectPath, query.value.trim().toLowerCase()))
  .sort((a, b) => Number(b.pinned) - Number(a.pinned) || (sort.value === 'name' ? a.name.localeCompare(b.name) : b.lastActivityAt - a.lastActivityAt)
    || a.name.localeCompare(b.name) || a.projectKey.localeCompare(b.projectKey)))
function action(project: UnifiedProjectIdentity, action: ProjectActionRequest['action'] | 'hide' | 'show') {
  if (action === 'hide' || action === 'show') void management.setHidden(project, action === 'hide')
  else emit('project-action', { ...project, action })
}
</script>
<template>
  <section class="projects-view" :aria-label="t('projectManagement')">
    <header class="projects-header"><h1>{{ t('projects') }}</h1><AppButton data-add-managed-project size="compact" :disabled="management.busy" @click="emit('add-project')">{{ t('addProject') }}</AppButton></header>
    <div class="projects-toolbar">
      <AppInput v-model="query" data-project-search size="compact" :aria-label="t('searchProjects')" :placeholder="t('searchProjects')" />
      <AppSelect v-model="sort" data-project-sort size="compact" :aria-label="t('projectSort')" :options="[{ value: 'recent', label: t('projectSortRecent') }, { value: 'name', label: t('projectSortName') }]" />
      <label class="show-hidden"><input v-model="showHidden" data-show-hidden type="checkbox">{{ t('projectShowHidden') }}</label>
    </div>
    <InlineNotice v-if="management.loading" :message="t('loading')" />
    <ul class="projects-list" :aria-label="t('projects')">
      <ProjectRow v-for="project in rows" :key="project.projectKey" :project="project" :active="props.active" :busy="management.busy" :open-count="management.openCount(project.projectPath)"
        @open="emit('open', project)" @new-session="emit('new-session-request', project)" @action="action(project, $event)" />
    </ul>
    <EmptyState v-if="!rows.length && !management.loading" :title="t(query ? 'noMatchingProjects' : 'noProjectsYet')" :description="t('projectsEmptyHint')" :action-label="t('addProject')" @action="emit('add-project')" />
  </section>
</template>
<style scoped>
.projects-view { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; overflow: hidden; padding: 16px; gap: 12px; }
.projects-header { display: flex; justify-content: space-between; align-items: center; gap: 12px; min-width: 0; }
.projects-header h1 { font-size: 18px; color: var(--text-primary); }
.projects-toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-width: 0; }
.projects-toolbar > :first-child { flex: 1 1 160px; min-width: 0; }
.show-hidden { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text-secondary); white-space: nowrap; }
.projects-list { flex: 1; min-height: 0; min-width: 0; overflow-y: auto; overflow-x: hidden; list-style: none; margin: 0; padding: 0; }
</style>
