import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mockIPC } from '@tauri-apps/api/mocks'
import { useWorkspaceStore } from '@/stores/workspace'

const project = (projectId: string, selectedPath: string) => ({
  projectId, selectedPath, hostId: 'local', sourcePathKey: `key:${projectId}`,
  canonicalPath: null,
  alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' },
})

beforeEach(() => setActivePinia(createPinia()))

describe('D07 independent project registry', () => {
  it('loads persisted projects without calling native history APIs', async () => {
    const calls: string[] = []
    mockIPC((command) => {
      calls.push(command)
      if (command === 'cli_list_projects') {
        return { revision: '2', projects: [project('a', '/Project'), project('b', '/project')] }
      }
      throw new Error('native history must not be consulted')
    })
    const store = useWorkspaceStore()
    await store.load()
    expect(store.projects).toHaveLength(2)
    expect(store.revision).toBe('2')
    expect(calls).toEqual(['cli_list_projects'])
  })

  it('preserves the last valid registry when reload fails', async () => {
    mockIPC(() => ({ revision: '1', projects: [project('a', '/a')] }))
    const store = useWorkspaceStore()
    await store.load()
    mockIPC(() => { throw { code: 'WORKSPACE_INVALID', retryable: false } })
    await expect(store.load()).rejects.toMatchObject({ code: 'WORKSPACE_INVALID' })
    expect(store.projects).toHaveLength(1)
    expect(store.status).toBe('error')
  })

  it('does not overwrite newer registration with a late list reply', async () => {
    let finish!: (value: unknown) => void
    mockIPC((command) => {
      if (command === 'cli_list_projects') return new Promise(resolve => { finish = resolve })
      if (command === 'cli_register_project') {
        return { revision: '2', projectId: 'a', projects: [project('a', '/a')] }
      }
      throw new Error('unexpected command')
    })
    const store = useWorkspaceStore()
    const reading = store.load()
    await store.register('/a')
    finish({ revision: '1', projects: [] })
    await reading
    expect(store.revision).toBe('2')
    expect(store.projects).toHaveLength(1)
  })

  it('reports conflicts without replaying metadata mutation', async () => {
    let mutations = 0
    mockIPC((command) => {
      if (command === 'cli_list_projects') return { revision: '1', projects: [project('a', '/a')] }
      mutations++
      throw { code: 'REVISION_CONFLICT', retryable: true }
    })
    const store = useWorkspaceStore()
    await store.load()
    await expect(store.patch('a', { pinned: { mode: 'set', value: false } }))
      .rejects.toMatchObject({ code: 'REVISION_CONFLICT' })
    expect(mutations).toBe(1)
    expect(store.projects).toHaveLength(1)
  })

  it('removes only a registry entry and preserves exact backend identity', async () => {
    const calls: string[] = []
    mockIPC((command, args) => {
      calls.push(command)
      if (command === 'cli_list_projects') return { revision: '1', projects: [project('a', '/A')] }
      expect(command).toBe('cli_remove_project')
      expect(args).toEqual({ projectId: 'a', expectedRevision: '1' })
      return { revision: '2', projects: [] }
    })
    const store = useWorkspaceStore()
    await store.load()
    await store.remove('a')
    expect(store.projects).toEqual([])
    expect(calls).toEqual(['cli_list_projects', 'cli_remove_project'])
  })
})
