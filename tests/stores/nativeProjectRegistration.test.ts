import { readFileSync } from 'node:fs'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useCliWorkspaceStore } from '@/stores/cliWorkspace'

function profile() {
  return {
    id: 'codex-main',
    revision: '9',
    cli: 'codex' as const,
    name: 'Codex',
    launcher: { kind: 'native' as const },
    programPath: { mode: 'inherit' as const },
    defaultArgs: { mode: 'inherit' as const },
    skipPermissions: { mode: 'inherit' as const },
    observer: { mode: 'inherit' as const },
    env: {},
  }
}

function projectList(include = false) {
  return {
    revision: include ? '2' : '1',
    projects: include
      ? [{
          projectId: 'project-new',
          hostId: 'host-new',
          sourcePathKey: 'root-new',
          selectedPath: '/repo/new',
          canonicalPath: '/repo/new',
          alias: { mode: 'inherit' },
          pinned: { mode: 'inherit' },
          hidden: { mode: 'inherit' },
        }]
      : [],
    metadata: {},
    warnings: [],
    ...(include ? { projectId: 'project-new' } : {}),
  }
}

beforeEach(() => {
  clearMocks()
  setActivePinia(createPinia())
  const profiles = useCliProfilesStore()
  profiles.profiles = [profile()] as any
  profiles.revision = '1'
  profiles.select('codex', 'codex-main')
})

afterEach(() => clearMocks())

describe('D28 native project registration', () => {
  it('D28_Project_AddButtonUsesRegistrationMutation_01', () => {
    const source = readFileSync('src/components/NativeCliWorkbench.vue', 'utf8')
    const start = source.indexOf('async function addProject()')
    const end = source.indexOf('\n}\n\nfunction createNew()', start)
    expect(start).toBeGreaterThan(-1)
    expect(end).toBeGreaterThan(start)
    const body = source.slice(start, end)
    expect(body).toContain('workbench.workspace.registerProject(result.path)')
    expect(body).not.toContain('workbench.workspace.open(')
  })

  it('D28_Project_SelectedDirectoryIsPersistedAndAdopted_02', async () => {
    const calls: Array<[string, unknown]> = []
    mockIPC((command, payload) => {
      calls.push([command, payload])
      if (command === 'cli_list_projects') return projectList(false)
      if (command === 'cli_register_project') {
        expect(payload).toEqual({ selectedPath: '/repo/new' })
        return projectList(true)
      }
      throw new Error(`unexpected:${command}`)
    })

    const workspace = useCliWorkspaceStore()
    await workspace.open('codex')
    const projectId = await workspace.registerProject('/repo/new')

    expect(projectId).toBe('project-new')
    expect(workspace.projects.map(item => item.projectId)).toEqual(['project-new'])
    expect(workspace.status).toBe('ready')
    expect(workspace.error).toBeNull()
    expect(calls.map(([command]) => command)).toEqual([
      'cli_list_projects',
      'cli_register_project',
    ])
  })

  it('D28_Project_RegistrationFailureExposesOnlySafeCode_03', async () => {
    mockIPC(command => {
      if (command === 'cli_list_projects') return projectList(false)
      if (command === 'cli_register_project') {
        throw { code: 'COMMIT_STATE_UNKNOWN', message: 'C:\\private\\secret.json' }
      }
      throw new Error(`unexpected:${command}`)
    })

    const workspace = useCliWorkspaceStore()
    await workspace.open('codex')
    await expect(workspace.registerProject('/repo/new')).rejects.toMatchObject({
      code: 'COMMIT_STATE_UNKNOWN',
    })

    expect(workspace.status).toBe('ready')
    expect(workspace.error).toBe('COMMIT_STATE_UNKNOWN')
    expect(workspace.error).not.toContain('private')
  })
})
