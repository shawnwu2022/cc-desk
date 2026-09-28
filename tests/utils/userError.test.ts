import { describe, expect, test } from 'vitest'
import { mapSafeUserError } from '@/utils/userError'

describe('mapSafeUserError', () => {
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
