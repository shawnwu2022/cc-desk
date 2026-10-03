import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { useUnifiedSessionsStore } from './unifiedSessions'
import { useNativeTabsStore } from './nativeTabs'
import { useSessionStore } from './session'
import { useCliProfilesStore } from './cliProfiles'
import { useWorkspaceStore } from './workspace'
import { useNativeProjectionStore } from './nativeProjection'
import { useConfigStore } from './config'
import { useSidebarStore } from './sidebar'
import { sameProjectPath } from '@/utils/path'
import { projectionErrorCode } from '@/api/nativeProjection'
import { hasExactProjectResourceSource, isResourceProjectPath, projectResourceItems } from '@/utils/projectResources'
import type { ResourceItem } from '@/types/nativeProjection'
import type { ProjectResourceContext, ProjectResourceItem, ProjectResourceKind } from '@/types/projectResources'

export const useProjectResourcesStore = defineStore('project-resources', () => {
  const sessions = useUnifiedSessionsStore()
  const native = useNativeTabsStore()
  const legacy = useSessionStore()
  const profiles = useCliProfilesStore()
  const projects = useWorkspaceStore()
  const projection = useNativeProjectionStore()
  const config = useConfigStore()
  const sidebar = useSidebarStore()
  const active = ref(false)
  const kind = ref<ProjectResourceKind>('instructions')
  const items = ref<ProjectResourceItem[]>([])
  const loading = ref(false), stale = ref(false), unavailable = ref(false), partial = ref(false)
  const error = ref<string | null>(null)
  const hasSession = computed(() => !!sessions.activeSession)
  const context = computed<ProjectResourceContext | null>(() => {
    const session = sessions.activeSession
    if (!session || !isResourceProjectPath(session.projectPath) || !session.projectKey) return null
    if (session.runtime === 'legacy-claude') {
      if (session.cli !== 'claude') return null
      const tab = legacy.tabs.get(session.adapterSessionId)
      if (!tab && (!session.id.startsWith('legacy-history:') || session.processState !== 'stopped')) return null
      if (tab && (!sameProjectPath(tab.projectPath, session.projectPath) || tab.cli && tab.cli !== 'claude')) return null
      return { runtime: 'legacy-claude', sessionId: session.id, projectId: session.projectKey, projectPath: session.projectPath, cli: 'claude',
        attempt: JSON.stringify([session.adapterSessionId, tab?.ptyId, tab?.ptyGeneration]) }
    }
    const tab = native.tab(session.adapterSessionId)
    // A disappeared active terminal is not historical configured-profile authority.
    if (!tab && (!session.id.startsWith('native-history:') || session.processState !== 'stopped')) return null
    if (tab && (tab.cli !== session.cli || !sameProjectPath(tab.projectPath, session.projectPath))) return null
    const origin = tab ?? session.nativeOrigin
    if (!origin || origin.cli !== session.cli || !sameProjectPath(origin.projectPath, session.projectPath)) return null
    const base = { runtime: 'native-cli' as const, sessionId: session.id, projectId: origin.projectId, projectPath: session.projectPath, cli: session.cli,
      profileId: origin.profileId, profileRevision: origin.profileRevision,
      attempt: JSON.stringify([session.adapterSessionId, tab?.requestId, tab?.runId, tab?.generation]) }
    if (tab && (!native.hasUnstartedAttempt(tab.tabId) || tab.launchRevision !== null || tab.status !== 'stopped')) {
      // Even an unavailable/revoked run remains a run request; never fallback.
      return { ...base, target: { kind: 'run', runId: tab.runId, generation: tab.generation } }
    }
    const profile = profiles.profile(origin.profileId)
    const registered = projects.projects.find(row => row.projectId === origin.projectId)
    if (!profile || profile.cli !== session.cli || profile.revision !== origin.profileRevision || !registered
      || ![registered.selectedPath, registered.canonicalPath].some(path => path && sameProjectPath(path, session.projectPath))) return null
    return { ...base, target: { kind: 'profile', profileId: profile.id, expectedProfileRevision: profile.revision, projectId: registered.projectId } }
  })
  const legacyProjectOnly = computed(() => context.value?.runtime === 'legacy-claude')
  const identity = computed(() => JSON.stringify([sessions.activeSessionId, context.value, kind.value]))
  let owner: object = {}
  let loadedIdentity: string | null = null

  function reset() {
    owner = {}; loadedIdentity = null; items.value = []; loading.value = false
    stale.value = false; unavailable.value = false; partial.value = false; error.value = null
  }
  async function refresh(): Promise<void> {
    const selected = context.value
    const key = identity.value
    const resourceKind = kind.value
    if (!active.value) return
    if (!selected) { reset(); unavailable.value = hasSession.value; error.value = hasSession.value ? 'SOURCE_UNAVAILABLE' : null; return }
    const token = owner = {}
    loading.value = true; stale.value = loadedIdentity === key
    unavailable.value = false; error.value = null
    const current = () => owner === token && active.value && identity.value === key
    try {
      let rows: ResourceItem[]
      let incomplete = false
      if (selected.runtime === 'native-cli') {
        const result = await projection.readScoped(selected.target, resourceKind, selected, current)
        if (!current()) return
        if (result.state === 'unavailable') throw { code: result.reason }
        rows = result.items; incomplete = result.hasMore
      } else {
        incomplete = true // Legacy contracts prove project-only observations, not a launch-root snapshot.
        if (resourceKind === 'config' || resourceKind === 'mcp') {
          const result = await config.readProjectConfig(selected.projectPath)
          if (resourceKind === 'config') {
            rows = result.basic.filter(row => hasExactProjectResourceSource(selected.projectPath, row.source, 'config')).slice(0, 200)
              .filter(row => typeof row.model === 'string').map(row => ({ type: 'setting', name: 'model', value: row.model!, origin: row.source.type }))
          } else {
            // getAllMcpServers labels ancestor .mcp.json files as "project" but
            // omits their paths. Only the existing config DTO proves exact origin.
            rows = result.mcp.filter(row => hasExactProjectResourceSource(selected.projectPath, row.source, 'mcp')).slice(0, 200)
              .map(row => ({ type: 'mcp', name: row.name, transport: row.type ?? 'unknown', origin: 'project' }))
          }
        } else rows = await sidebar.readProjectResources(selected.projectPath, resourceKind)
      }
      if (!current()) return
      items.value = projectResourceItems(rows)
      partial.value = incomplete
      stale.value = false; loadedIdentity = key
    } catch (failure) {
      if (!current()) return
      error.value = projectionErrorCode(failure); unavailable.value = true
      stale.value = loadedIdentity === key
    } finally { if (current()) loading.value = false }
  }
  function setActive(value: boolean) {
    if (active.value === value) return
    active.value = value
    if (value) void refresh()
    else { owner = {}; loading.value = false; stale.value = loadedIdentity === identity.value }
  }
  watch(identity, () => { reset(); if (active.value) void refresh() }, { flush: 'sync' })
  return { context, kind, items, loading, stale, unavailable, partial, error, hasSession, legacyProjectOnly, setActive, refresh }
})
