import { afterEach, expect, it, vi } from 'vitest'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import type { LaunchRequest } from '@/types/cli'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'
import { mount } from '../fixtures/native-host-ui/entry'

const request: LaunchRequest = {
  requestId: 'fixture-request', tabId: 'fixture-tab', runId: 'fixture-run', generation: 1,
  cli: 'codex', launchCwd: '/isolated/project', profileId: 'fixture-profile',
  expectedProfileRevision: '1', action: { kind: 'raw', argv: ['public-fixture'] },
  extraArgs: [], cols: 80, rows: 24,
}
function receipt(): LaunchStatus {
  return { instanceId: 'fixture-instance', requestId: request.requestId,
    run: { runId: request.runId, generation: request.generation }, revision: '3', phase: 'running', failure: null }
}
function bridge(value: LaunchStatus) {
  const invoke = vi.fn().mockResolvedValue(value)
  vi.stubGlobal('__CC_DESK_DOCUMENT__', { invoke })
  vi.stubGlobal('__TAURI_INTERNALS__', { transformCallback: vi.fn().mockReturnValue(7) })
  return invoke
}
afterEach(() => { vi.unstubAllGlobals(); document.body.innerHTML = '' })

// 使用真实 store 和生产面板，数字型 LaunchRequest 字段不能混进 preparedIdentity。
it('HostUi_UsesExactLaunchReceipt_001', async () => {
  const invoke = bridge(receipt())
  await mount(request, 'public candidate')
  const tabs = useNativeTabsStore()
  expect(tabs.tab(request.tabId)).toMatchObject({ status: 'running', generation: 1, launchRevision: '3' })
  expect(tabs.hasFrozenLaunchReceipt(request.tabId)).toBe(true)
  expect(document.querySelector('.remote-host-panel button')?.hasAttribute('disabled')).toBe(false)
  expect(invoke).toHaveBeenCalledOnce()
  expect(invoke.mock.calls[0]?.slice(0, 2)).toEqual(['cli_start', request])
})

it.each(['request', 'run', 'generation'] as const)('HostUi_RejectsForeignReceipt_002 %s', async field => {
  const foreign = receipt()
  if (field === 'request') foreign.requestId = 'foreign'
  else if (field === 'run') foreign.run.runId = 'foreign'
  else foreign.run.generation++
  bridge(foreign)
  await expect(mount(request, 'public candidate')).rejects.toThrow('FIXTURE_LAUNCH_IDENTITY_MISMATCH')
  expect(useNativeTabsStore().tab(request.tabId)?.status).not.toBe('running')
  expect(document.querySelector('.remote-host-panel')).toBeNull()
})
