import { beforeEach, expect, it, vi } from 'vitest'
import { mockIPC } from '@tauri-apps/api/mocks'
import { open, message } from '@tauri-apps/plugin-dialog'
import { saveLastProject, selectDirectory } from '@/api/tauri'

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), message: vi.fn() }))

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(message).mockResolvedValue(undefined)
})

it('D07 explicitly saving an open project registers it before the legacy last-opened write', async () => {
  const calls: string[] = []
  mockIPC((command, args) => {
    calls.push(command)
    if (command === 'cli_register_project') {
      expect(args).toEqual({ selectedPath: '/synthetic/project' })
      return { revision: '1', projects: [], projectId: 'fixture-id' }
    }
    expect(command).toBe('save_last_project')
    expect(args).toEqual({ path: '/synthetic/project' })
  })
  await saveLastProject('/synthetic/project')
  expect(calls).toEqual(['cli_register_project', 'save_last_project'])
})

it('D07 selecting a directory persists its registration without waiting for any CLI', async () => {
  vi.mocked(open).mockResolvedValue('/synthetic/project')
  const calls: string[] = []
  mockIPC((command) => {
    calls.push(command)
    if (command === 'cli_register_project') return { revision: '1', projects: [], projectId: 'id' }
    throw new Error('no native CLI or history call allowed')
  })
  expect(await selectDirectory()).toEqual({ path: '/synthetic/project' })
  expect(calls).toEqual(['cli_register_project'])
})

it('D07 cancelled selection performs no persistence', async () => {
  vi.mocked(open).mockResolvedValue(null)
  const calls: string[] = []
  mockIPC((command) => { calls.push(command) })
  expect(await selectDirectory()).toBeNull()
  expect(calls).toEqual([])
})

it('D07 registration failure is displayed without opening or starting a CLI', async () => {
  vi.mocked(open).mockResolvedValue('/synthetic/project')
  mockIPC(() => { throw { code: 'STORAGE_IO', retryable: false } })
  expect(await selectDirectory()).toBeNull()
  expect(message).toHaveBeenCalledTimes(1)
})
