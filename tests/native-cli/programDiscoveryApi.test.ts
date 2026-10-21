import { afterEach, expect, it } from 'vitest'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { cliDiscoverPrograms } from '@/api/programDiscovery'
afterEach(clearMocks)
const response = () => ({ profileId: 'p', profileRevision: '2', workspaceRevision: '3', projectId: 'project', cli: 'codex', candidates: [{ programPath: 'C:\\Tools\\codex.cmd', launcher: { kind: 'shim', runner: 'C:\\Windows\\System32\\cmd.exe', dialect: 'cmd' }, env: { SECRET: 'DO_NOT_COPY' } }] })
it('ProgramDiscoveryApi_IdentityBoundAndCopiesOnlyCandidatePaths_001', async () => {
  mockIPC((command, payload) => {
    expect(command).toBe('cli_discover_programs')
    expect(payload).toEqual({ request: { profileId: 'p', expectedRevision: '2', projectId: 'project' } })
    return response()
  })
  const result = await cliDiscoverPrograms('p', '2', 'project')
  expect(result.candidates[0].launcher.kind).toBe('shim')
  expect(JSON.stringify(result)).not.toMatch(/SECRET|DO_NOT_COPY/)
})
it.each(['profileId', 'profileRevision', 'projectId'])('ProgramDiscoveryApi_RejectsForeignIdentity_002: %s', async key => {
  mockIPC(() => ({ ...response(), [key]: 'other' }))
  await expect(cliDiscoverPrograms('p', '2', 'project')).rejects.toThrow('DISCOVERY_UNAVAILABLE')
})
it.each(['relative/codex', 'C:\\Tools\\codex\0.exe'])('ProgramDiscoveryApi_RejectsInvalidPath_003: %s', async programPath => {
  mockIPC(() => ({ ...response(), candidates: [{ programPath, launcher: { kind: 'native' } }] }))
  await expect(cliDiscoverPrograms('p', '2', 'project')).rejects.toThrow('DISCOVERY_UNAVAILABLE')
})
// 后端 CliKind 已扩至 opencode：discovery 白名单必须接受，未知 CLI 仍整体拒绝。
it('ProgramDiscoveryApi_AcceptsOpencodeAndRejectsUnknownCli_004', async () => {
  mockIPC(() => ({ ...response(), cli: 'opencode' }))
  expect((await cliDiscoverPrograms('p', '2', 'project')).cli).toBe('opencode')
  mockIPC(() => ({ ...response(), cli: 'futurecli' }))
  await expect(cliDiscoverPrograms('p', '2', 'project')).rejects.toThrow('DISCOVERY_UNAVAILABLE')
})
