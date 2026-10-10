<script setup lang="ts">
import { computed, provide, defineAsyncComponent, onMounted, onUnmounted, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppShell from '@/components/shell/AppShell.vue'
import SidebarPanel from '@/components/sidebar/SidebarPanel.vue'
import WorkspaceView from '@/components/workspace/WorkspaceView.vue'
import WorkspaceSourceDetails from '@/components/workspace/WorkspaceSourceDetails.vue'
import ProjectResourcesDrawer from '@/components/workspace/ProjectResourcesDrawer.vue'
import { useProjectResourcesStore } from '@/stores/projectResources'
import ProjectConfirmDialog from '@/components/dialogs/ProjectConfirmDialog.vue'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import SessionDiagnosticsDialog from '@/components/sessions/SessionDiagnosticsDialog.vue'
import SessionConfirmDialog from '@/components/dialogs/SessionConfirmDialog.vue'
import AppToastHost from '@/components/ui/AppToastHost.vue'
import ErrorDetails from '@/components/ui/ErrorDetails.vue'
import NewSessionMenu from '@/components/sessions/NewSessionMenu.vue'
import ResumeSessionDialog from '@/components/sessions/ResumeSessionDialog.vue'
import NewSessionDialog from '@/components/sessions/NewSessionDialog.vue'
import LaunchProgramDiscovery from '@/components/sessions/LaunchProgramDiscovery.vue'
import LaunchConfigurationEditor from '@/components/settings/LaunchConfigurationEditor.vue'
import type { ConfirmedLaunchProgram } from '@/types/profile'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import UnifiedTerminalHost from '@/components/workspace/UnifiedTerminalHost.vue'
import { SESSION_INTERACTION_OWNER } from '@/session/sessionInteraction'
import { useAppShortcuts } from '@/composables/useAppShortcuts'
import { APP_RENAME_SHORTCUT, type AppShortcutAction } from '@/config/appShortcuts'
import { useUnifiedWorkspaceRuntime } from '@/composables/useUnifiedWorkspaceRuntime'
import { useLaunchConfigurationEditor } from '@/composables/useLaunchConfigurationEditor'
import type { UnifiedTerminalHostPort } from '@/terminal/unifiedTerminalHost'
import ProjectsView from '@/components/projects/ProjectsView.vue'
import ProjectManagementDialogs from '@/components/projects/ProjectManagementDialogs.vue'
import { useProjectManagementStore } from '@/stores/projectManagement'
import EmptyState from '@/components/ui/EmptyState.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import IconButton from '@/components/ui/IconButton.vue'
import AppButton from '@/components/ui/AppButton.vue'
import AppDrawer from '@/components/ui/AppDrawer.vue'
import { useShellStore, type WorkspaceRequest } from '@/stores/shell'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useAppStore } from '@/stores/app'
import { useSidebarStore, type SettingsSection } from '@/stores/sidebar'
import { applyThemeToDom } from '@/utils/theme'
import { projectBasename } from '@/utils/displayName'
import { mapSafeUserError } from '@/utils/userError'
import { sameProjectPath } from '@/utils/path'
import { onMenuSettings, onMenuShortcuts, onConfigFontSize, onOpenDirectory, onTerminalRestart } from '@/api/tauri'
import type { NewSessionRequest, UnifiedProjectIdentity } from '@/types/unifiedSession'

const SettingsView = defineAsyncComponent(() => import('@/components/settings/SettingsView.vue'))
const emit = defineEmits<{ 'workspace-request': [request: WorkspaceRequest] }>()
const { t } = useI18n()
const shell = useShellStore()
const sessions = useUnifiedSessionsStore()
provide(SESSION_INTERACTION_OWNER, id => {
  try { return sessions.captureSessionOwnership(id) }
  catch { return () => false }
})
const resources = useProjectResourcesStore()
watch(() => shell.section === 'workspace' && shell.drawerVisible, resources.setActive, { immediate: true, flush: 'sync' })
const newSessionDraft = useNewSessionDraftStore()
const newMenuAnchor = ref({ x: 320, y: 64 })
watch(() => [shell.section, newSessionDraft.chooserVisible], ([section]) => {
  if (section !== 'workspace') newSessionDraft.chooserVisible = false
}, { flush: 'sync' })
const app = useAppStore()
provide(APP_RENAME_SHORTCUT, computed(() => app.shortcutBindings.rename))
const sidebar = useSidebarStore()
const management = useProjectManagementStore()
const configurations = useCliProfilesStore()
const { editor: configurationEditor, opening: configurationEditorOpening, error: configurationEditorError,
  open: openConfigurationEditor, close: closeConfigurationEditor } = useLaunchConfigurationEditor(() => shell.section === 'workspace')
watch(() => [shell.section, shell.navigationSequence, shell.requestSequence, sessions.activeSessionId], closeConfigurationEditor, { flush: 'sync' })
function editPreparationConfiguration() {
  const session = sessions.activeSession
  const profile = session?.launchConfigId ? configurations.profile(session.launchConfigId) : undefined
  if (shell.section !== 'workspace' || session?.safeErrorCode !== 'LAUNCH_CONFIGURATION_REQUIRED') return
  if (profile && profile.cli === session.cli) {
    const owns = sessions.captureSessionOwnership(session.id), selected = sessions.captureSelectionOwnership()
    void openConfigurationEditor({ kind: 'edit', profileId: profile.id }, () => owns() && selected()
      && sessions.activeSessionId === session.id && sessions.activeSession?.safeErrorCode === 'LAUNCH_CONFIGURATION_REQUIRED'
      && sessions.activeSession.launchConfigId === profile.id && configurations.profile(profile.id)?.cli === session.cli)
  }
  else { sidebar.activeSettingsSection = 'launch-configurations'; shell.navigate('settings') }
}
function confirmDiscoveredProgram(confirmation: ConfirmedLaunchProgram) {
  const row = sessions.activeSession
  if (shell.section !== 'workspace' || row?.id !== confirmation.sessionId || row.preparationState !== 'failed'
    || row.preparationIssueCode !== 'PROGRAM_TRUST_REQUIRED' || !confirmation.canContinue()) return
  const pending = sessions.retryConfirmedCreation(confirmation.sessionId, confirmation.profileId, confirmation.profileRevision, confirmation.canContinue)
  const feedbackOwner = sessions.captureFeedbackOwner()
  void pending.catch(error => sessions.publishActionFailure(() => confirmation.canContinue() && feedbackOwner(), error))
}
const sessionSidebar = ref<InstanceType<typeof SidebarPanel> | null>(null)
const terminalHost = ref<UnifiedTerminalHostPort | null>(null)
const runtime = useUnifiedWorkspaceRuntime(terminalHost)
const sourceNoticeArea = ref<HTMLElement | null>(null)
const sourceDiagnosticsOpen = ref(false)
watch(() => [shell.section, shell.sidebarVisible], () => {
  if (shell.section !== 'workspace' || !shell.sidebarVisible) sourceDiagnosticsOpen.value = false
}, { flush: 'sync' })
function editSourceConfiguration(profileId: string) {
  const owner = runtime.sourceWarningConfigurations.value.find(row => row.profileId === profileId)
  if (shell.section !== 'workspace' || !owner || configurations.profile(profileId)?.revision !== owner.profileRevision) return
  sourceDiagnosticsOpen.value = false
  const selected = sessions.captureSelectionOwnership()
  void openConfigurationEditor({ kind: 'edit', profileId }, () => selected()
    && runtime.sourceWarningConfigurations.value.some(row => row.profileId === profileId
      && row.profileRevision === owner.profileRevision && row.warningKey === owner.warningKey))
}
async function dismissSourceNotice() {
  const restoreFocus = sourceNoticeArea.value?.contains(document.activeElement) ?? false
  runtime.dismissSourceNotice()
  await nextTick()
  if (restoreFocus && shell.section === 'workspace') {
    const target = shell.sidebarVisible ? '[data-history-source-entry]' : '[data-primary-section="workspace"]'
    document.querySelector<HTMLElement>(target)?.focus()
  }
}
const configFailed = ref(false)
const settingsLoaded = ref(false)
const startupNavigation = shell.navigationSequence
let startupApplied = false
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
watch(() => shell.section, section => {
  ++navigationVersion; management.closeDialog(); sessions.clearActionFeedback()
  if (section !== 'workspace') sessions.closeSessionConfirmation()
  if (section !== 'settings') configurations.closeDeleteConfirmation()
  if (section === 'settings') settingsLoaded.value = true
}, { immediate: true, flush: 'sync' })
watch(() => app.theme, applyThemeToDom, { immediate: true })
// Compatibility settings intents resolve to the same Settings section.
watch(() => sidebar.showSettings, open => {
  if (open) { shell.navigate('settings'); sidebar.closeSettings() }
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
function openSettings(section: SettingsSection = 'general') {
  sidebar.activeSettingsSection = section
  shell.navigate('settings')
}
async function loadPreferences() {
  configFailed.value = false
  try {
    await app.loadAppConfig()
    if (!startupApplied && shell.navigationSequence === startupNavigation && shell.requestSequence === 0 && shell.section === 'workspace') {
      shell.navigate(app.startupDestination)
    }
    startupApplied = true
  }
  catch { configFailed.value = true }
}
function closeSessions() {
  if (shell.section === 'workspace' && shell.sidebarVisible) shell.sidebarVisible = false
}
function shortcutAction(action: AppShortcutAction) {
  if (action === 'projects') { shell.navigate('workspace'); shell.sidebarVisible = true; void sessionSidebar.value?.focusSearch(); return }
  if (action === 'settings') {
    if (shell.section === 'settings') shell.navigate('workspace')
    else openSettings()
    return
  }
  if (action === 'new-session') {
    const selected = project.value
    shell.navigate(selected ? 'workspace' : 'projects')
    request(selected ? { kind: 'new-session', project: selected } : { kind: 'add-project' })
    return
  }
  const selected = sessions.activeSession
  if (!selected) return
  shell.navigate('workspace')
  if (action === 'close-session') request({ kind: 'menu-action', sessionId: selected.id, action: 'close' })
  else if (!sessions.isPreparingSession(selected.id)) { shell.sidebarVisible = true; sessions.beginRename(selected.id) }
}
const shortcuts = useAppShortcuts({ onAction: shortcutAction })

let disposed = false
const unlisteners: Array<() => void> = []
function retain(registration: Promise<() => void>) {
  void registration.then(unlisten => {
    if (disposed) unlisten()
    else unlisteners.push(unlisten)
  }).catch(() => { /* A missing OS menu bridge must not block application navigation. */ })
}
onMounted(() => {
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
  unlisteners.push(...shortcuts.setupShortcutListeners())
})
onUnmounted(() => {
  disposed = true
  resources.setActive(false)
  unlisteners.splice(0).forEach(unlisten => unlisten())
})
</script>

<template>
  <AppShell :title="windowTitle">
    <template #sidebar>
      <SidebarPanel ref="sessionSidebar" @select-project="selectProject" :active="shell.section === 'workspace' && shell.sidebarVisible" :project-groups="management.visibleGroups" :archived-sessions="visibleArchiveSessions"
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
        @restore-request="request({ kind: 'restore-archive', sessionId: $event })">
        <template #source-status>
          <AppButton v-if="runtime.sourceIssuesAvailable?.value" data-history-source-entry variant="ghost" size="compact"
            :inert="shell.section !== 'workspace' || !shell.sidebarVisible || undefined" @click="sourceDiagnosticsOpen = true">{{ t('historySourceIssues') }}</AppButton>
        </template>
      </SidebarPanel>
    </template>
    <InlineNotice v-if="configFailed" kind="warning" :message="t('workspaceConfigFailed')"
      :action-label="t('retry')" @action="loadPreferences" />
    <WorkspaceView v-show="shell.section === 'workspace'" :project="project" :project-title="projectTitle"
      :active-session="sessions.activeSession" :request-pending="!!shell.pendingRequest && shell.pendingRequest.kind !== 'restore-session'" :cli-availability="runtime.cliAvailability.value"
      @add-project="request({ kind: 'add-project' })" @new-session-request="request({ kind: 'new-session', project: $event })">
      <template #terminal>
        <EmptyState v-if="runtime.fatal?.value" data-workspace-fatal :title="t('workspaceLoadFailed')" :description="t('workspaceLoadFailedHint')" :action-label="t('retry')" @action="request({ kind: 'refresh' })" />
        <InlineNotice v-for="problem in runtime.cliProblems?.value ?? []" :key="problem.cli" data-cli-banner kind="warning" :message="t('cliFailureBanner', { cli: problem.cli === 'claude' ? 'Claude Code' : 'Codex CLI', reason: t(problem.messageKey) })" :action-label="t('refresh')" @action="request({ kind: 'refresh' })" />
        <InlineNotice v-if="sessions.actionFeedback && sessions.actionFeedback.detailCode !== 'LAUNCH_CONFIGURATION_REQUIRED'" data-action-feedback :kind="sessions.actionFeedback.severity" :message="t(sessions.actionFeedback.messageKey)" :action-label="sessions.actionFeedback.retryable ? t('retry') : t('refresh')" @action="runtime.retryAction?.()">
          <ErrorDetails :code="sessions.actionFeedback.detailCode" context="session" />
        </InlineNotice>
        <InlineNotice v-if="runtime.error.value && runtime.error.value !== 'workspaceRuntimePartial'" kind="warning" :message="t(runtime.error.value)"
          :action-label="t('retry')" @action="request({ kind: 'refresh' })" />
        <div v-if="!runtime.fatal?.value" ref="sourceNoticeArea" :inert="configurationEditorOpening || undefined">
          <InlineNotice v-if="runtime.sourceWarnings?.value.length && !runtime.sourceNoticeDismissed?.value" data-workspace-source-notice class="workspace-source-notice" kind="warning" :message="t('workspaceRuntimePartial')"
            :action-label="t('retry')" @action="request({ kind: 'refresh' })">
            <IconButton data-dismiss-source-notice :label="t('sourceWarningDismiss')" @click="dismissSourceNotice"><span>×</span></IconButton>
            <WorkspaceSourceDetails :warnings="runtime.sourceWarnings.value" :truncated="runtime.sourceWarningsTruncated.value"
              :configurations="runtime.sourceWarningConfigurations?.value ?? []" @configure="editSourceConfiguration" />
          </InlineNotice>
          <template v-if="runtime.historyMetadataPartial?.value && !runtime.historyNoticeDismissed?.value">
            <InlineNotice data-history-metadata-partial kind="info" :message="t('workspaceHistoryMetadataPartial')">
              <IconButton data-dismiss-history-notice :label="t('sourceWarningDismiss')" @click="dismissSourceNotice"><span>×</span></IconButton>
            </InlineNotice>
            <WorkspaceSourceDetails v-if="runtime.historyReadWarnings?.value.length" data-history-read-details compact partial
              :warnings="runtime.historyReadWarnings.value" :truncated="false" />
          </template>
        </div>
        <LaunchProgramDiscovery v-if="shell.section === 'workspace' && !configurationEditor && sessions.activeSession?.safeErrorCode === 'LAUNCH_CONFIGURATION_REQUIRED' && sessions.activeSession.preparationIssueCode === 'PROGRAM_TRUST_REQUIRED'"
          :inert="configurationEditorOpening || undefined"
          :key="`${sessions.activeSession.id}:${shell.navigationSequence}:${shell.requestSequence}`" :session="sessions.activeSession" @edit="editPreparationConfiguration" @confirmed="confirmDiscoveredProgram" />
        <InlineNotice v-else-if="sessions.activeSession?.safeErrorCode === 'LAUNCH_CONFIGURATION_REQUIRED'" data-launch-preparation kind="warning" :inert="configurationEditorOpening || undefined"
          :message="t(mapSafeUserError(sessions.activeSession.preparationIssueCode ?? 'GENERIC_UNAVAILABLE', 'launch').messageKey)"
          :action-label="t('launchConfigEditAction')" @action="editPreparationConfiguration">
          <ErrorDetails :code="sessions.activeSession.preparationIssueCode ?? 'GENERIC_UNAVAILABLE'" context="launch" />
        </InlineNotice>
        <InlineNotice v-if="sessions.activeSession?.safeErrorCode === 'NEW_SESSION_PREPARATION_FAILED'" kind="warning"
          :message="t('newSessionPreparationFailed')" :action-label="t('newSessionMoreOptions')"
          @action="newSessionDraft.open(sessions.activeSession!, sessions.activeSession!.cli)" />
        <UnifiedTerminalHost v-show="!runtime.fatal?.value" ref="terminalHost" :sessions="runtime.openSessions.value" :active-session-id="sessions.activeSessionId"
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
    <InlineNotice v-if="configurationEditorOpening" data-configuration-prepare :message="t('loading')" :action-label="t('cancel')" @action="closeConfigurationEditor" />
    <InlineNotice v-if="configurationEditorError" :kind="configurationEditorError.severity" :message="t(configurationEditorError.messageKey)" />
    <LaunchConfigurationEditor v-if="configurationEditor" :request="configurationEditor" :active="shell.section === 'workspace'" @close="closeConfigurationEditor" />
    <ProjectConfirmDialog :request="configurations.deleteConfirmation" :active="shell.section === 'settings'" :busy="configurations.deleteBusy" :error-key="configurations.deleteError?.messageKey" @confirm="configurations.confirmDelete" @cancel="configurations.closeDeleteConfirmation" />
    <AppDrawer v-model:open="sourceDiagnosticsOpen" :title="t('historySourceDiagnostics')">
      <div data-history-source-dialog>
        <InlineNotice v-if="runtime.sourceDiagnosticsWarnings?.value.length" kind="warning" :message="t('workspaceRuntimePartial')" />
        <WorkspaceSourceDetails compact :warnings="runtime.sourceDiagnosticsWarnings.value" :truncated="runtime.sourceDiagnosticsTruncated.value"
          :configurations="runtime.sourceWarningConfigurations?.value ?? []" @configure="editSourceConfiguration" />
        <InlineNotice v-if="runtime.historyMetadataPartial?.value" kind="info" :message="t('workspaceHistoryMetadataPartial')" />
        <WorkspaceSourceDetails compact partial :warnings="runtime.historyReadWarnings.value" :truncated="false" />
        <p v-if="runtime.sourceChecksPending?.value">{{ t('loading') }}</p>
        <AppButton data-source-warning-retry variant="ghost" :disabled="runtime.loading.value" @click="request({ kind: 'refresh' })">{{ t('retry') }}</AppButton>
      </div>
    </AppDrawer>
    <SessionDiagnosticsDialog :diagnostics="runtime.diagnostics?.value ?? null" @close="runtime.closeDiagnostics()" />
    <SessionConfirmDialog :active="shell.section === 'workspace'" />
    <AppToastHost />
    <ResumeSessionDialog :active="shell.section === 'workspace'" />
    <InlineNotice v-if="management.error" kind="warning" :message="t(management.error)" :action-label="t('retry')" @action="management.refresh" />
    <ProjectsView v-show="shell.section === 'projects'" :active="shell.section === 'projects'"
      @add-project="request({ kind: 'add-project' })" @open="selectProject"
      @new-session-request="selectProject($event); request({ kind: 'new-session', project: $event })"
      @project-action="request({ kind: 'project-action', request: $event })" />
    <ProjectManagementDialogs />
    <SettingsView v-if="settingsLoaded" v-show="shell.section === 'settings'" :active="shell.section === 'settings'" @close="shell.navigate('workspace')" />
    <template #context>
      <ProjectResourcesDrawer />
    </template>
  </AppShell>
</template>

<style scoped>
.workspace-source-notice { flex-wrap: wrap; }
[data-history-source-entry] { margin: 4px 12px 0; align-self: flex-start; }
</style>
