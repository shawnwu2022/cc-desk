import { afterEach, describe, it, expect } from 'vitest'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { checkForUpdates } from '@/api/tauri'
import { isOrdinaryUpdateEligible } from '@/utils/updatePolicy'

afterEach(clearMocks)
const receipt = { version: '0.9.0', currentVersion: '0.8.0', hasUpdate: true, releaseNotes: 'Notes', platformAsset: null, installEligible: false }
describe('checkForUpdates host summary', () => {
  it('CheckForUpdates_NoUpdate_001', async () => {
    mockIPC(() => ({ ...receipt, version: '0.8.0', hasUpdate: false, releaseNotes: '' }))
    const result = await checkForUpdates()
    expect(result.hasUpdate).toBe(false); expect(result.version).toBe('0.8.0')
    expect(result.currentVersion).toBe('0.8.0'); expect(result.releaseNotes).toBe(''); expect(result.platformAsset).toBeNull()
  })
  it('CheckForUpdates_HasUpdate_001', async () => {
    mockIPC(() => receipt)
    expect(await checkForUpdates()).toEqual(receipt)
  })
  it('CheckForUpdates_NoBody_001', async () => {
    mockIPC(() => ({ ...receipt, releaseNotes: '' }))
    expect((await checkForUpdates()).releaseNotes).toBe('')
  })
  it.each(['stable', 'candidate', 'test-only'])('CheckForUpdates_ChannelExclusion_%s_004', async channel => {
    mockIPC(() => ({ ...receipt, channel, installEligible: true }))
    expect(isOrdinaryUpdateEligible(await checkForUpdates())).toBe(false)
  })
  it('CheckForUpdates_RetainsInstallCapabilityOnlyInHost_005', async () => {
    const commands: string[] = []
    mockIPC(command => { commands.push(command); return receipt })
    const result = await checkForUpdates()
    expect(commands).toEqual(['check_desktop_update'])
    expect(result).not.toHaveProperty('download'); expect(result).not.toHaveProperty('install'); expect(result).not.toHaveProperty('close')
  })
})
