import { describe, expect, it } from 'vitest'
import { isOrdinaryUpdateEligible, excludedArtifactChannel } from '@/utils/updatePolicy'

const admitted = () => ({
  channel: 'stable', installEligible: true, hasUpdate: true,
  version: '0.18.2', currentVersion: '0.18.1',
  admissionId: '8e111fa0-8baf-4ef2-8d2a-71be6e100321',
  officialRelease: { id: 123, tag: 'v0.18.2', sourceSha: 'a'.repeat(40) },
})

describe('ordinary updater admission', () => {
  it('allows an official backend admission instead of denying all releases', () => {
    expect(isOrdinaryUpdateEligible(admitted())).toBe(true)
  })
  it.each(['candidate', 'test-only', 'unverified'])('excludes %s even with forged stable fields', channel => {
    expect(isOrdinaryUpdateEligible({ ...admitted(), channel })).toBe(false)
  })
  it('does not treat stable labels or a caller boolean as an admission', () => {
    expect(isOrdinaryUpdateEligible({ channel: 'stable', hasUpdate: true, installEligible: true })).toBe(false)
    expect(isOrdinaryUpdateEligible({ ...admitted(), admissionId: undefined })).toBe(false)
  })
  it('binds the receipt to the observed version and full source', () => {
    expect(isOrdinaryUpdateEligible({ ...admitted(), officialRelease: { ...admitted().officialRelease, tag: 'v0.18.1' } })).toBe(false)
    expect(isOrdinaryUpdateEligible({ ...admitted(), officialRelease: { ...admitted().officialRelease, sourceSha: 'short' } })).toBe(false)
    expect(isOrdinaryUpdateEligible({ ...admitted(), installEligible: false })).toBe(false)
    expect(isOrdinaryUpdateEligible({ ...admitted(), hasUpdate: false })).toBe(false)
  })
  it.each([
    ['stable', 'stable', '0.27.0', true],
    ['stable', 'rc', '0.27.0-rc.1', false],
    ['test', 'rc', '0.27.0-rc.1', true],
    ['test', 'beta', '0.27.0-beta.2', true],
    ['test', 'stable', '0.27.0', true],
    ['test', 'preview', '0.28.0-preview.1', false],
    ['preview', 'preview', '0.28.0-preview.1', true],
    ['preview', 'rc', '0.27.0-rc.1', true],
    ['preview', 'alpha', '0.28.0-alpha.1', false],
    ['other', 'stable', '0.27.0', false],
    ['stable', 'stable', '0.27.0-rc.1', false],
    ['test', 'rc', '0.27.0-rc.01', false],
  ])('Update_ChannelMatrix_001 %s %s %s', (selectedChannel, channel, version, eligible) => {
    const info = { ...admitted(), selectedChannel, channel, version, officialRelease: { ...admitted().officialRelease, tag: `v${version}` } }
    expect(isOrdinaryUpdateEligible(info)).toBe(eligible)
  })
  it('Update_ArtifactExclusionIndependent_002', () => {
    for (const marker of [{ publishable: false }, { updaterPublication: false }]) {
      expect(isOrdinaryUpdateEligible({ ...admitted(), selectedChannel: 'preview', ...marker })).toBe(false)
    }
    expect(excludedArtifactChannel({ product: 'CC Desk', publishable: false, updaterPublication: false, channel: 'test-only' })).toBe('test-only')
    expect(isOrdinaryUpdateEligible({ ...admitted(), selectedChannel: 'test', channel: 'test-only' })).toBe(false)
  })
})
