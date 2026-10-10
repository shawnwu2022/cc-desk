import { afterEach, describe, expect, it } from 'vitest'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import * as api from '@/api/tauri'

afterEach(clearMocks)
describe('desktop updater host API', () => {
  // 检测使用独立只读命令，既不保存配置也不调用更新安装准入。
  it('ProxyTest_ReadOnlyDraftCommand_001', async () => {
    const calls: [string, unknown][] = []
    mockIPC((command, args) => { calls.push([command, args]); return { elapsedMs: 32, mode: 'custom' } })
    expect(await api.testUpdaterProxy('http://localhost:1080')).toEqual({ elapsedMs: 32, mode: 'custom' })
    expect(calls).toEqual([['test_updater_proxy', { proxy: 'http://localhost:1080' }]])
  })
  it('uses the host admission rather than a frontend stable claim', async () => {
    const commands: string[] = []
    const receipt = { version: '0.18.2', currentVersion: '0.18.1', hasUpdate: true, channel: 'stable', installEligible: true, admissionId: 'host-owned' }
    mockIPC(command => { commands.push(command); return command === 'check_desktop_update' ? receipt : null })
    expect(await api.checkForUpdates()).toEqual(receipt)
    expect(commands).toEqual(['check_desktop_update'])
  })
  it('persists only the explicit updater setting and installs by retained admission ID', async () => {
    const calls: [string, unknown][] = []
    mockIPC((command, args) => { calls.push([command, args]); return command === 'get_updater_settings' ? { proxy: null } : undefined })
    expect(await api.getUpdaterSettings()).toEqual({ proxy: null })
    await api.saveUpdaterSettings('http://localhost:1080')
    await api.installDesktopUpdate('admission')
    expect(calls).toEqual([
      ['get_updater_settings', {}],
      ['save_updater_settings', { proxy: 'http://localhost:1080' }],
      ['install_desktop_update', { admissionId: 'admission' }],
    ])
  })
})
