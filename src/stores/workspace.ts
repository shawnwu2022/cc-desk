import { defineStore } from 'pinia'
import { ref } from 'vue'
import { listRegisteredProjects, registerProject, patchProject, removeProject } from '@/api/workspace'
import { parseU64 } from '@/utils/nativeIdentity'
import type { SafeError } from '@/types/cli'
import type { ProfileOverride } from '@/types/profile'
import type { ProjectChanges, ProjectList, ProjectMetadata, RegisteredProject } from '@/types/workspace'

function invalid(): never {
  throw { code: 'INVALID_WORKSPACE_RESPONSE', retryable: false } satisfies SafeError
}

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return invalid()
  return value as Record<string, unknown>
}

function text(value: unknown): string {
  if (typeof value !== 'string' || !value || value.includes('\0')) return invalid()
  return value
}

function override<T extends string | boolean>(value: unknown, type: 'string' | 'boolean'): ProfileOverride<T> {
  const item = record(value)
  if (item.mode === 'inherit' || item.mode === 'unset') {
    if ('value' in item) return invalid()
    return { mode: item.mode }
  }
  if (item.mode !== 'set' || typeof item.value !== type) return invalid()
  if (typeof item.value === 'string' && item.value.includes('\0')) return invalid()
  return { mode: 'set', value: item.value as T }
}

function validateList(value: unknown): ProjectList {
  const list = record(value)
  try { parseU64(list.revision) } catch { return invalid() }
  if (!Array.isArray(list.projects)) return invalid()
  const ids = new Set<string>()
  const projects: RegisteredProject[] = list.projects.map(item => {
    const project = record(item)
    const projectId = text(project.projectId)
    if (ids.has(projectId)) return invalid()
    ids.add(projectId)
    return {
      projectId,
      hostId: text(project.hostId),
      sourcePathKey: text(project.sourcePathKey),
      selectedPath: text(project.selectedPath),
      canonicalPath: project.canonicalPath === null ? null : text(project.canonicalPath),
      alias: override<string>(project.alias, 'string'),
      pinned: override<boolean>(project.pinned, 'boolean'),
      hidden: override<boolean>(project.hidden, 'boolean'),
    }
  })
  const metadata: Record<string, ProjectMetadata> = Object.create(null)
  if (list.metadata !== undefined) {
    for (const [id, value] of Object.entries(record(list.metadata))) {
      if (!ids.has(id)) return invalid()
      const item = record(value)
      if (item.alias !== null && typeof item.alias !== 'string') return invalid()
      if (item.pinned !== null && typeof item.pinned !== 'boolean') return invalid()
      if (item.hidden !== null && typeof item.hidden !== 'boolean') return invalid()
      metadata[id] = item as unknown as ProjectMetadata
    }
  }
  if (list.warnings !== undefined && (!Array.isArray(list.warnings)
    || list.warnings.some(item => typeof item !== 'string'))) return invalid()
  return {
    revision: list.revision as string, projects, metadata,
    warnings: (list.warnings ?? []) as string[],
    ...(list.projectId !== undefined ? { projectId: text(list.projectId) } : {}),
  }
}

export const useWorkspaceStore = defineStore('cli-workspace', () => {
  const projects = ref<RegisteredProject[]>([])
  const revision = ref('0')
  const metadata = ref<Record<string, ProjectMetadata>>({})
  const warnings = ref<string[]>([])
  const status = ref<'idle' | 'loading' | 'loaded' | 'error'>('idle')
  const lastError = ref<unknown>(null)
  let epoch = 0
  let initialized = false
  let mutationTail: Promise<void> = Promise.resolve()

  async function execute(operation: () => Promise<ProjectList>): Promise<ProjectList> {
    const current = ++epoch
    status.value = 'loading'
    try {
      const next = validateList(await operation())
      // An older list reply must never erase a more recent acknowledged mutation.
      if (!initialized || parseU64(next.revision) >= parseU64(revision.value)) {
        projects.value = next.projects
        revision.value = next.revision
        metadata.value = next.metadata ?? {}
        warnings.value = next.warnings ?? []
        initialized = true
      }
      if (current === epoch) {
        status.value = 'loaded'
        lastError.value = null
      }
      return next
    } catch (error) {
      if (current === epoch) {
        status.value = 'error'
        lastError.value = error
      }
      throw error
    }
  }

  function mutate(operation: () => Promise<ProjectList>): Promise<ProjectList> {
    const next = mutationTail.then(() => execute(operation))
    mutationTail = next.then(() => undefined, () => undefined)
    return next
  }

  const load = () => execute(listRegisteredProjects)

  async function register(selectedPath: string): Promise<string> {
    const result = await mutate(() => registerProject(selectedPath))
    if (!result.projectId) return invalid()
    return result.projectId
  }

  function patch(projectId: string, changes: ProjectChanges): Promise<ProjectList> {
    return mutate(() => {
      if (!initialized) throw { code: 'WORKSPACE_NOT_LOADED', retryable: false } satisfies SafeError
      return patchProject(projectId, revision.value, changes)
    })
  }

  function remove(projectId: string): Promise<ProjectList> {
    return mutate(() => {
      if (!initialized) throw { code: 'WORKSPACE_NOT_LOADED', retryable: false } satisfies SafeError
      return removeProject(projectId, revision.value)
    })
  }

  return { projects, revision, metadata, warnings, status, lastError, load, register, patch, remove }
})
