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
import type { ProjectActionRequest, UnifiedProjectGroup, UnifiedProjectIdentity, UnifiedSession } from '@/types/unifiedSession'

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
  const dialog = ref<{ kind: 'rename' | 'remove' | 'close-sessions'; project: UnifiedProjectIdentity } | null>(null)
  const renameValue = ref('')
  const renameError = ref<string | null>(null)
  // 行投影缓存：与 catalog.sessions 的行对象身份联动，避免任何单行变化让整表重渲染。
  const projectedRows = new WeakMap<UnifiedSession, { resumePending: boolean; row: UnifiedSession }>()
  // 组投影缓存：键为规范化 projectKey，值是上一次发布的组对象。
  const groupCache = new Map<string, UnifiedProjectGroup>()
  let feedbackVersion = 0
  let projectCloseOwner: { request: NonNullable<typeof dialog.value>; registration: string | null; close: ReturnType<typeof catalog.captureProjectClose> } | null = null
  let removeOwner: { request: NonNullable<typeof dialog.value>; registration?: string | null; selected: () => boolean } | null = null
  function registration(path: string): string | null {
    const matches = registry.projects.filter(project => sameProjectPath(project.selectedPath, path))
    if (matches.length > 1) throw new Error('PROJECT_IDENTITY_CHANGED')
    const row = matches[0]
    return row ? JSON.stringify([row.projectId, row.hostId, row.sourcePathKey, row.selectedPath, row.canonicalPath]) : null
  }
  let refreshTail: Promise<void> | null = null

  function openSessionIds(path: string): string[] {
    // Open stopped terminals still own runtime/profile/project identity. A stale
    // catalog saying "ended" cannot authorize unregistering their project.
    return [...new Set([
      ...[...legacy.tabs.values()].filter(tab => sameProjectPath(tab.projectPath, path)).map(tab => `legacy-tab:${tab.tabId}`),
      ...[...native.tabs.values()].filter(tab => sameProjectPath(tab.projectPath, path)).map(tab => `native-tab:${tab.tabId}`),
      ...catalog.sessions.filter(row => sameProjectPath(row.projectPath, path) && catalog.isPreparingSession(row.id)).map(row => row.id),
    ])]
  }
  function openCount(path: string): number { return openSessionIds(path).length }
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
      // 行投影复用同一对象：catalog 每次发布都会重建行对象，这里若再展开新对象，
      // 任何单行状态变化都会让所有 SessionItem 的 prop 换身份、整表重渲染。
      const resumePending = catalog.isResumePending(row)
      let projected = projectedRows.get(row)
      if (!projected || projected.resumePending !== resumePending) {
        projected = { resumePending, row: { ...row, resumePending } }
        projectedRows.set(row, projected)
      }
      if (!row.archived) group.sessions.push(projected.row)
      if (row.processState === 'running' || row.processState === 'starting') group.runningCount++
      if (row.attentionState === 'needs-user') group.needsUserCount++
    }
    // 组投影缓存：行对象身份已由 projectedRows 稳定，这里对未变化的项目复用
    // 同一组对象，单行状态变化不再让无关项目的 ProjectNode 重渲染。
    for (const [key, group] of rows) {
      const previous = groupCache.get(key)
      if (!previous
        || previous.projectPath !== group.projectPath || previous.name !== group.name
        || previous.pinned !== group.pinned || previous.hidden !== group.hidden
        || previous.lastActivityAt !== group.lastActivityAt
        || previous.runningCount !== group.runningCount || previous.needsUserCount !== group.needsUserCount
        || previous.sessions.length !== group.sessions.length
        || group.sessions.some((row, index) => row !== previous.sessions[index])) continue
      rows.set(key, previous)
    }
    groupCache.clear()
    for (const [key, group] of rows) groupCache.set(key, group)
    return [...rows.values()].sort((a, b) => Number(b.pinned) - Number(a.pinned)
      || b.lastActivityAt - a.lastActivityAt || a.name.localeCompare(b.name) || a.projectKey.localeCompare(b.projectKey))
  })
  const visibleGroups = computed(() => groups.value.filter(group => !group.hidden || openCount(group.projectPath) > 0)
    .map(group => group.hidden ? { ...group, hidden: false } : group))

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
  function beginCloseSessions(project: UnifiedProjectIdentity) {
    error.value = null
    try {
      const proof = registration(project.projectPath)
      const close = catalog.captureProjectClose(project.projectPath, openSessionIds(project.projectPath))
      dialog.value = { kind: 'close-sessions', project: { ...project } }
      projectCloseOwner = { request: dialog.value, registration: proof, close }
    } catch { error.value = 'resumeConfigurationChanged' }
  }
  async function closeSessions() {
    const owner = projectCloseOwner
    if (!owner || busy.value || dialog.value !== owner.request) return
    const current = () => projectCloseOwner === owner && dialog.value === owner.request
    const canContinue = () => current() && registration(owner.request.project.projectPath) === owner.registration
    busy.value = true; error.value = null
    try {
      await owner.close(canContinue)
      if (current()) closeDialog()
    } catch {
      if (current()) error.value = 'projectCloseSessionsPartial'
    } finally { busy.value = false }
  }
  function closeDialog() { ++feedbackVersion; dialog.value = null; removeOwner = null; projectCloseOwner = null; renameError.value = null; error.value = null }
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
    if (request.action === 'close-project-sessions') { beginCloseSessions(request); return }
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
    openCount, refresh, add, action, setHidden, beginRename, beginRemove, beginCloseSessions, closeSessions, closeDialog, rename, remove }
})
