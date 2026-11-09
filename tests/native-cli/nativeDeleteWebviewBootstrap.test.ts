import { afterEach, expect, it, vi } from 'vitest'

afterEach(() => {
  vi.restoreAllMocks()
  delete (window as any).__STAGE11__
  document.body.innerHTML = ''
})

it('PermanentDeleteWebView_ModuleDoesNotWriteOrMountBeforeNativeOwnershipProof_001', async () => {
  document.body.innerHTML = '<div id="app"></div>'
  const write = vi.spyOn(Storage.prototype, 'setItem')
  let importError: unknown
  await import('../native-delete-webview/main').catch(error => { importError = error })
  expect(write).not.toHaveBeenCalled()
  expect(document.querySelector('#app')?.childElementCount).toBe(0)
  expect(importError).toBeUndefined()
  expect(typeof (window as any).__STAGE11__?.start).toBe('function')
})
