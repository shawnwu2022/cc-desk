import { afterEach, describe, expect, it, vi } from 'vitest'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { cliGetAvailability } from '@/api/cliAvailability'

const ready = {
  profileId: 'codex', profileRevision: '1', cli: 'codex',
  state: 'available-unverified', hostStatus: 'available', certified: false,
}

afterEach(() => clearMocks())

describe('D08 per-profile availability boundary', () => {
  it('sends only profile identity and revision without legacy checks', async () => {
    const calls: unknown[] = []
    mockIPC((command, args) => {
      calls.push([command, args])
      return ready
    })
    expect(await cliGetAvailability('codex', '1')).toEqual(ready)
    expect(calls).toEqual([['cli_get_availability', {
      request: { profileId: 'codex', expectedRevision: '1' },
    }]])
  })

  it('keeps unavailable host separate and never grants certification', async () => {
    const response = { ...ready, hostStatus: 'unavailable' }
    mockIPC(() => response)
    expect(await cliGetAvailability('codex', '1')).toEqual(response)
    mockIPC(() => ({ ...ready, certified: true }))
    await expect(cliGetAvailability('codex', '1')).rejects.toThrow('INVALID_AVAILABILITY_RESPONSE')
  })

  it('rejects stale or cross-profile responses', async () => {
    for (const change of [{ profileId: 'claude' }, { profileRevision: '2' }]) {
      mockIPC(() => ({ ...ready, ...change }))
      await expect(cliGetAvailability('codex', '1')).rejects.toThrow('INVALID_AVAILABILITY_RESPONSE')
    }
  })

  it('does not propagate extra environment fields into application state', async () => {
    mockIPC(() => ({ ...ready, env: { TOKEN: 'private-secret' }, argv: ['private-secret'] }))
    expect(await cliGetAvailability('codex', '1')).toEqual(ready)
  })

  it('rejects invalid input before invoking the backend', async () => {
    const invoked = vi.fn(() => ready)
    mockIPC(invoked)
    for (const revision of ['01', '-1', '18446744073709551616']) {
      await expect(cliGetAvailability('codex', revision)).rejects.toThrow('INVALID_AVAILABILITY_REQUEST')
    }
    await expect(cliGetAvailability('../private-secret', '1')).rejects.toThrow('INVALID_AVAILABILITY_REQUEST')
    expect(invoked).not.toHaveBeenCalled()
  })

  it('propagates sanitized backend conflict without automatic retry', async () => {
    const issue = { code: 'REVISION_CONFLICT', retryable: true }
    const invoked = vi.fn(() => Promise.reject(issue))
    mockIPC(invoked)
    await expect(cliGetAvailability('codex', '1')).rejects.toEqual(issue)
    expect(invoked).toHaveBeenCalledOnce()
  })
})
