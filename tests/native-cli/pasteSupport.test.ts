import { beforeEach, expect, it, vi } from 'vitest'
import { codexNativeTextPasteForAttempt } from '@/api/cliPasteSupport'
const io = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => io.invoke(...args) }))
beforeEach(() => { vi.clearAllMocks() })

it('CodexCompatibility_FreshAttemptRechecksUnchangedProfile_074', async () => {
  io.invoke.mockResolvedValueOnce({ profileId: 'replacement-profile', nativeTextPaste: true })
    .mockResolvedValueOnce({ profileId: 'replacement-profile', nativeTextPaste: false, futureField: true })
  expect(await codexNativeTextPasteForAttempt('replacement-profile', '1')).toBe(true)
  expect(await codexNativeTextPasteForAttempt('replacement-profile', '1')).toBe(false)
  expect(io.invoke).toHaveBeenCalledTimes(2)
})

it('CodexCompatibility_PendingProbeNeverCrossesAttempts_078', async () => {
  let settleOld!: (value: unknown) => void
  io.invoke.mockReturnValueOnce(new Promise(resolve => { settleOld = resolve }))
    .mockResolvedValueOnce({ profileId: 'pending-profile', nativeTextPaste: false })
  const old = codexNativeTextPasteForAttempt('pending-profile', '1')
  const next = codexNativeTextPasteForAttempt('pending-profile', '1')
  expect(io.invoke).toHaveBeenCalledTimes(2)
  expect(await next).toBe(false)
  settleOld({ profileId: 'pending-profile', nativeTextPaste: true })
  expect(await old).toBe(true)
})
