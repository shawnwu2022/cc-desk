import { describe, expect, it } from 'vitest'
import {
  PERSIST_FAILED_CODE,
  isPersistFailedError,
  isStartupFailure,
  reduceWaiter,
} from '@/composables/useSessionStartWaiter'

describe('reduceWaiter', () => {
  it('Waiter_SessionStart_Started_001', () => {
    expect(reduceWaiter('waiting', { type: 'sessionStart' })).toBe('started')
  })

  it('Waiter_Timeout_MonitoringUnavailable_001', () => {
    expect(reduceWaiter('waiting', { type: 'timeout' })).toBe('unavailable')
  })

  it('Waiter_PtyExit_Exited_001', () => {
    expect(reduceWaiter('waiting', { type: 'ptyExit' })).toBe('exited')
  })

  it('Waiter_SpawnFail_Failed_001', () => {
    expect(reduceWaiter('waiting', { type: 'spawnFail' })).toBe('failed')
  })

  it('Waiter_Unmount_Cancelled_001', () => {
    expect(reduceWaiter('waiting', { type: 'unmount' })).toBe('cancelled')
  })

  it('Waiter_TerminalStatesAbsorbLaterEvents_001', () => {
    for (const state of ['started', 'unavailable', 'exited', 'failed', 'cancelled'] as const) {
      expect(reduceWaiter(state, { type: 'sessionStart' })).toBe(state)
      expect(reduceWaiter(state, { type: 'timeout' })).toBe(state)
    }
  })
})

describe('isStartupFailure', () => {
  it('MonitoringUnavailable_IsNotProcessFailure_001', () => {
    expect(isStartupFailure('unavailable')).toBe(false)
    expect(isStartupFailure('started')).toBe(false)
  })

  it('ProcessAndSpawnFailures_AreFailures_001', () => {
    expect(isStartupFailure('exited')).toBe(true)
    expect(isStartupFailure('failed')).toBe(true)
    expect(isStartupFailure('cancelled')).toBe(true)
  })
})

describe('isPersistFailedError', () => {
  it('IsPersistFailed_TaggedCode_001', () => {
    const error = new Error('persist_failed') as Error & { code: string }
    error.code = PERSIST_FAILED_CODE
    expect(isPersistFailedError(error)).toBe(true)
  })

  it('IsPersistFailed_UntaggedOrOtherValues_001', () => {
    expect(isPersistFailedError(new Error('spawn failed'))).toBe(false)
    expect(isPersistFailedError(null)).toBe(false)
    expect(isPersistFailedError(undefined)).toBe(false)
    expect(isPersistFailedError('persist_failed')).toBe(false)
    expect(isPersistFailedError({ code: 'other' })).toBe(false)
  })
})
