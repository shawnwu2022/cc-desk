import { beforeEach, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mockIPC } from '@tauri-apps/api/mocks'
import { useWorkspaceStore } from '@/stores/workspace'

const project = {
  projectId: 'a', hostId: 'local', sourcePathKey: 'verified-key', selectedPath: '/a',
  canonicalPath: '/a', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' },
}

beforeEach(() => setActivePinia(createPinia()))

it('D07 rejects a registration receipt missing its id before adopting the new state', async () => {
  mockIPC(() => ({ revision: '1', projects: [project] }))
  const store = useWorkspaceStore()
  await store.load()
  mockIPC(() => ({ revision: '2', projects: [] }))
  await expect(store.register('/b')).rejects.toMatchObject({ code: 'INVALID_WORKSPACE_RESPONSE' })
  expect(store.revision).toBe('1')
  expect(store.projects).toHaveLength(1)
  expect(store.status).toBe('error')
})

it('D07 malformed list keeps the last valid registry and never logs payloads', async () => {
  mockIPC(() => ({ revision: '1', projects: [project] }))
  const store = useWorkspaceStore()
  await store.load()
  mockIPC(() => ({ revision: '02', projects: [] }))
  await expect(store.load()).rejects.toMatchObject({ code: 'INVALID_WORKSPACE_RESPONSE' })
  expect(store.revision).toBe('1')
  expect(store.projects).toHaveLength(1)
})
