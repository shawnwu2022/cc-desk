import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import type { LaunchAction, NativeCliKind } from '@/types/cli'
import type { CliProfile } from '@/types/profile'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useCliWorkspaceStore } from '@/stores/cliWorkspace'
import { useNativeTabsStore } from '@/stores/nativeTabs'

const SAFE_WORKBENCH_ERRORS = new Set([
  'CLI_PROFILE_REQUIRED',
  'PROFILE_CLI_MISMATCH',
  'PROFILE_SELECTION_CHANGED',
  'PROFILE_NOT_FOUND',
  'PROFILE_EXISTS',
  'PROFILE_CREATE_NOT_ADOPTED',
  'PROJECT_REQUIRED',
  'PROJECT_NOT_FOUND',
  'REVISION_CONFLICT',
  'DOCUMENT_BRIDGE_UNAVAILABLE',
  'BACKEND_INSTANCE_CHANGED',
])

function safeWorkbenchError(
  failure: unknown,
  workspaceError: string | null = null,
): string {
  if (workspaceError) return workspaceError
  if (failure instanceof Error && SAFE_WORKBENCH_ERRORS.has(failure.message)) {
    return failure.message
  }
  if (failure && typeof failure === 'object' && 'code' in failure) {
    const code = (failure as { code?: unknown }).code
    if (typeof code === 'string' && SAFE_WORKBENCH_ERRORS.has(code)) return code
  }
  return 'NATIVE_WORKBENCH_UNAVAILABLE'
}

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
      error.value = safeWorkbenchError(failure, workspace.error)
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
      error.value = safeWorkbenchError(failure, workspace.error)
      throw failure
    }
  }

  async function selectProfile(nextCli: NativeCliKind, profileId: string): Promise<void> {
    profiles.select(nextCli, profileId)
    if (cli.value === nextCli) await selectCli(nextCli)
  }

  async function createDefaultProfile(nextCli: NativeCliKind): Promise<CliProfile> {
    status.value = 'loading'
    error.value = null
    let workspaceAttempted = false

    try {
      let adopted = profiles.byCli[nextCli][0]

      if (!adopted) {
        const preferredId = nextCli === 'claude' ? 'legacyClaude' : 'codexDefault'
        const id = profiles.profile(preferredId)
          ? `${nextCli}-${crypto.randomUUID()}`
          : preferredId
        const created: CliProfile = {
          id,
          revision: '0',
          cli: nextCli,
          name: nextCli === 'claude' ? 'Claude Code' : 'Codex CLI',
          launcher: { kind: 'native' },
          programPath: { mode: 'inherit' },
          defaultArgs: { mode: 'inherit' },
          skipPermissions: { mode: 'inherit' },
          observer: { mode: 'inherit' },
          env: {},
        }

        const result = await profiles.patch(profiles.revision, {
          op: 'create',
          profile: created,
        })
        adopted = result.profiles.find(item => item.id === id && item.cli === nextCli)
        if (!adopted) throw new Error('PROFILE_CREATE_NOT_ADOPTED')
      }

      profiles.select(nextCli, adopted.id)
      cli.value = nextCli
      workspaceAttempted = true
      await workspace.open(nextCli)
      reconcileProject()
      status.value = 'ready'
      error.value = null
      return adopted
    } catch (failure) {
      status.value = 'error'
      error.value = safeWorkbenchError(
        failure,
        workspaceAttempted ? workspace.error : null,
      )
      throw failure
    }
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
    createDefaultProfile,
    selectProject,
    createTab,
    restartTab,
    closeTab,
    clear,
  }
})
