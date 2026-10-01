import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useNotificationsStore } from '@/stores/notifications'
import type { DeleteLaunchConfigurationRequest } from '@/types/confirmation'
import { mapSafeUserError, type UserErrorPresentation, safeUserErrorCode } from '@/utils/userError'
import { cliListProfiles, cliPatchProfile } from '@/api/cli'
import type { SafeError, NativeCliKind } from '@/types/cli'
import type {
  CliProfile,
  ProfileEnvValue,
  ProfileList,
  ProfileOverride,
  ProfilePatch,
  ProfileLauncher,
} from '@/types/profile'
import { parseU64 } from '@/utils/nativeIdentity'
import { computed, ref } from 'vue'
import { defineStore } from 'pinia'

function invalid(): never {
  throw { code: 'INVALID_PROFILE_RESPONSE', retryable: false } satisfies SafeError
}

function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return invalid()
  return value as Record<string, unknown>
}

function text(value: unknown): string {
  if (typeof value !== 'string' || value.length === 0 || value.includes('\0')) return invalid()
  return value
}

function u64(value: unknown): string {
  if (typeof value !== 'string') return invalid()
  try {
    parseU64(value)
    return value
  } catch {
    return invalid()
  }
}

function override<T>(value: unknown, parse: (item: unknown) => T): ProfileOverride<T> {
  const row = object(value)
  if (row.mode === 'inherit' || row.mode === 'unset') {
    if ('value' in row) return invalid()
    return { mode: row.mode }
  }
  if (row.mode !== 'set' || !('value' in row)) return invalid()
  return { mode: 'set', value: parse(row.value) }
}

function bool(value: unknown): boolean {
  if (typeof value !== 'boolean') return invalid()
  return value
}

function stringValue(value: unknown): string {
  if (typeof value !== 'string' || value.includes('\0')) return invalid()
  return value
}

function stringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return invalid()
  return value.map(stringValue)
}

function envValue(value: unknown): ProfileEnvValue {
  const row = object(value)
  if (row.kind === 'literal') {
    if (row.nonSecret !== true) return invalid()
    return { kind: 'literal', value: stringValue(row.value), nonSecret: true }
  }
  if (row.kind === 'host-ref') {
    return { kind: 'host-ref', name: text(row.name) }
  }
  return invalid()
}

function launcher(value: unknown): ProfileLauncher {
  const row = object(value)
  if (row.kind === 'native') {
    if (Object.keys(row).length !== 1) return invalid()
    return { kind: 'native' }
  }
  if (row.kind === 'shell') {
    if (!['bash', 'power-shell', 'cmd'].includes(String(row.dialect))) return invalid()
    return {
      kind: 'shell',
      program: text(row.program),
      dialect: row.dialect as 'bash' | 'power-shell' | 'cmd',
    }
  }
  if (row.kind === 'shim') {
    if (!['bash', 'power-shell', 'cmd'].includes(String(row.dialect))) return invalid()
    return {
      kind: 'shim',
      runner: text(row.runner),
      dialect: row.dialect as 'bash' | 'power-shell' | 'cmd',
    }
  }
  return invalid()
}

function profile(value: unknown): CliProfile {
  const row = object(value)
  if (row.cli !== 'claude' && row.cli !== 'codex') return invalid()

  const envRow = object(row.env)
  const env: CliProfile['env'] = {}
  for (const [key, item] of Object.entries(envRow)) {
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) return invalid()
    env[key] = override(item, envValue)
  }

  return {
    id: text(row.id),
    revision: u64(row.revision),
    cli: row.cli,
    name: text(row.name),
    launcher: launcher(row.launcher),
    programPath: override(row.programPath, stringValue),
    defaultArgs: override(row.defaultArgs, stringArray),
    skipPermissions: override(row.skipPermissions, bool),
    observer: override(row.observer, bool),
    env,
  }
}

function validateList(value: unknown): ProfileList {
  const row = object(value)
  const revision = u64(row.revision)
  if (!Array.isArray(row.profiles)) return invalid()

  const ids = new Set<string>()
  const profiles = row.profiles.map(item => {
    const parsed = profile(item)
    if (ids.has(parsed.id)) return invalid()
    ids.add(parsed.id)
    return parsed
  })
  return { revision, profiles }
}

export const useCliProfilesStore = defineStore('cli-profiles', () => {
  const profiles = ref<CliProfile[]>([])
  const revision = ref('0')
  const status = ref<'idle' | 'loading' | 'loaded' | 'error'>('idle')
  const lastError = ref<string | null>(null)
  const deleteConfirmation = ref<DeleteLaunchConfigurationRequest | null>(null)
  const deleteError = ref<UserErrorPresentation | null>(null)
  const deleteBusy = ref(false)
  const deletingIds = new Set<string>()
  function isDeleting(id: string) { return deletingIds.has(id) }
  function isInUse(id: string) { return [...useNativeTabsStore().tabs.values()].some(tab => tab.profileId === id) }
  function closeDeleteConfirmation() { deleteConfirmation.value = null; deleteError.value = null }
  function requestDelete(id: string): DeleteLaunchConfigurationRequest | null {
    const profile = profiles.value.find(row => row.id === id)
    deleteError.value = null
    if (!profile || isInUse(id) || deletingIds.has(id)) {
      deleteError.value = mapSafeUserError(profile ? 'PROFILE_IN_USE' : 'PROFILE_SELECTION_CHANGED', 'settings')
      return null
    }
    deleteConfirmation.value = { kind: 'delete-launch-configuration', title: profile.name, profileId: id, profileRevision: profile.revision, workspaceRevision: revision.value }
    return deleteConfirmation.value
  }
  async function confirmDelete(): Promise<boolean> {
    const request = deleteConfirmation.value
    if (!request || deleteBusy.value) return false
    const current = () => deleteConfirmation.value === request
    deleteBusy.value = true; deleteError.value = null; deletingIds.add(request.profileId)
    let attempted = false
    const operation = mutationTail.then(async () => {
      if (!current()) throw new Error('ACTION_CANCELLED')
      const profile = profiles.value.find(row => row.id === request.profileId)
      if (!profile || profile.revision !== request.profileRevision || revision.value !== request.workspaceRevision) throw new Error('REVISION_CONFLICT')
      if (isInUse(request.profileId)) throw new Error('PROFILE_IN_USE')
      try {
        attempted = true
        await execute(() => cliPatchProfile(request.workspaceRevision, { op: 'delete', id: request.profileId }))
      } catch (failure) {
        // The original writer retains queue ownership through read-only recovery.
        // A lost acknowledgement is never replayed or compensated by another write.
        try { await execute(cliListProfiles) } catch { throw new Error('RECOVERY_UNAVAILABLE') }
        throw failure
      }
    })
    mutationTail = operation.then(() => undefined, () => undefined)
    try {
      await operation
      if (current()) { closeDeleteConfirmation(); useNotificationsStore().pushToast({ kind: 'success', messageKey: 'feedbackConfigurationDeleted' }) }
      return true
    } catch (failure) {
      if (!attempted && safeUserErrorCode(failure) === 'REVISION_CONFLICT') {
        try { await load() } catch { failure = new Error('RECOVERY_UNAVAILABLE') }
      }
      if (current()) deleteError.value = mapSafeUserError(safeUserErrorCode(failure), 'settings')
      return false
    } finally { deletingIds.delete(request.profileId); deleteBusy.value = false }
  }

  const selectedIds = ref<Record<NativeCliKind, string | null>>({
    claude: null,
    codex: null,
  })

  let epoch = 0
  let initialized = false
  let mutationTail: Promise<void> = Promise.resolve()

  const byCli = computed<Record<NativeCliKind, CliProfile[]>>(() => ({
    claude: profiles.value.filter(item => item.cli === 'claude'),
    codex: profiles.value.filter(item => item.cli === 'codex'),
  }))

  const selected = computed<Record<NativeCliKind, CliProfile | null>>(() => ({
    claude: selectedProfile('claude'),
    codex: selectedProfile('codex'),
  }))

  function selectedProfile(cli: NativeCliKind): CliProfile | null {
    const rows = profiles.value.filter(item => item.cli === cli)
    const id = selectedIds.value[cli]
    return rows.find(item => item.id === id) ?? rows[0] ?? null
  }

  function reconcileSelection() {
    for (const cli of ['claude', 'codex'] as const) {
      const rows = profiles.value.filter(item => item.cli === cli)
      const current = selectedIds.value[cli]
      selectedIds.value[cli] = rows.some(item => item.id === current)
        ? current
        : (rows[0]?.id ?? null)
    }
  }

  function adopt(next: ProfileList) {
    if (initialized && parseU64(next.revision) < parseU64(revision.value)) return
    profiles.value = next.profiles
    revision.value = next.revision
    initialized = true
    reconcileSelection()
  }

  async function execute(operation: () => Promise<ProfileList>): Promise<ProfileList> {
    const current = ++epoch
    status.value = 'loading'
    try {
      const next = validateList(await operation())
      adopt(next)
      if (current === epoch) {
        status.value = 'loaded'
        lastError.value = null
      }
      return next
    } catch (error) {
      if (current === epoch) {
        status.value = 'error'
        lastError.value = safeUserErrorCode(error)
      }
      throw error
    }
  }

  function load(): Promise<ProfileList> {
    return execute(cliListProfiles)
  }

  function patch(expectedRevision: string, change: ProfilePatch): Promise<ProfileList> {
    if (change.op === 'delete') return Promise.reject(new Error('CONFIRMATION_REQUIRED'))
    if (isDeleting(change.op === 'create' ? change.profile.id : change.id)) return Promise.reject(new Error('PROFILE_IN_USE'))
    const next = mutationTail.then(() =>
      execute(() => cliPatchProfile(expectedRevision, change)),
    )
    mutationTail = next.then(() => undefined, () => undefined)
    return next
  }

  function select(cli: NativeCliKind, profileId: string): void {
    const found = profiles.value.find(item => item.id === profileId)
    if (!found) throw new Error('PROFILE_NOT_FOUND')
    if (found.cli !== cli) throw new Error('PROFILE_CLI_MISMATCH')
    selectedIds.value[cli] = profileId
  }

  function getProfile(profileId: string): CliProfile | undefined {
    return profiles.value.find(item => item.id === profileId)
  }

  return {
    profiles,
    deleteConfirmation, deleteError, deleteBusy, requestDelete, confirmDelete, closeDeleteConfirmation, isDeleting,
    revision,
    status,
    lastError,
    byCli,
    selected,
    load,
    patch,
    select,
    profile: getProfile,
  }
})
