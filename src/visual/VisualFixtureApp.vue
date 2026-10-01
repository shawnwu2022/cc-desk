<script setup lang="ts">
import { ref, onMounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import AppShell from '@/components/shell/AppShell.vue'
import SidebarPanel from '@/components/sidebar/SidebarPanel.vue'
import WorkspaceView from '@/components/workspace/WorkspaceView.vue'
import UnifiedTerminalHost from '@/components/workspace/UnifiedTerminalHost.vue'
import ProjectsView from '@/components/projects/ProjectsView.vue'
import SettingsView from '@/components/settings/SettingsView.vue'
import TerminalThemePreview from '@/components/settings/TerminalThemePreview.vue'
import ProjectResourcesDrawer from '@/components/workspace/ProjectResourcesDrawer.vue'
import NewSessionDialog from '@/components/sessions/NewSessionDialog.vue'
import ArchivedSessionsDrawer from '@/components/sessions/ArchivedSessionsDrawer.vue'
import SessionConfirmDialog from '@/components/dialogs/SessionConfirmDialog.vue'
import EmptyState from '@/components/ui/EmptyState.vue'
import AppTooltip from '@/components/ui/AppTooltip.vue'
import AppButton from '@/components/ui/AppButton.vue'
import { useAppStore } from '@/stores/app'
import { useShellStore } from '@/stores/shell'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useProjectManagementStore } from '@/stores/projectManagement'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useSidebarStore } from '@/stores/sidebar'
import { useProjectResourcesStore } from '@/stores/projectResources'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import { fixtureSessions, fixtureProfiles, fixtureResources, longProjectName, projectPaths } from './fixtures'
import { blockedHostCalls } from './tauriStub'
import { applyThemeToDom } from '@/utils/theme'
import { resolveTerminalThemeId } from '@/config/terminalThemes'

const parameters = new URLSearchParams(location.search)
const scenario = parameters.get('scenario') ?? 'mixed'
const emptyWorkspace = scenario === 'empty' || scenario === 'empty-project'
const { locale, t } = useI18n()
locale.value = parameters.get('locale') === 'zh' ? 'zh' : 'en'
const app = useAppStore(), shell = useShellStore(), catalog = useUnifiedSessionsStore()
const metadata = useProjectsStateStore(), profiles = useCliProfilesStore(), sidebar = useSidebarStore()
app.theme = parameters.get('gui') === 'dark' ? 'dark' : 'light'
app.terminalTheme = resolveTerminalThemeId(parameters.get('terminal') ?? 'cc-box-dark', app.theme)
app.terminalCursorBlink = false
app.guiDensity = parameters.get('density') === 'compact' ? 'compact' : 'standard'
applyThemeToDom(app.theme)
document.documentElement.dataset.density = app.guiDensity
document.documentElement.dataset.visualFixture = ''
metadata.loaded = true
profiles.profiles = structuredClone(fixtureProfiles); profiles.status = 'loaded'; profiles.revision = '1'
profiles.select('claude', 'visual-claude-0'); profiles.select('codex', 'visual-codex-0')
catalog.sessions = emptyWorkspace ? [] : fixtureSessions()
catalog.activeSessionId = catalog.sessions[0]?.id ?? null
app.cachedProjects = scenario === 'empty' ? [] : projectPaths.map((path, index) => ({ path, name: ['cc-desk', 'Atlas design system', longProjectName, 'Empty project'][index], lastDuration: 0 }))
metadata.displayNames.set(projectPaths[2].toLowerCase(), longProjectName)
const management = useProjectManagementStore()
const resources = useProjectResourcesStore()
resources.kind = 'mcp'; resources.items = structuredClone(fixtureResources); resources.partial = true
const draft = useNewSessionDraftStore()
const project = { projectKey: projectPaths[0].toLowerCase(), projectPath: projectPaths[0] }
const archived = ref(scenario === 'archived')
if (scenario === 'projects') shell.navigate('projects')
else if (scenario === 'terminal-settings' || scenario === 'launch-configurations') {
  sidebar.activeSettingsSection = scenario === 'terminal-settings' ? 'terminal' : 'launch-configurations'
  shell.navigate('settings')
}
if (scenario === 'resources') shell.drawerVisible = true
if (scenario === 'new-session') { draft.open(project, 'codex'); draft.title = 'Review terminal continuity' }
if (scenario === 'confirmation') catalog.sessionConfirmation = { kind: 'stop-and-archive', sessionId: 'visual-session-0', title: 'Review terminal rendering' }
const ready = ref(false)
onMounted(async () => { await nextTick(); ready.value = true })
</script>

<template>
  <div :data-visual-ready="ready" :data-blocked-host-calls="blockedHostCalls">
    <AppShell title="CC Desk · Visual project">
      <template #sidebar><SidebarPanel :project-groups="management.visibleGroups" :archived-sessions="catalog.sessions" :selected-id="catalog.activeSessionId" /></template>
      <WorkspaceView v-if="shell.section === 'workspace'" :project="scenario === 'empty' ? null : project" project-title="cc-desk" :active-session="catalog.activeSession">
        <template #terminal>
          <!-- Empty sessions exercise the production layout without mounting either terminal. -->
          <UnifiedTerminalHost v-if="emptyWorkspace" :sessions="[]" :active-session-id="null">
            <EmptyState :title="t('workspaceWelcome')" :description="t('workspaceWelcomeHint')" :action-label="t(scenario === 'empty' ? 'addProject' : 'newSession')" />
          </UnifiedTerminalHost>
          <div v-else class="visual-terminal"><TerminalThemePreview :preferences="app.terminalPreferences" /></div>
        </template>
      </WorkspaceView>
      <ProjectsView v-else-if="shell.section === 'projects'" />
      <SettingsView v-else />
      <template #context><ProjectResourcesDrawer /></template>
    </AppShell>
    <NewSessionDialog />
    <ArchivedSessionsDrawer v-model:open="archived" :sessions="catalog.sessions" />
    <SessionConfirmDialog />
    <div v-if="scenario === 'tooltip'" class="visual-tooltip-clipping" data-tooltip-clipping>
      <AppTooltip text="A long project description that must remain readable outside transformed and clipped ancestors, near the viewport edge."><AppButton data-tooltip-trigger>Inspect project</AppButton></AppTooltip>
    </div>
  </div>
</template>
