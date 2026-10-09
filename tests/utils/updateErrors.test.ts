import { describe, expect, it } from 'vitest'
import { updateFailure } from '@/utils/updateErrors'
describe('updater safe diagnostics', () => {
  it('retains fixed stage/code instead of claiming every error is connectivity', () => {
    expect(updateFailure({ code: 'UPDATER_MANIFEST_INVALID', stage: 'check' })).toEqual({ code: 'UPDATER_MANIFEST_INVALID', stage: 'check', key: 'updateManifestInvalid' })
    expect(updateFailure({ code: 'UPDATER_REQUEST_FAILED', stage: 'provenance' }).key).toBe('updateRequestFailed')
  })
  it('does not expose arbitrary exceptions, fields, URLs or proxy credentials', () => {
    for (const error of [new Error('https://user:secret@proxy/private'), { code: 'TOKEN=secret', stage: '/private', message: 'private' }, 'raw']) {
      expect(updateFailure(error)).toEqual({ code: 'UPDATER_FAILED', stage: 'unknown', key: 'updateCheckSafeFailed' })
    }
  })
})
