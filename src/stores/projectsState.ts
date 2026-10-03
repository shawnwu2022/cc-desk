import { safeUserErrorCode } from '@/utils/userError'
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

function objectEntries(value: unknown): [string, unknown][] {
  return value && typeof value === 'object' && !Array.isArray(value) ? Object.entries(value) : []
}
function boundedText(value: unknown, maximum: number, empty = false): value is string {
  return typeof value === 'string' && (empty || value.length > 0) && !value.includes('\0') && [...value].length <= maximum
}
function sessionRecord(value: unknown): value is SessionUiRecord {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const row = value as SessionUiRecord
  return ['legacy-claude', 'native-cli'].includes(row.runtime) && ['claude', 'codex'].includes(row.cli)
    && boundedText(row.projectPath, 32768) && boundedText(row.adapterSessionId, 256)
    && (row.nativeSessionId == null || boundedText(row.nativeSessionId, 256)) && boundedText(row.title, 200, true)
    && Number.isInteger(row.lastActivityAt) && row.lastActivityAt >= 0
}
function launchPreference(value: unknown): value is ProjectLaunchPreference {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const row = value as ProjectLaunchPreference
  return ['claude', 'codex'].includes(row.lastCli)
    && [row.claudeLaunchConfigId, row.codexLaunchConfigId].every(id => id == null || boundedText(id, 256))
}
function syncStringMap(target: Map<string, string>, source: Record<string, string> | undefined): void {
  target.clear()
  for (const [key, value] of objectEntries(source)) {
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

function syncObjectMap<T>(target: Map<string, T>, source: unknown, valid: (value: unknown) => value is T, keyLimit: number): void {
  target.clear()
  for (const [key, value] of objectEntries(source).sort(([left], [right]) => left.localeCompare(right))) {
    if (target.size >= 10000) break
    if (boundedText(key, keyLimit) && valid(value)) target.set(key, value)
  }
}

export const useProjectsStateStore = defineStore('projects-state', () => {
  const pinnedProjects = ref<string[]>([])
  const archivedSessions = reactive(new Map<string, string[]>())
  const displayNames = reactive(new Map<string, string>())
  const sessionRecords = reactive(new Map<string, SessionUiRecord>())
  const launchPreferences = reactive(new Map<string, ProjectLaunchPreference>())
  const loaded = ref(false)
  const error = ref(false)
  const lastErrorCode = ref<string | null>(null)

  let loadPromise: Promise<void> | null = null
  let mutationTail: Promise<void> = Promise.resolve()

  function applyState(state: ProjectsState): void {
    pinnedProjects.value = [...(state.pinnedProjects ?? [])]
    syncStringArrayMap(archivedSessions, state.archivedSessions)
    syncStringMap(displayNames, state.displayNames)
    syncObjectMap(sessionRecords, state.sessionRecords, sessionRecord, 8192)
    syncObjectMap(launchPreferences, state.launchPreferences, launchPreference, 32768)
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

  function mutate(operation: () => Promise<ProjectsState>, reconcileFailure = false, beforeMutation?: () => void): Promise<ProjectsState> {
    return enqueue(async () => {
      await ensureLoaded()
      // Waits in the canonical queue (including initial load) cannot preserve
      // admission. A rejected owner has issued no write and needs no recovery.
      beforeMutation?.()
      try {
        const next = await operation()
        applyState(next)
        error.value = false
        lastErrorCode.value = null
        return next
      } catch (failure) {
        lastErrorCode.value = safeUserErrorCode(failure)
        if (reconcileFailure || errorCode(failure) === 'REVISION_CONFLICT') {
          try {
            await reloadNow()
          } catch {
            if (reconcileFailure) loaded.value = false
            error.value = true
          }
        }
        throw failure
      }
    })
  }

  function pinProject(path: string): Promise<ProjectsState> {
    return mutate(() => projectsApi.pinProject(path), true)
  }

  function unpinProject(path: string, beforeMutation?: () => void): Promise<ProjectsState> {
    return mutate(() => projectsApi.unpinProject(path), true, beforeMutation)
  }

  function setProjectDisplayName(path: string, alias: string): Promise<ProjectsState> {
    const validation = validateDisplayName(alias)
    if (!validation.ok) {
      return Promise.reject(new Error(
        validation.error === 'tooLong' ? 'alias too long' : 'alias invalid characters',
      ))
    }
    return mutate(() => projectsApi.setDisplayName(path, alias), true)
  }

  function archiveSession(projectPath: string, sessionId: string, beforeMutation?: () => void): Promise<ProjectsState> {
    return mutate(() => projectsApi.archiveSession(projectPath, sessionId), true, beforeMutation)
  }

  function restoreSession(projectPath: string, sessionId: string): Promise<ProjectsState> {
    return mutate(() => projectsApi.restoreSession(projectPath, sessionId), true)
  }

  function upsertSessionRecord(key: string, record: SessionUiRecord, beforeMutation?: () => void): Promise<ProjectsState> {
    return mutate(() => projectsApi.upsertSessionUiRecord(key, record), true, beforeMutation)
  }

  function removeSessionRecord(key: string): Promise<ProjectsState> {
    return mutate(() => projectsApi.removeSessionUiRecord(key), true)
  }

  function setLaunchPreference(
    projectPath: string,
    cli: UnifiedCliKind,
    launchConfigId: string | null,
  ): Promise<ProjectsState> {
    // Reconcile an uncertain acknowledgement while holding this writer queue.
    // A later CLI cannot merge against the pre-commit snapshot.
    return mutate(() => {
      // Read after prior mutations and the initial load have been adopted. Two
      // successful CLIs must not overwrite each other's last-used configuration.
      const current = launchPreferences.get(normalizePath(projectPath))
      const next: ProjectLaunchPreference = {
        lastCli: cli,
        claudeLaunchConfigId: current?.claudeLaunchConfigId ?? null,
        codexLaunchConfigId: current?.codexLaunchConfigId ?? null,
      }
      if (cli === 'claude') next.claudeLaunchConfigId = launchConfigId
      else next.codexLaunchConfigId = launchConfigId
      return projectsApi.setProjectLaunchPreference(projectPath, next)
    }, true)
  }

  return {
    pinnedProjects,
    archivedSessions,
    displayNames,
    sessionRecords,
    launchPreferences,
    loaded,
    error,
    lastErrorCode,
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
