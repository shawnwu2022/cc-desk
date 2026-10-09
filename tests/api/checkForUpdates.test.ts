import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock @tauri-apps/plugin-updater 的 check 函数
const mockCheck = vi.fn()
vi.mock('@tauri-apps/plugin-updater', () => ({
  check: (...args: unknown[]) => mockCheck(...args),
}))

// 模拟 __APP_VERSION__
vi.stubGlobal('__APP_VERSION__', '0.8.0')

import { checkForUpdates } from '@/api/tauri'

describe('checkForUpdates 返回值', () => {
  beforeEach(() => {
    mockCheck.mockReset()
  })

  // check() 返回 null 时，返回 hasUpdate: false 的完整对象
  it('CheckForUpdates_NoUpdate_001', async () => {
    mockCheck.mockResolvedValue(null)
    const result = await checkForUpdates()
    expect(result.hasUpdate).toBe(false)
    expect(result.version).toBe('0.8.0')
    expect(result.currentVersion).toBe('0.8.0')
    expect(result.releaseNotes).toBe('')
    expect(result.platformAsset).toBeNull()
  })

  // check() 返回 update 对象时，hasUpdate 为 true，字段正确映射
  it('CheckForUpdates_HasUpdate_001', async () => {
    mockCheck.mockResolvedValue({
      version: '0.9.0',
      body: '### Features\n- New feature',
      currentVersion: '0.8.0',
    })
    const result = await checkForUpdates()
    expect(result.hasUpdate).toBe(true)
    expect(result.version).toBe('0.9.0')
    expect(result.currentVersion).toBe('0.8.0')
    expect(result.releaseNotes).toBe('### Features\n- New feature')
  })

  // update.body 为 undefined 时，releaseNotes 降级为空字符串
  it('CheckForUpdates_NoBody_001', async () => {
    mockCheck.mockResolvedValue({
      version: '1.0.0',
      body: undefined,
      currentVersion: '0.8.0',
    })
    const result = await checkForUpdates()
    expect(result.hasUpdate).toBe(true)
    expect(result.releaseNotes).toBe('')
  })
  // 常规版本号/正文/单独的stable标记不能证明准入，现有测试包标记只能证明排除。
  it.each(['stable', 'candidate', 'test-only'])('CheckForUpdates_ChannelExclusion_%s_004', async channel => {
    mockCheck.mockResolvedValue({ version: '99.0.0', body: 'stable release', rawJson: { product: 'CC Desk', channel, publishable: false, updaterPublication: false } })
    const result = await checkForUpdates()
    expect(result.installEligible).toBe(false)
    expect(result.channel).toBe(channel === 'stable' ? 'unverified' : channel)
  })
  // 普通检查释放仅供读取的updater资源，不下载或安装。
  it('CheckForUpdates_ReleasesReadResource_005', async () => {
    const close = vi.fn().mockResolvedValue(undefined)
    mockCheck.mockResolvedValue({ version: '99.0.0', body: '', rawJson: { channel: 'stable' }, close })
    const result = await checkForUpdates()
    expect(result.channel).toBe('unverified'); expect(close).toHaveBeenCalledTimes(1)
  })

})
