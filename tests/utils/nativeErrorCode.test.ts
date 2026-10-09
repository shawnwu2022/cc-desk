import { describe, expect, it } from 'vitest'
import { publicNativeErrorCode } from '@/utils/nativeErrorCode'

describe('D26 native public error boundary', () => {
  it('preserves only fixed public codes', () => {
    expect(publicNativeErrorCode({ code: 'FORBIDDEN' })).toBe('FORBIDDEN')
    expect(publicNativeErrorCode(new Error('LAUNCH_STATE_UNKNOWN'))).toBe('LAUNCH_STATE_UNKNOWN')
  })

  it('never reflects arbitrary messages or secret-looking codes', () => {
    expect(publicNativeErrorCode(new Error('C:\\Users\\private\\token.txt')))
      .toBe('NATIVE_OPERATION_FAILED')
    expect(publicNativeErrorCode({ code: 'FIXTURE_SECRET_DO_NOT_RENDER' }))
      .toBe('NATIVE_OPERATION_FAILED')
    expect(publicNativeErrorCode({ code: 'INVALID_REQUEST', message: 'fixture-secret' }))
      .toBe('INVALID_REQUEST')
  })
})
