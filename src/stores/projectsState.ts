import { defineStore } from 'pinia'
import { reactive, ref } from 'vue'
import * as projectsApi from '@/api/tauri'
import type {
  ProjectLaunchPreference,
  ProjectsState,
  SessionUiRecord,
} from '@/types/app'
import type { UnifiedCliKind } from '@/types/unifiedSession'
import { normalizePath } from '@/utils/path'
import { validateDisplayName } from '@/utils/displayName'

function errorCode(value: unknown): string | null {
  if (value && typeof value === 'object' && 'code' in value) {
    const code = (value as { code?: unknown }).code
    return typeof code === 'string' ? code : null
  }
  if (value instanceof Error) return value.message
  return null
}

function syncStringMap(target: Map<string, string>, source: Record<string, string> | undefined): void {
  target.clear()
  for (const [key, value] of Object.entries(source ?? {})) {
    if (typeof value === 'string') target.set(key, value)
  }
}

function syncStringArrayMap(
  target: Map<string, string[]>,
  source: Record<string, string[]> | undefined,
): void {
  target.clear()
  for (const [key, value] of Object.entries(source ?? {})) {
    if (Array.isArray(value)) target.set(key, [...value])
  }
}

function syncObjectMap<T>(target: Map<string, T>, source: Record<string, T> | undefined): void {
  target.clear()
  for (const [key, value] of Object.entries(source ?? {})) target.set(key, value)
}

export const useProjectsStateStore = defineStore('projects-state', () => {
  const pinnedProjects = ref<string[]>([])
  const archivedSessions = reactive(new Map<string, string[]>())
  const displayNames = reactive(new Map<string, string>())
  const sessionRecords = reactive(new Map<string, SessionUiRecord>())
  const launchPreferences = reactive(new Map<string, ProjectLaunchPreference>())
  const loaded = ref(false)
  const error = ref(false)

  let loadPromise: Promise<void> | null = null
  let mutationTail: Promise<void> = Promise.resolve()

  function applyState(state: ProjectsState): void {
    pinnedProjects.value = [...(state.pinnedProjects ?? [])]
    syncStringArrayMap(archivedSessions, state.archivedSessions)
    syncStringMap(displayNames, state.displayNames)
    syncObjectMap(sessionRecords, state.sessionRecords)
    syncObjectMap(launchPreferences, state.launchPreferences)
  }

  function load(): Promise<void> {
    if (loadPromise) return loadPromise
    loadPromise = (async () => {
      try {
        const next = await projectsApi.getProjectsState()
        applyState(next)
        loaded.value = true
        error.value = false
      } catch (failure) {
        loaded.value = false
        error.value = true
        throw failure
      } finally {
        loadPromise = null
      }
    })()
    return loadPromise
  }

  async function ensureLoaded(): Promise<void> {
    if (loaded.value) return
    await load()
    if (!loaded.value) throw new Error('PROJECTS_STATE_NOT_LOADED')
  }

  function enqueue<T>(operation: () => Promise<T>): Promise<T> {
    const next = mutationTail.then(operation)
    mutationTail = next.then(() => undefined, () => undefined)
    return next
  }

  async function reloadNow(): Promise<ProjectsState> {
    const next = await projectsApi.getProjectsState()
    applyState(next)
    loaded.value = true
    error.value = false
    return next
  }

  function reload(): Promise<ProjectsState> {
    return enqueue(reloadNow)
  }

  function mutate(operation: () => Promise<ProjectsState>): Promise<ProjectsState> {
    return enqueue(async () => {
      await ensureLoaded()
      try {
        const next = await operation()
        applyState(next)
        error.value = false
        return next
      } catch (failure) {
        if (errorCode(failure) === 'REVISION_CONFLICT') {
          try {
            await reloadNow()
          } catch {
            error.value = true
          }
        }
        throw failure
      }
    })
  }

  function pinProject(path: string): Promise<ProjectsState> {
    return mutate(() => projectsApi.pinProject(path))
  }

  function unpinProject(path: string): Promise<ProjectsState> {
    return mutate(() => projectsApi.unpinProject(path))
  }

  function setProjectDisplayName(path: string, alias: string): Promise<ProjectsState> {
    const validation = validateDisplayName(alias)
    if (!validation.ok) {
      return Promise.reject(new Error(
        validation.error === 'tooLong' ? 'alias too long' : 'alias invalid characters',
      ))
    }
    return mutate(() => projectsApi.setDisplayName(path, alias))
  }

  function archiveSession(projectPath: string, sessionId: string): Promise<ProjectsState> {
    return mutate(() => projectsApi.archiveSession(projectPath, sessionId))
  }

  function restoreSession(projectPath: string, sessionId: string): Promise<ProjectsState> {
    return mutate(() => projectsApi.restoreSession(projectPath, sessionId))
  }

  function upsertSessionRecord(key: string, record: SessionUiRecord): Promise<ProjectsState> {
    return mutate(() => projectsApi.upsertSessionUiRecord(key, record))
  }

  function removeSessionRecord(key: string): Promise<ProjectsState> {
    return mutate(() => projectsApi.removeSessionUiRecord(key))
  }

  function setLaunchPreference(
    projectPath: string,
    cli: UnifiedCliKind,
    launchConfigId: string | null,
  ): Promise<ProjectsState> {
    const normalized = normalizePath(projectPath)
    const current = launchPreferences.get(normalized)
    const next: ProjectLaunchPreference = {
      lastCli: cli,
      claudeLaunchConfigId: current?.claudeLaunchConfigId ?? null,
      codexLaunchConfigId: current?.codexLaunchConfigId ?? null,
    }
    if (cli === 'claude') next.claudeLaunchConfigId = launchConfigId
    else next.codexLaunchConfigId = launchConfigId
    return mutate(() => projectsApi.setProjectLaunchPreference(projectPath, next))
  }

  return {
    pinnedProjects,
    archivedSessions,
    displayNames,
    sessionRecords,
    launchPreferences,
    loaded,
    error,
    load,
    reload,
    ensureLoaded,
    mutate,
    pinProject,
    unpinProject,
    setProjectDisplayName,
    archiveSession,
    restoreSession,
    upsertSessionRecord,
    removeSessionRecord,
    setLaunchPreference,
  }
})