import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import type { NativeCliKind } from '@/types/cli'
import type { ProjectionResult, ResourceKind } from '@/types/nativeProjection'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useWorkspaceStore } from '@/stores/workspace'
import { useNativeProjectionStore } from '@/stores/nativeProjection'

export interface CliWorkspaceProfileIdentity {
  profileId: string
  revision: string
  cli: NativeCliKind
}

const SAFE_ERRORS = new Set([
  'REVISION_CONFLICT',
  'COMMIT_STATE_UNKNOWN',
  'INVALID_WORKSPACE_RESPONSE',
  'WORKSPACE_NOT_LOADED',
  'PROFILE_NOT_FOUND',
  'PROJECT_NOT_FOUND',
  'PROJECT_IDENTITY_CHANGED',
  'DOCUMENT_BRIDGE_UNAVAILABLE',
  'BACKEND_INSTANCE_CHANGED',
  'FORBIDDEN',
  'INVALID_REQUEST',
  'SCOPE_UNKNOWN',
  'SCOPE_STALE',
  'SCOPE_REVOKED',
  'SCOPE_CAPACITY',
  'SCOPE_EPOCH_EXHAUSTED',
  'SCOPE_UNAVAILABLE',
  'SOURCE_UNSUPPORTED',
  'SOURCE_INVALID',
  'SOURCE_INVALID_TEXT',
  'SOURCE_PATH_REJECTED',
  'SOURCE_CHANGED',
  'SOURCE_NOT_REGULAR',
  'SOURCE_TOO_LARGE',
  'SOURCE_TOO_MANY_ENTRIES',
  'SOURCE_BUDGET_EXCEEDED',
  'SOURCE_READ_FAILED',
  'SOURCE_READ_FORBIDDEN',
  'SOURCE_RESPONSE_TOO_LARGE',
  'SOURCE_AMBIGUOUS',
  'SOURCE_BUSY',
  'SOURCE_TASK_FAILED',
])

function safeErrorCode(value: unknown): string {
  if (value && typeof value === 'object' && 'code' in value) {
    const code = (value as { code?: unknown }).code
    if (typeof code === 'string' && SAFE_ERRORS.has(code)) return code
  }
  return 'NATIVE_WORKSPACE_UNAVAILABLE'
}

export const useCliWorkspaceStore = defineStore('cli-product-workspace', () => {
  const profiles = useCliProfilesStore()
  const projectsStore = useWorkspaceStore()
  const projection = useNativeProjectionStore()

  const cli = ref<NativeCliKind | null>(null)
  const profileIdentity = ref<CliWorkspaceProfileIdentity | null>(null)
  const status = ref<'idle' | 'loading' | 'ready' | 'error'>('idle')
  const error = ref<string | null>(null)
  let owner: object = {}

  const projects = computed(() => projectsStore.projects)
  const enrichment = computed(() => projectsStore.enrichment)
  const resource = computed<ProjectionResult | null>(() => projection.result)

  function clearResource() {
    projection.clear()
  }

  function invalidate(code: string) {
    clearResource()
    void projectsStore.enrich(null)
    status.value = 'error'
    error.value = code
  }

  function currentSelectionMatches(): boolean {
    const identity = profileIdentity.value
    if (!identity) return false
    const selected = profiles.selected[identity.cli]
    return Boolean(
      selected
      && selected.id === identity.profileId
      && selected.revision === identity.revision
      && selected.cli === identity.cli,
    )
  }

  function requireCurrentProfile(): CliWorkspaceProfileIdentity {
    const identity = profileIdentity.value
    if (!identity || cli.value !== identity.cli) {
      invalidate('CLI_PROFILE_REQUIRED')
      throw new Error('CLI_PROFILE_REQUIRED')
    }
    if (!currentSelectionMatches()) {
      invalidate('PROFILE_SELECTION_CHANGED')
      throw new Error('PROFILE_SELECTION_CHANGED')
    }
    return identity
  }

  async function open(nextCli: NativeCliKind): Promise<void> {
    const selected = profiles.selected[nextCli]
    if (!selected) {
      owner = {}
      cli.value = nextCli
      profileIdentity.value = null
      invalidate('CLI_PROFILE_REQUIRED')
      throw new Error('CLI_PROFILE_REQUIRED')
    }
    if (selected.cli !== nextCli) {
      owner = {}
      cli.value = nextCli
      profileIdentity.value = null
      invalidate('PROFILE_CLI_MISMATCH')
      throw new Error('PROFILE_CLI_MISMATCH')
    }

    const selectedOwner = {}
    owner = selectedOwner
    const identity: CliWorkspaceProfileIdentity = Object.freeze({
      profileId: selected.id,
      revision: selected.revision,
      cli: nextCli,
    })
    cli.value = nextCli
    profileIdentity.value = identity
    status.value = 'loading'
    error.value = null
    clearResource()

    try {
      await projectsStore.loadNativeHome({
        profileId: identity.profileId,
        revision: identity.revision,
      })
      if (owner === selectedOwner) {
        if (!currentSelectionMatches()) {
          owner = {}
          invalidate('PROFILE_SELECTION_CHANGED')
          throw new Error('PROFILE_SELECTION_CHANGED')
        }
        status.value = 'ready'
        error.value = null
      }
    } catch (failure) {
      if (owner === selectedOwner) {
        status.value = 'error'
        error.value = safeErrorCode(failure)
      }
      throw failure
    }
  }

  async function registerProject(selectedPath: string): Promise<string> {
    const identity = requireCurrentProfile()
    const selectedOwner = owner
    error.value = null
    try {
      const projectId = await projectsStore.register(selectedPath)
      if (owner === selectedOwner && currentSelectionMatches()) {
        await projectsStore.enrich({
          profileId: identity.profileId,
          revision: identity.revision,
        })
      }
      if (owner === selectedOwner) {
        status.value = currentSelectionMatches() ? 'ready' : 'error'
        error.value = currentSelectionMatches() ? null : 'PROFILE_SELECTION_CHANGED'
      }
      return projectId
    } catch (failure) {
      if (owner === selectedOwner) {
        error.value = safeErrorCode(failure)
      }
      throw failure
    }
  }

  async function loadResource(
    kind: ResourceKind,
    options: {
      projectId?: string
      query?: string
      sessionId?: string
      limit?: number
      offset?: number
    } = {},
  ): Promise<void> {
    const identity = requireCurrentProfile()

    const selectedOwner = owner
    const projectId = options.projectId
    if (projectId !== undefined
      && !projectsStore.projects.some(project => project.projectId === projectId)) {
      error.value = 'PROJECT_NOT_FOUND'
      throw new Error('PROJECT_NOT_FOUND')
    }
    const { projectId: _projectId, ...readOptions } = options
    error.value = null
    await projection.load(
      {
        kind: 'profile',
        profileId: identity.profileId,
        expectedProfileRevision: identity.revision,
        ...(projectId === undefined ? {} : { projectId }),
      },
      kind,
      readOptions,
    )
    if (owner !== selectedOwner) return

    if (projection.error) {
      error.value = SAFE_ERRORS.has(projection.error)
        ? projection.error
        : 'NATIVE_WORKSPACE_UNAVAILABLE'
    } else {
      error.value = null
    }
  }

  watch(
    () => [
      profiles.selected.claude?.id ?? null,
      profiles.selected.claude?.revision ?? null,
      profiles.selected.codex?.id ?? null,
      profiles.selected.codex?.revision ?? null,
    ],
    () => {
      if (profileIdentity.value && !currentSelectionMatches()) {
        owner = {}
        invalidate('PROFILE_SELECTION_CHANGED')
      }
    },
  )

  function clear() {
    owner = {}
    cli.value = null
    profileIdentity.value = null
    status.value = 'idle'
    error.value = null
    clearResource()
    void projectsStore.enrich(null)
  }

  return {
    cli,
    profileIdentity,
    status,
    error,
    projects,
    enrichment,
    resource,
    open,
    registerProject,
    loadResource,
    clear,
  }
})
