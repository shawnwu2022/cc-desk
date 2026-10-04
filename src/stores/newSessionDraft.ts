import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { useAppStore } from './app'
import { useProjectsStateStore } from './projectsState'
import { useCliProfilesStore } from './cliProfiles'
import { useNativeTabsStore, type NativeCliTab } from './nativeTabs'
import { cliGetAvailability } from '@/api/cliAvailability'
import { parseNativeRawArgv } from '@/utils/nativeRawArgv'
import { normalizePath } from '@/utils/path'
import { LaunchConfigurationRequiredError } from '@/utils/launchPreparation'
import type { CliProfile } from '@/types/profile'
import type { CreateUnifiedSessionInput, UnifiedCliKind, UnifiedProjectIdentity } from '@/types/unifiedSession'

const preferenceKey = 'cc-desk-launch-preferences-v1'
interface Preferences { defaults: Partial<Record<UnifiedCliKind, string>> }
function readPreferences(): Preferences {
  try {
    const value = JSON.parse(localStorage.getItem(preferenceKey) ?? 'null')
    if (value && typeof value.defaults === 'object' && value.defaults) {
      return { defaults: Object.fromEntries(Object.entries(value.defaults).filter(([cli, id]) => ['claude', 'codex'].includes(cli) && typeof id === 'string')) }
    }
  } catch { /* Optional UI preference storage must never prevent a launch. */ }
  return { defaults: {} }
}
export function argvFromLines(value: string): string[] {
  return value === '' ? [] : value.replace(/\r\n/g, '\n').split('\n')
}
export type NewSessionStartMode = 'new' | 'history' | 'resume-picker' | 'resume-id'

/** UI draft and configuration preparation only; no process launch or permission override. */
export const useNewSessionDraftStore = defineStore('new-session-draft', () => {
  const profiles = useCliProfilesStore()
  const projects = useProjectsStateStore()
  const tabs = useNativeTabsStore()
  const preferences = ref(readPreferences())
  const visible = ref(false)
  const chooserVisible = ref(false)
  const project = ref<UnifiedProjectIdentity | null>(null)
  const cli = ref<UnifiedCliKind>('claude')
  const title = ref('')
  const launchConfigId = ref('')
  const startMode = ref<NewSessionStartMode>('new')
  const rawEnabled = ref(false)
  const argvFormat = ref<'lines' | 'json'>('lines')
  const argvText = ref('')
  const preflight = ref<Record<string, { state: 'unknown' | 'unavailable'; observedFailures: string[] }>>({})
  const failureIdentity = (tab: NativeCliTab) => JSON.stringify([tab.tabId, tab.requestId, tab.runId, tab.generation, tab.errorCode])
  let availabilityEpoch = 0
  const configKey = (profile: CliProfile) => JSON.stringify([profile.id, profile.revision])
  async function refreshAvailability() {
    const epoch = ++availabilityEpoch
    const snapshot = profiles.profiles.filter(profile => profile.cli !== 'shell').map(profile => ({ ...profile }))
    for (let i = 0; i < snapshot.length; i += 2) {
      if (epoch !== availabilityEpoch) return
      await Promise.all(snapshot.slice(i, i + 2).map(async profile => {
        const observedFailures = [...tabs.tabs.values()].filter(tab => tab.profileId === profile.id && tab.profileRevision === profile.revision && tab.errorCode === 'PROGRAM_UNAVAILABLE').map(failureIdentity)
        let state: 'unknown' | 'unavailable' = 'unknown'
        try {
          const result = await cliGetAvailability(profile.id, profile.revision)
          if (result.cli !== profile.cli) return
          if (result.state === 'unavailable') state = 'unavailable'
        } catch { return /* Failed reads cannot override previous evidence. */ }
        if (epoch === availabilityEpoch && profiles.profile(profile.id)?.revision === profile.revision) preflight.value[configKey(profile)] = { state, observedFailures }
      }))
    }
  }
  function savePreferences() { try { localStorage.setItem(preferenceKey, JSON.stringify(preferences.value)) } catch { /* In-memory preferences remain usable. */ } }
  function preferred(identity: Pick<UnifiedProjectIdentity, 'projectPath'>, tool: UnifiedCliKind): CliProfile | null {
    const canonical = projects.launchPreferences.get(normalizePath(identity.projectPath))
    const recent = tool === 'claude' ? canonical?.claudeLaunchConfigId : canonical?.codexLaunchConfigId
    const ids = [recent, preferences.value.defaults[tool]]
    for (const id of ids) {
      const found = id ? profiles.profile(id) : undefined
      if (found?.cli === tool) return found
    }
    return profiles.selected[tool]
  }
  function setDefault(tool: UnifiedCliKind, id: string) {
    if (profiles.profile(id)?.cli !== tool) throw new Error('PROFILE_CLI_MISMATCH')
    preferences.value.defaults[tool] = id; savePreferences()
  }
  function defaultFor(tool: UnifiedCliKind): CliProfile | null {
    const selected = profiles.profile(preferences.value.defaults[tool] ?? '')
    return selected?.cli === tool ? selected : profiles.selected[tool]
  }
  function forgetDefault(id: string): void {
    for (const tool of ['claude', 'codex'] as const) {
      if (preferences.value.defaults[tool] !== id) continue
      const replacement = profiles.byCli[tool].find(row => row.id !== id)
      if (replacement) preferences.value.defaults[tool] = replacement.id
      else delete preferences.value.defaults[tool]
    }
    savePreferences()
  }
  async function recordSuccess(path: string, tool: UnifiedCliKind, id: string): Promise<void> {
    if (profiles.profile(id)?.cli !== tool) return
    try { await projects.setLaunchPreference(path, tool, id) }
    catch {
      // The canonical writer already reconciles under queue ownership.
      // Never enqueue recovery here after another success save can overtake it.
      throw new Error('NEW_SESSION_PREFERENCE_SAVE_FAILED')
    }
  }
  function availabilityFor(identity?: Pick<UnifiedProjectIdentity, 'projectPath'>) { return Object.fromEntries((['claude', 'codex'] as const).map(tool => {
    const config = identity ? preferred(identity, tool) : profiles.selected[tool]
    const evidence = [...tabs.tabs.values()].reverse().filter(tab => tab.cli === tool && tab.profileId === config?.id && tab.profileRevision === config?.revision)
      .sort((a, b) => b.lastActivityAt - a.lastActivityAt)
    const latest = evidence[0]
    const checked = config ? preflight.value[configKey(config)] : undefined
    const failureStillCurrent = latest?.errorCode === 'PROGRAM_UNAVAILABLE' && !checked?.observedFailures.includes(failureIdentity(latest))
    return [tool, latest?.status === 'running' ? 'available' : failureStillCurrent ? 'unavailable' : checked?.state ?? 'unknown']
  })) as Record<UnifiedCliKind, 'unknown' | 'available' | 'unavailable'> }
  const cliAvailability = computed(() => availabilityFor(project.value ?? undefined))
  function resetDraft(identity: UnifiedProjectIdentity, tool: UnifiedCliKind, advanced: boolean) {
    project.value = { projectKey: identity.projectKey, projectPath: identity.projectPath }
    cli.value = tool; title.value = ''; launchConfigId.value = ''; startMode.value = 'new'
    rawEnabled.value = false; argvFormat.value = 'lines'; argvText.value = ''; visible.value = advanced; chooserVisible.value = !advanced
  }
  function open(identity: UnifiedProjectIdentity, tool: UnifiedCliKind = useAppStore().defaultNewCli) { resetDraft(identity, tool, true) }
  function openChooser(identity: UnifiedProjectIdentity) { resetDraft(identity, useAppStore().defaultNewCli, false) }
  function argv(): string[] { return argvFormat.value === 'json' ? parseNativeRawArgv(argvText.value) : argvFromLines(argvText.value) }
  function setArgvFormat(format: 'lines' | 'json') {
    if (format === argvFormat.value) return
    const values = argv()
    if (format === 'lines' && (values.some(value => /[\r\n]/.test(value)) || (values.length === 1 && values[0] === ''))) throw new Error('ARGV_REQUIRES_JSON')
    argvText.value = format === 'json' ? JSON.stringify(values) : values.join('\n')
    argvFormat.value = format
  }
  function toInput(): CreateUnifiedSessionInput {
    if (!project.value) throw new Error('PROJECT_REQUIRED')
    if (startMode.value !== 'new') throw new Error('RESTORE_FLOW_REQUIRED')
    const selected = launchConfigId.value ? profiles.profile(launchConfigId.value) : projects.loaded ? preferred(project.value, cli.value) : null
    if (launchConfigId.value && selected?.cli !== cli.value) throw new Error('PROFILE_CLI_MISMATCH')
    const args = rawEnabled.value ? argv() : null
    if (args?.some(value => value.includes('\0'))) throw new Error('INVALID_RAW_ARGV_JSON')
    return { ...project.value, cli: cli.value, title: title.value.trim() || undefined,
      ...(selected ? { launchConfigId: selected.id, launchConfigRevision: selected.revision } : {}),
      action: args ? { kind: 'raw', argv: args } : { kind: 'new' } }
  }
  async function prepareInput(input: CreateUnifiedSessionInput): Promise<CreateUnifiedSessionInput> {
    try {
      await projects.ensureLoaded()
      const usedCachedProfiles = profiles.status === 'loaded'
      if (!usedCachedProfiles) await profiles.load()
      let selected = input.launchConfigId ? profiles.profile(input.launchConfigId) : preferred(input, input.cli)
      if (input.launchConfigId && (!selected || selected.cli !== input.cli || (input.launchConfigRevision && selected.revision !== input.launchConfigRevision))) throw new Error('PROFILE_SELECTION_CHANGED')
      if (!selected && usedCachedProfiles) {
        // Projects and profiles share the backend workspace CAS. Registration may
        // have advanced it since this cache was read; refresh before the first write.
        await profiles.load()
        selected = preferred(input, input.cli)
      }
      if (!selected) {
        const safe: CliProfile = { id: `desk-safe-${input.cli}`, revision: '0', cli: input.cli,
          name: input.cli === 'claude' ? 'Claude Code' : 'Codex CLI', launcher: { kind: 'native' },
          programPath: { mode: 'inherit' }, defaultArgs: { mode: 'set', value: [] },
          skipPermissions: input.cli === 'claude' ? { mode: 'set', value: false } : { mode: 'inherit' },
          observer: { mode: 'set', value: false }, env: {} }
        try { await profiles.patch(profiles.revision, { op: 'create', profile: safe }) }
        catch { await profiles.load().catch(() => undefined); throw new Error('NEW_SESSION_PREPARATION_FAILED') }
        selected = profiles.profile(safe.id)
      }
      if (!selected || selected.cli !== input.cli) throw new Error('NEW_SESSION_PREPARATION_FAILED')
      const profileId = selected.id, profileRevision = selected.revision
      const availability = await cliGetAvailability(profileId, profileRevision)
      if (profiles.profile(profileId)?.revision !== profileRevision || availability.cli !== input.cli) {
        throw new Error('PROFILE_SELECTION_CHANGED')
      }
      if (availability.state !== 'available-unverified') throw new LaunchConfigurationRequiredError(profileId, availability.issue)
      if (availability.hostStatus === 'unavailable') throw new Error('NEW_SESSION_PREPARATION_FAILED')
      return { ...input, launchConfigId: selected.id, launchConfigRevision: selected.revision }
    } catch (failure) {
      if (failure instanceof LaunchConfigurationRequiredError) throw failure
      throw new Error('NEW_SESSION_PREPARATION_FAILED')
    }
  }
  return { visible, chooserVisible, project, cli, title, launchConfigId, startMode, rawEnabled, argvFormat, argvText,
    cliAvailability, availabilityFor, refreshAvailability, preferred, setDefault, defaultFor, forgetDefault, recordSuccess, open, openChooser, setArgvFormat, toInput, prepareInput }
})
