<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppShell from '@/components/shell/AppShell.vue'
import SidebarPanel from '@/components/sidebar/SidebarPanel.vue'
import WorkspaceView from '@/components/workspace/WorkspaceView.vue'
import AppButton from '@/components/ui/AppButton.vue'
import EmptyState from '@/components/ui/EmptyState.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import { useShellStore, isCompatibilityEnabled, type WorkspaceRequest } from '@/stores/shell'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useAppStore } from '@/stores/app'
import { useSidebarStore, type SettingsSection } from '@/stores/sidebar'
import { applyThemeToDom } from '@/utils/theme'
import { projectBasename } from '@/utils/displayName'
import { sameProjectPath } from '@/utils/path'
import { onMenuSettings, onMenuShortcuts, onConfigFontSize, onOpenDirectory, onTerminalRestart } from '@/api/tauri'
import type { UnifiedProjectIdentity } from '@/types/unifiedSession'

// Removed in Task 21. A production build cannot enter this route by setting the
// flag alone; its modules and implicit Legacy startup are never mounted normally.
const compatibilityEnabled = isCompatibilityEnabled(import.meta.env.DEV, import.meta.env.VITE_CC_DESK_COMPATIBILITY)
const LegacyCompatibilityApp = compatibilityEnabled
  ? defineAsyncComponent(() => import('@/components/LegacyCompatibilityApp.vue')) : null
const SettingsView = defineAsyncComponent(() => import('@/components/settings/SettingsView.vue'))
const emit = defineEmits<{ 'workspace-request': [request: WorkspaceRequest] }>()
const { t } = useI18n()
const shell = useShellStore()
const sessions = useUnifiedSessionsStore()
const app = useAppStore()
const sidebar = useSidebarStore()
const configFailed = ref(false)
const settingsLoaded = ref(false)
const selectedProject = ref<UnifiedProjectIdentity | null>(null)
const project = computed<UnifiedProjectIdentity | null>(() => {
  const session = sessions.activeSession
  if (session) return { projectKey: session.projectKey, projectPath: session.projectPath }
  const selected = selectedProject.value ?? sessions.projectGroups[0]
  return selected ? { projectKey: selected.projectKey, projectPath: selected.projectPath } : null
})
const projectTitle = computed(() => project.value
  ? sessions.projectGroups.find(group => sameProjectPath(group.projectPath, project.value!.projectPath))?.name
    ?? projectBasename(project.value.projectPath) : '')
const windowTitle = computed(() => [projectTitle.value, sessions.activeSession?.title].filter(Boolean).join(' / ') || 'CC Desk')
watch(() => shell.section, section => { if (section === 'settings') settingsLoaded.value = true }, { immediate: true })
watch(() => app.theme, theme => { if (!compatibilityEnabled) applyThemeToDom(theme) }, { immediate: true })
// Old settings buttons may still emit this presentation intent. Do not mount
// SettingsOverlay or let its boolean become another routing source.
watch(() => sidebar.showSettings, open => {
  if (!compatibilityEnabled && open) { shell.navigate('settings'); sidebar.closeSettings() }
})
function request(action: WorkspaceRequest) {
  shell.requestWorkspaceAction(action)
  emit('workspace-request', action)
}
function selectProject(identity: UnifiedProjectIdentity) {
  selectedProject.value = { projectKey: identity.projectKey, projectPath: identity.projectPath }
  shell.navigate('workspace')
}
function openSettings(section: SettingsSection = 'appearance') {
  sidebar.activeSettingsSection = section
  shell.navigate('settings')
}
async function loadPreferences() {
  configFailed.value = false
  try { await app.loadAppConfig() }
  catch { configFailed.value = true }
}
function closeSessions() {
  if (shell.section === 'workspace' && shell.sidebarVisible) shell.sidebarVisible = false
}
function toggleProjects() { shell.navigate(shell.section === 'projects' ? 'workspace' : 'projects') }
function keydown(event: KeyboardEvent) {
  if (event.defaultPrevented) return
  if ((event.ctrlKey || event.metaKey) && event.key === ',') {
    event.preventDefault()
    if (shell.section === 'settings') shell.navigate('workspace')
    else openSettings()
  }
}
let disposed = false
const unlisteners: Array<() => void> = []
function retain(registration: Promise<() => void>) {
  void registration.then(unlisten => {
    if (disposed) unlisten()
    else unlisteners.push(unlisten)
  }).catch(() => { /* A missing OS menu bridge must not block application navigation. */ })
}
onMounted(() => {
  if (compatibilityEnabled) return
  void loadPreferences()
  // No Claude-only environment gate, history scan, startup decision or PTY launch.
  // Task 11 will configure the unified adapters and the single terminal host.
  retain(onMenuSettings(() => { if (!disposed) openSettings() }))
  retain(onMenuShortcuts(() => { if (!disposed) openSettings('shortcuts') }))
  retain(onConfigFontSize(size => { if (!disposed) app.setFontSize(size) }))
  retain(onOpenDirectory(projectPath => {
    if (disposed) return
    shell.navigate('projects')
    request({ kind: 'open-project', projectPath })
  }))
  retain(onTerminalRestart(() => {
    if (disposed || !sessions.activeSessionId) return
    shell.navigate('workspace')
    request({ kind: 'menu-action', sessionId: sessions.activeSessionId, action: 'restart' })
  }))
  window.addEventListener('app:toggleHome', toggleProjects)
  window.addEventListener('keydown', keydown)
})
onUnmounted(() => {
  disposed = true
  unlisteners.splice(0).forEach(unlisten => unlisten())
  window.removeEventListener('app:toggleHome', toggleProjects)
  window.removeEventListener('keydown', keydown)
})
</script>

<template>
  <LegacyCompatibilityApp v-if="compatibilityEnabled" />
  <AppShell v-else :title="windowTitle">
    <template #sidebar>
      <SidebarPanel :active="shell.section === 'workspace' && shell.sidebarVisible" :project-groups="sessions.projectGroups" :archived-sessions="sessions.sessions"
        :selected-id="sessions.activeSessionId" :current-project-path="project?.projectPath" :loading="sessions.loading"
        @close="closeSessions" @add-project="request({ kind: 'add-project' })"
        @refresh="request({ kind: 'refresh' })" @new-session-request="request({ kind: 'new-session', project: $event })"
        @project-action="request({ kind: 'project-action', request: $event })"
        @activate="request({ kind: 'activate', sessionId: $event })"
        @primary-action="(id, action) => request({ kind: 'primary-action', sessionId: id, action })"
        @menu-action="(id, action) => request({ kind: 'menu-action', sessionId: id, action })"
        @rename-commit="(id, title) => request({ kind: 'rename', sessionId: id, title })"
        @rename-cancel="request({ kind: 'rename-cancel', sessionId: $event })"
        @confirmation-request="request({ kind: 'confirmation', request: $event })"
        @restore-request="request({ kind: 'restore-archive', sessionId: $event })" />
    </template>
    <InlineNotice v-if="configFailed" kind="warning" :message="t('workspaceConfigFailed')"
      :action-label="t('retry')" @action="loadPreferences" />
    <WorkspaceView v-show="shell.section === 'workspace'" :project="project" :project-title="projectTitle"
      :active-session="sessions.activeSession" :request-pending="!!shell.pendingRequest"
      @add-project="request({ kind: 'add-project' })" @new-session-request="request({ kind: 'new-session', project: $event })" />
    <!-- Task 15 replaces this content-only project landing, never the global shell. -->
    <section v-show="shell.section === 'projects'" class="projects-content" :aria-label="t('projects')">
      <h1>{{ t('projects') }}</h1>
      <AppButton size="compact" @click="request({ kind: 'add-project' })">{{ t('addProject') }}</AppButton>
      <InlineNotice v-if="shell.pendingRequest" :message="t('workspaceActionPending')" />
      <EmptyState v-if="!sessions.projectGroups.length" :title="t('noProjectsYet')" :description="t('workspaceWelcomeHint')"
        :action-label="t('addProject')" @action="request({ kind: 'add-project' })" />
      <AppButton v-for="group in sessions.projectGroups" :key="group.projectKey" class="project-entry"
        @click="selectProject(group)">{{ group.name }}</AppButton>
    </section>
    <SettingsView v-if="settingsLoaded" v-show="shell.section === 'settings'" @close="shell.navigate('workspace')" />
    <template #context>
      <InlineNotice :message="t('contextResourcesHint')" />
    </template>
  </AppShell>
</template>

<style scoped>
.projects-content { display: flex; flex-direction: column; align-items: flex-start; gap: 12px; flex: 1; min-width: 0; min-height: 0; overflow-y: auto; overflow-x: hidden; padding: 20px; }
.projects-content h1 { font-size: 18px; color: var(--text-primary); }
.project-entry { max-width: 100%; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
</style>
