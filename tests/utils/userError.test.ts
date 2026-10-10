import { describe, expect, test } from 'vitest'
import { mapSafeUserError, safeUserErrorCode } from '@/utils/userError'

describe('mapSafeUserError', () => {
  test('SourceBudget_ExplicitRetry_002', () => {
    const code = safeUserErrorCode({ code: 'SOURCE_BUDGET_EXCEEDED', field: '/private/history', message: 'secret' })
    const result = mapSafeUserError(code, 'session')
    expect(result).toMatchObject({ detailCode: 'SOURCE_BUDGET_EXCEEDED', actionKey: 'retry', retryable: true, severity: 'warning' })
    expect(JSON.stringify(result)).not.toMatch(/private|secret/)
  })
  // 来源忙碌保留固定码并提供显式重试，原始错误字段不能进入提示。
  test('SourceBusy_ExplicitRetry_001', () => {
    const code = safeUserErrorCode({ code: 'SOURCE_BUSY', field: '/private', message: 'secret' })
    const result = mapSafeUserError(code, 'session')
    expect(result).toMatchObject({ detailCode: 'SOURCE_BUSY', actionKey: 'retry', retryable: true, severity: 'warning' })
    expect(JSON.stringify(result)).not.toMatch(/private|secret/)
  })
  test('maps safe internal codes to user-facing message keys', () => {
    expect(mapSafeUserError('REVISION_CONFLICT', 'workspace').messageKey)
      .toBe('errorRevisionConflict')
    expect(mapSafeUserError('CLI_NOT_FOUND', 'launch').messageKey)
      .toBe('errorCliNotFound')
  })

  test('does not reflect arbitrary error strings', () => {
    const result = mapSafeUserError('token=super-secret C:\\Users\\name', 'resource')
    expect(result.messageKey).toBe('errorGenericUnavailable')
    expect(JSON.stringify(result)).not.toContain('super-secret')
    expect(JSON.stringify(result)).not.toContain('Users')
  })
})
