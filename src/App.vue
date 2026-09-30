<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppShell from '@/components/shell/AppShell.vue'
import SidebarPanel from '@/components/sidebar/SidebarPanel.vue'
import WorkspaceView from '@/components/workspace/WorkspaceView.vue'
import ProjectResourcesDrawer from '@/components/workspace/ProjectResourcesDrawer.vue'
import { useProjectResourcesStore } from '@/stores/projectResources'
import NewSessionMenu from '@/components/sessions/NewSessionMenu.vue'
import ResumeSessionDialog from '@/components/sessions/ResumeSessionDialog.vue'
import NewSessionDialog from '@/components/sessions/NewSessionDialog.vue'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import UnifiedTerminalHost from '@/components/workspace/UnifiedTerminalHost.vue'
import { useUnifiedWorkspaceRuntime } from '@/composables/useUnifiedWorkspaceRuntime'
import type { UnifiedTerminalHostPort } from '@/terminal/unifiedTerminalHost'
import ProjectsView from '@/components/projects/ProjectsView.vue'
import ProjectManagementDialogs from '@/components/projects/ProjectManagementDialogs.vue'
import { useProjectManagementStore } from '@/stores/projectManagement'
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
import type { NewSessionRequest, UnifiedProjectIdentity } from '@/types/unifiedSession'

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
const resources = useProjectResourcesStore()
watch(() => !compatibilityEnabled && shell.section === 'workspace' && shell.drawerVisible, resources.setActive, { immediate: true, flush: 'sync' })
const newSessionDraft = useNewSessionDraftStore()
const newMenuAnchor = ref({ x: 320, y: 64 })
watch(() => [shell.section, newSessionDraft.chooserVisible], ([section]) => {
  if (section !== 'workspace') newSessionDraft.chooserVisible = false
}, { flush: 'sync' })
const app = useAppStore()
const sidebar = useSidebarStore()
const management = useProjectManagementStore()
const terminalHost = ref<UnifiedTerminalHostPort | null>(null)
const runtime = useUnifiedWorkspaceRuntime(terminalHost, !compatibilityEnabled)
const configFailed = ref(false)
const settingsLoaded = ref(false)
let navigationVersion = 0
const selectedProject = ref<UnifiedProjectIdentity | null>(null)
const project = computed<UnifiedProjectIdentity | null>(() => {
  const session = sessions.activeSession
  if (session) return { projectKey: session.projectKey, projectPath: session.projectPath }
  const selected = selectedProject.value ?? management.visibleGroups[0]
  return selected ? { projectKey: selected.projectKey, projectPath: selected.projectPath } : null
})
watch(() => management.groups, groups => {
  if (selectedProject.value && !sessions.activeSession && !groups.some(group => !group.hidden && sameProjectPath(group.projectPath, selectedProject.value!.projectPath))) selectedProject.value = null
}, { deep: true })
const visibleArchiveSessions = computed(() => sessions.sessions.filter(row => !app.isHidden(row.projectPath) || management.openCount(row.projectPath) > 0))
const projectTitle = computed(() => project.value
  ? management.groups.find(group => sameProjectPath(group.projectPath, project.value!.projectPath))?.name
    ?? projectBasename(project.value.projectPath) : '')
const windowTitle = computed(() => [projectTitle.value, sessions.activeSession?.title].filter(Boolean).join(' / ') || 'CC Desk')
watch(() => shell.section, section => { ++navigationVersion; management.closeDialog(); if (section === 'settings') settingsLoaded.value = true }, { immediate: true })
watch(() => app.theme, theme => { if (!compatibilityEnabled) applyThemeToDom(theme) }, { immediate: true })
// Old settings buttons may still emit this presentation intent. Do not mount
// SettingsOverlay or let its boolean become another routing source.
watch(() => sidebar.showSettings, open => {
  if (!compatibilityEnabled && open) { shell.navigate('settings'); sidebar.closeSettings() }
})
function request(action: WorkspaceRequest) {
  if (action.kind === 'new-session' && !action.project.intent) {
    const opener = document.activeElement
    const rect = opener instanceof HTMLElement && opener !== document.body ? opener.getBoundingClientRect() : null
    newMenuAnchor.value = { x: rect?.left ?? shell.sidebarWidth + 12, y: rect ? rect.bottom + 4 : 64 }
  }
  shell.requestWorkspaceAction(action)
  emit('workspace-request', action)
  if (action.kind === 'add-project' || action.kind === 'open-project' || action.kind === 'project-action') {
    const sequence = shell.requestSequence
    const navigation = navigationVersion
    void (async () => {
      if (action.kind === 'project-action') await management.action(action.request)
      else {
        const identity = await management.add(action.kind === 'open-project' ? action.projectPath : undefined)
        // A later navigation/request owns selection; an older add may persist but cannot steal it.
        if (identity && shell.requestSequence === sequence && navigationVersion === navigation) selectProject(identity)
      }
      shell.clearWorkspaceRequest(sequence)
    })()
  }
}
function chooseNewSession(intent: NonNullable<NewSessionRequest['intent']>) {
  if (shell.section === 'workspace' && newSessionDraft.project) request({ kind: 'new-session', project: { ...newSessionDraft.project, intent } })
}
function selectProject(identity: UnifiedProjectIdentity) {
  sessions.selectProjectContext(identity.projectPath)
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
  void management.refresh()
  // Runtime bootstrap reads each source independently; no implicit CLI launch.
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
  resources.setActive(false)
  unlisteners.splice(0).forEach(unlisten => unlisten())
  window.removeEventListener('app:toggleHome', toggleProjects)
  window.removeEventListener('keydown', keydown)
})
</script>

<template>
  <LegacyCompatibilityApp v-if="compatibilityEnabled" />
  <AppShell v-else :title="windowTitle">
    <template #sidebar>
      <SidebarPanel :active="shell.section === 'workspace' && shell.sidebarVisible" :project-groups="management.visibleGroups" :archived-sessions="visibleArchiveSessions"
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
      :active-session="sessions.activeSession" :request-pending="!!shell.pendingRequest && shell.pendingRequest.kind !== 'restore-session'" :cli-availability="runtime.cliAvailability.value"
      @add-project="request({ kind: 'add-project' })" @new-session-request="request({ kind: 'new-session', project: $event })">
      <template #terminal>
        <InlineNotice v-if="runtime.error.value" kind="warning" :message="t(runtime.error.value)"
          :action-label="t('retry')" @action="request({ kind: 'refresh' })" />
        <InlineNotice v-if="sessions.activeSession?.safeErrorCode === 'NEW_SESSION_PREPARATION_FAILED'" kind="warning"
          :message="t('newSessionPreparationFailed')" :action-label="t('newSessionMoreOptions')"
          @action="newSessionDraft.open(sessions.activeSession!, sessions.activeSession!.cli)" />
        <UnifiedTerminalHost ref="terminalHost" :sessions="runtime.openSessions.value" :active-session-id="sessions.activeSessionId"
          :visible="shell.section === 'workspace'">
          <EmptyState :title="t('workspaceWelcome')" :description="t('workspaceWelcomeHint')"
            :action-label="t(project ? 'newSession' : 'addProject')"
            @action="project ? request({ kind: 'new-session', project }) : request({ kind: 'add-project' })" />
        </UnifiedTerminalHost>
      </template>
    </WorkspaceView>
    <NewSessionMenu v-model:open="newSessionDraft.chooserVisible" :active="shell.section === 'workspace'"
      :anchor="newMenuAnchor" :availability="newSessionDraft.cliAvailability" @select="chooseNewSession" />
    <NewSessionDialog :active="shell.section === 'workspace'" @create="request({ kind: 'create-session', input: $event })"
      @restore="request({ kind: 'restore-session', ...$event })" />
    <ResumeSessionDialog :active="shell.section === 'workspace'" />
    <InlineNotice v-if="management.error" kind="warning" :message="t(management.error)" :action-label="t('retry')" @action="management.refresh" />
    <ProjectsView v-show="shell.section === 'projects'" :active="shell.section === 'projects'"
      @add-project="request({ kind: 'add-project' })" @open="selectProject"
      @new-session-request="selectProject($event); request({ kind: 'new-session', project: $event })"
      @project-action="request({ kind: 'project-action', request: $event })" />
    <ProjectManagementDialogs />
    <SettingsView v-if="settingsLoaded" v-show="shell.section === 'settings'" @close="shell.navigate('workspace')" />
    <template #context>
      <ProjectResourcesDrawer />
    </template>
  </AppShell>
</template>
