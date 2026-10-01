import { afterEach, beforeEach, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import type { CliProfile } from '@/types/profile'
import type { SessionAdapter } from '@/types/unifiedSession'
import contracts from '../fixtures/native-cli/launch-preflight.json'

let rows: CliProfile[], commands: string[]
beforeEach(() => {
  localStorage.clear(); setActivePinia(createPinia()); clearMocks(); rows = []; commands = []
  mockIPC((command, payload: any) => {
    commands.push(command)
    if (command === 'get_projects_state') return { pinnedProjects: [], archivedSessions: {} }
    if (command === 'cli_list_profiles') return { revision: '1', profiles: rows }
    if (command === 'cli_patch_profile') {
      const contract = contracts.find(entry => entry.profile.cli === payload.patch.profile.cli)!
      expect(payload.patch.profile).toEqual(contract.profile)
      rows.push({ ...payload.patch.profile, revision: contract.availability.profileRevision })
      return { revision: '2', profiles: rows }
    }
    if (command === 'cli_get_availability') {
      const selected = rows.find(row => row.id === payload.request.profileId)!
      // The same fixture is checked against real Rust get_availability.
      return contracts.find(entry => entry.profile.id === selected.id)!.availability
    }
    throw new Error(`Unexpected command ${command}`)
  })
})
afterEach(clearMocks)

// This joins the real profile store, availability decoder and creation admission.
// Returning from preparation before checking availability would create a tab here.
it.each(['claude', 'codex'] as const)('LaunchPreparation_DefaultRequiresProgramBeforeAdmission_001: %s', async cli => {
  const draft = useNewSessionDraftStore(), catalog = useUnifiedSessionsStore(), tabs = useNativeTabsStore()
  catalog.configureCreationPreparer(draft.prepareInput)
  catalog.configureAdapters([{
    runtime: 'native-cli', listSessions: async () => [],
    createSession: async (input: any) => {
      const tab = tabs.create({ cli: input.cli, projectId: 'project', projectPath: input.projectPath,
        profileId: input.launchConfigId, profileRevision: input.launchConfigRevision, action: { kind: 'new' } })
      return { id: `native-tab:${tab.tabId}`, adapterSessionId: tab.tabId, runtime: 'native-cli', cli,
        projectKey: '/repo', projectPath: '/repo', title: cli, processState: 'stopped', attentionState: 'none',
        lastActivityAt: 0, archived: false, resumable: false }
    },
  } as unknown as SessionAdapter])
  await expect(catalog.createSession({ cli, projectKey: '/repo', projectPath: '/repo' }))
    .rejects.toThrow('LAUNCH_CONFIGURATION_REQUIRED')
  expect(tabs.tabs.size).toBe(0)
  expect(catalog.sessions[0]).toMatchObject({ processState: 'failed', safeErrorCode: 'LAUNCH_CONFIGURATION_REQUIRED', launchConfigId: `desk-safe-${cli}` })
  expect(useCliProfilesStore().profile(`desk-safe-${cli}`)?.programPath).toEqual({ mode: 'inherit' })
  expect(commands.filter(command => command === 'cli_get_availability')).toHaveLength(1)
  expect(commands).not.toContain('cli_start')
  await catalog.closeSession(catalog.sessions[0].id)
  expect(catalog.sessions).toHaveLength(0)
})
