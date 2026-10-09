import { beforeEach, afterEach, expect, it } from 'vitest'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { cliListProfiles, cliPatchProfile } from '@/api/cli'

beforeEach(() => clearMocks())
afterEach(() => clearMocks())

it('D06_ProfileApi_ListDoesNotReadLegacyCredentials_01', async () => {
  const calls: string[] = []
  mockIPC((command) => {
    calls.push(command)
    return { revision: '0', profiles: [] }
  })
  expect(await cliListProfiles()).toEqual({ revision: '0', profiles: [] })
  expect(calls).toEqual(['cli_list_profiles'])
})

it('D06_ProfileApi_PatchPreservesExplicitUnset_02', async () => {
  let received: unknown
  mockIPC((command, args) => {
    expect(command).toBe('cli_patch_profile')
    received = args
    return { revision: '2', profiles: [] }
  })
  const patch = { op: 'update' as const, id: 'legacyClaude', changes: { skipPermissions: { mode: 'unset' as const } } }
  await cliPatchProfile('1', patch)
  expect(received).toEqual({ expectedRevision: '1', patch })
})

it('D06_ProfileApi_ConflictIsNotAutomaticallyRetried_03', async () => {
  let calls = 0
  mockIPC(() => {
    calls++
    throw { code: 'REVISION_CONFLICT', retryable: true }
  })
  await expect(cliPatchProfile('0', { op: 'delete', id: 'codex' }))
    .rejects.toMatchObject({ code: 'REVISION_CONFLICT' })
  expect(calls).toBe(1)
})
