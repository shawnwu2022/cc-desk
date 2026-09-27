import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import type { LaunchAction, NativeCliKind } from '@/types/cli'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useCliWorkspaceStore } from '@/stores/cliWorkspace'
import { useNativeTabsStore } from '@/stores/nativeTabs'

export const useNativeWorkbenchStore = defineStore('native-cli-workbench', () => {
  const profiles = useCliProfilesStore()
  const workspace = useCliWorkspaceStore()
  const tabs = useNativeTabsStore()

  const cli = ref<NativeCliKind>('codex')
  const selectedProjectId = ref<string | null>(null)
  const status = ref<'idle' | 'loading' | 'ready' | 'error'>('idle')
  const error = ref<string | null>(null)

  const selectedProject = computed(() => {
    const id = selectedProjectId.value
    if (!id) return null
    return workspace.projects.find(item => item.projectId === id) ?? null
  })

  function reconcileProject() {
    const id = selectedProjectId.value
    if (id && workspace.projects.some(item => item.projectId === id)) return
    selectedProjectId.value = workspace.projects[0]?.projectId ?? null
  }

  function firstAvailable(preferred: NativeCliKind): NativeCliKind | null {
    if (profiles.selected[preferred]) return preferred
    const other: NativeCliKind = preferred === 'claude' ? 'codex' : 'claude'
    return profiles.selected[other] ? other : null
  }

  async function initialize(preferred: NativeCliKind = 'codex'): Promise<void> {
    status.value = 'loading'
    error.value = null
    try {
      await profiles.load()
      const chosen = firstAvailable(preferred)
      if (!chosen) throw new Error('CLI_PROFILE_REQUIRED')
      cli.value = chosen
      await workspace.open(chosen)
      reconcileProject()
      status.value = 'ready'
    } catch (failure) {
      status.value = 'error'
      error.value = failure instanceof Error ? failure.message : 'NATIVE_WORKBENCH_UNAVAILABLE'
      throw failure
    }
  }

  async function selectCli(next: NativeCliKind): Promise<void> {
    if (!profiles.selected[next]) throw new Error('CLI_PROFILE_REQUIRED')
    status.value = 'loading'
    error.value = null
    try {
      cli.value = next
      await workspace.open(next)
      reconcileProject()
      status.value = 'ready'
    } catch (failure) {
      status.value = 'error'
      error.value = failure instanceof Error ? failure.message : 'NATIVE_WORKBENCH_UNAVAILABLE'
      throw failure
    }
  }

  async function selectProfile(nextCli: NativeCliKind, profileId: string): Promise<void> {
    profiles.select(nextCli, profileId)
    if (cli.value === nextCli) await selectCli(nextCli)
  }

  function selectProject(projectId: string): void {
    if (!workspace.projects.some(item => item.projectId === projectId)) {
      throw new Error('PROJECT_NOT_FOUND')
    }
    selectedProjectId.value = projectId
  }

  function createTab(action: LaunchAction) {
    const selectedProfile = profiles.selected[cli.value]
    if (!selectedProfile) throw new Error('CLI_PROFILE_REQUIRED')
    const project = selectedProject.value
    if (!project) throw new Error('PROJECT_REQUIRED')

    return tabs.create({
      cli: cli.value,
      projectId: project.projectId,
      projectPath: project.selectedPath,
      profileId: selectedProfile.id,
      profileRevision: selectedProfile.revision,
      action,
    })
  }

  function restartTab(tabId: string) {
    const value = tabs.tab(tabId)
    if (!value) throw new Error('TAB_NOT_FOUND')
    const selectedProfile = profiles.selected[value.cli]
    if (!selectedProfile) throw new Error('CLI_PROFILE_REQUIRED')
    return tabs.restart(tabId, {
      cli: value.cli,
      profileId: selectedProfile.id,
      profileRevision: selectedProfile.revision,
    })
  }

  function closeTab(tabId: string) {
    tabs.close(tabId)
  }

  function clear() {
    selectedProjectId.value = null
    status.value = 'idle'
    error.value = null
    workspace.clear()
    tabs.clear()
  }

  return {
    profiles,
    workspace,
    tabs,
    cli,
    selectedProjectId,
    selectedProject,
    status,
    error,
    initialize,
    selectCli,
    selectProfile,
    selectProject,
    createTab,
    restartTab,
    closeTab,
    clear,
  }
})
