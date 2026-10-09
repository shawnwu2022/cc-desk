import { describe, expect, it } from 'vitest'
import { isOrdinaryUpdateEligible } from '@/utils/updatePolicy'

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
})
