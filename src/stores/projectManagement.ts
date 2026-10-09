import { computed, ref } from 'vue'
import { mapSafeUserError, safeUserErrorCode } from '@/utils/userError'
import { useNotificationsStore, type ToastInput } from '@/stores/notifications'
import { defineStore } from 'pinia'
import { useAppStore } from '@/stores/app'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useWorkspaceStore } from '@/stores/workspace'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useSessionStore } from '@/stores/session'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { normalizePath, sameProjectPath } from '@/utils/path'
import { projectBasename, validateDisplayName } from '@/utils/displayName'
import { openInFileManager, selectDirectory } from '@/api/tauri'
import type { ProjectActionRequest, UnifiedProjectGroup, UnifiedProjectIdentity } from '@/types/unifiedSession'

/** Management never owns a process and never deletes CLI history or project files. */
export const useProjectManagementStore = defineStore('project-management', () => {
  const app = useAppStore()
  const state = useProjectsStateStore()
  const registry = useWorkspaceStore()
  const catalog = useUnifiedSessionsStore()
  const legacy = useSessionStore()
  const native = useNativeTabsStore()
  const error = ref<string | null>(null)
  const busy = ref(false)
  const loading = ref(false)
  const dialog = ref<{ kind: 'rename'; project: UnifiedProjectIdentity } | { kind: 'remove'; project: UnifiedProjectIdentity } | null>(null)
  const renameValue = ref('')
  const renameError = ref<string | null>(null)
  let feedbackVersion = 0
  let removeOwner: { request: NonNullable<typeof dialog.value>; registration?: string | null; selected: () => boolean } | null = null
  function registration(path: string): string | null {
    const matches = registry.projects.filter(project => sameProjectPath(project.selectedPath, path))
    if (matches.length > 1) throw new Error('PROJECT_IDENTITY_CHANGED')
    const row = matches[0]
    return row ? JSON.stringify([row.projectId, row.hostId, row.sourcePathKey, row.selectedPath, row.canonicalPath]) : null
  }
  let refreshTail: Promise<void> | null = null

  function openCount(path: string): number {
    // Open stopped terminals still own runtime/profile/project identity. A stale
    // catalog saying "ended" cannot authorize unregistering their project.
    return [...legacy.tabs.values()].filter(tab => sameProjectPath(tab.projectPath, path)).length
      + [...native.tabs.values()].filter(tab => sameProjectPath(tab.projectPath, path)).length
      + catalog.sessions.filter(row => sameProjectPath(row.projectPath, path) && catalog.isPreparingSession(row.id)).length
  }
  const groups = computed<UnifiedProjectGroup[]>(() => {
    const rows = new Map<string, UnifiedProjectGroup>()
    const pinned = new Set(state.pinnedProjects.map(normalizePath))
    function add(path: string, name?: string, activity = 0) {
      const key = normalizePath(path)
      if (!rows.has(key)) rows.set(key, { projectKey: key, projectPath: path,
        name: state.displayNames.get(key) || state.displayNames.get(path) || name || projectBasename(path),
        sessions: [], pinned: pinned.has(key), hidden: app.isHidden(path), runningCount: 0,
        needsUserCount: 0, lastActivityAt: activity })
    }
    for (const project of registry.projects) add(project.selectedPath)
    for (const project of app.cachedProjects) add(project.path, project.name)
    for (const path of state.pinnedProjects) add(path)
    for (const path of app.hiddenProjects) add(path)
    for (const path of state.archivedSessions.keys()) add(path)
    for (const row of catalog.sessions) {
      add(row.projectPath)
      const group = rows.get(normalizePath(row.projectPath))!
      group.lastActivityAt = Math.max(group.lastActivityAt, row.lastActivityAt)
      if (!row.archived) group.sessions.push(row)
      if (row.processState === 'running' || row.processState === 'starting') group.runningCount++
      if (row.attentionState === 'needs-user') group.needsUserCount++
    }
    return [...rows.values()].sort((a, b) => Number(b.pinned) - Number(a.pinned)
      || b.lastActivityAt - a.lastActivityAt || a.name.localeCompare(b.name) || a.projectKey.localeCompare(b.projectKey))
  })
  const visibleGroups = computed(() => groups.value.filter(group => !group.hidden || openCount(group.projectPath) > 0)
    .map(group => ({ ...group, hidden: false })))

  function refresh(): Promise<void> {
    if (refreshTail) return refreshTail
    loading.value = true; error.value = null
    refreshTail = (async () => {
      const results = await Promise.allSettled([() => app.loadManagedProjects(), () => app.loadProjectVisibility(), () => state.ensureLoaded(), () => registry.load()].map(operation => Promise.resolve().then(async () => { await operation() })))
      if (results.some(result => result.status === 'rejected')) error.value = 'projectsPartialAvailability'
    })().finally(() => { loading.value = false; refreshTail = null })
    return refreshTail
  }
  async function run<T>(operation: () => Promise<T>, successKey?: ToastInput['messageKey']): Promise<T | undefined> {
    if (busy.value) return
    const version = ++feedbackVersion
    const ownsSelection = catalog.captureSelectionOwnership()
    const current = () => version === feedbackVersion && ownsSelection()
    busy.value = true; error.value = null
    try {
      const result = await operation()
      if (current() && successKey) useNotificationsStore().pushToast({ kind: 'success', messageKey: successKey })
      return result
    }
    catch (failure) {
      const code = safeUserErrorCode(failure)
      if (current()) error.value = code === 'PROJECT_HAS_OPEN_SESSIONS' ? 'projectRemoveOpenSessions'
        : failure instanceof Error && failure.message === 'PROJECT_ACTION_UNAVAILABLE' ? 'projectHideCurrentUnavailable'
          : ['REVISION_CONFLICT', 'PROJECT_IDENTITY_CHANGED'].includes(code) ? mapSafeUserError(code, 'workspace').messageKey : 'projectManagementActionFailed'
      // Multi-store changes are not atomic. Read-only reconciliation is the only
      // automatic recovery, even when an acknowledgement is lost after commit.
      if (code !== 'PROJECT_HAS_OPEN_SESSIONS' && code !== 'ACTION_CANCELLED' && !(failure instanceof Error && failure.message === 'PROJECT_ACTION_UNAVAILABLE')) {
        const results = await Promise.allSettled([() => registry.load(), () => state.reload(), () => app.loadProjectVisibility(true)].map(operation => Promise.resolve().then(async () => { await operation() })))
        if (current() && results.some(result => result.status === 'rejected')) error.value = 'projectManagementReloadFailed'
      }
      return undefined
    } finally { busy.value = false }
  }
  async function add(path?: string): Promise<UnifiedProjectIdentity | undefined> {
    return run(async () => {
      const selected = path ? { path } : await selectDirectory({ register: false })
      if (!selected) return undefined
      return app.addManagedProject(selected.path)
    })
  }
  function beginRename(project: UnifiedProjectIdentity) {
    renameValue.value = groups.value.find(group => sameProjectPath(group.projectPath, project.projectPath))?.name ?? projectBasename(project.projectPath)
    renameError.value = null; dialog.value = { kind: 'rename', project: { ...project } }
  }
  function beginRemove(project: UnifiedProjectIdentity) {
    if (openCount(project.projectPath) > 0) { error.value = 'projectRemoveOpenSessions'; return }
    error.value = null
    dialog.value = { kind: 'remove', project: { ...project } }
    try { removeOwner = { request: dialog.value, registration: registry.status === 'loaded' ? registration(project.projectPath) : undefined, selected: catalog.captureSelectionOwnership() } }
    catch { closeDialog(); error.value = 'resumeConfigurationChanged' }
  }
  function closeDialog() { ++feedbackVersion; dialog.value = null; removeOwner = null; renameError.value = null; error.value = null }
  async function rename() {
    const target = dialog.value
    if (target?.kind !== 'rename' || busy.value) return
    const validation = validateDisplayName(renameValue.value)
    if (!validation.ok) { renameError.value = validation.error === 'tooLong' ? 'aliasTooLong' : 'aliasInvalidChars'; return }
    const saved = await run(async () => { await state.setProjectDisplayName(target.project.projectPath, renameValue.value); return true }, 'feedbackProjectRenamed')
    if (saved && dialog.value === target) closeDialog()
  }
  async function remove() {
    const target = dialog.value
    if (target?.kind !== 'remove' || busy.value) return
    const path = target.project.projectPath
    const owner = removeOwner
    const requireCurrent = (expectedRegistration = owner?.registration) => {
      if (!owner || removeOwner !== owner || dialog.value !== target || !owner.selected()) throw new Error('ACTION_CANCELLED')
      if (expectedRegistration !== undefined && registration(path) !== expectedRegistration) throw new Error('PROJECT_IDENTITY_CHANGED')
      if (openCount(path) > 0) throw new Error('PROJECT_HAS_OPEN_SESSIONS')
    }
    const removed = await run(async () => {
      requireCurrent()
      app.markProjectRemoving(path, true)
      try {
        await app.loadProjectVisibility()
        await state.ensureLoaded()
        if (registry.status !== 'loaded') await registry.load()
        if (owner && owner.registration === undefined) owner.registration = registration(path)
        requireCurrent()
        // Suppress rediscovery first; if unregister is uncertain, report partial
        // completion with no destructive compensation and no automatic replay.
        await app.setManagedHidden(path, true, requireCurrent)
        if (!app.isHidden(path)) throw new Error('PROJECT_ACTION_UNAVAILABLE')
        requireCurrent()
        const matches = registry.projects.filter(project => sameProjectPath(project.selectedPath, path))
        if (matches.length > 1) throw new Error('PROJECT_IDENTITY_CHANGED')
        if (matches[0]) await registry.remove(matches[0].projectId, requireCurrent)
        // The exact registration is now absent. Do not clean up metadata for a
        // replacement registration or a canceled confirmation after queueing.
        requireCurrent(null)
        if (state.pinnedProjects.some(pinned => sameProjectPath(pinned, path))) await state.unpinProject(path, () => requireCurrent(null))
        // Archive/display/launch preferences are intentionally kept for re-add.
        return true
      } finally { app.markProjectRemoving(path, false) }
    }, 'feedbackProjectRemoved')
    if (removed && dialog.value === target) closeDialog()
  }
  async function action(request: ProjectActionRequest) {
    if (busy.value) return
    if (request.action === 'rename') { beginRename(request); return }
    if (request.action === 'remove-project') { beginRemove(request); return }
    if (request.action === 'view-archive') return // The tree already owns its archive drawer.
    await run(async () => {
      if (request.action === 'pin') await state.pinProject(request.projectPath)
      else if (request.action === 'unpin') await state.unpinProject(request.projectPath)
      else await openInFileManager(request.projectPath)
    }, request.action === 'pin' ? 'feedbackPinned' : request.action === 'unpin' ? 'feedbackUnpinned' : undefined)
  }
  async function setHidden(project: UnifiedProjectIdentity, hidden: boolean) {
    await run(async () => {
      if (hidden && openCount(project.projectPath) > 0) throw new Error('PROJECT_HAS_OPEN_SESSIONS')
      if (hidden) app.markProjectVisibilityChanging(project.projectPath, true)
      try {
        await app.setManagedHidden(project.projectPath, hidden, () => {
          if (hidden && openCount(project.projectPath) > 0) throw new Error('PROJECT_HAS_OPEN_SESSIONS')
        })
        if (app.isHidden(project.projectPath) !== hidden) throw new Error('PROJECT_ACTION_UNAVAILABLE')
      } finally { if (hidden) app.markProjectVisibilityChanging(project.projectPath, false) }
    })
  }
  return { groups, visibleGroups, error, busy, loading, dialog, renameValue, renameError,
    openCount, refresh, add, action, setHidden, beginRename, beginRemove, closeDialog, rename, remove }
})
