import { createPinia, setActivePinia } from 'pinia'
"
        "import { beforeEach, describe, expect, test } from 'vitest'
"
        "import { useUpdateStore } from '@/stores/update'

"
        "describe('update store', () => {
"
        "  beforeEach(() => setActivePinia(createPinia()))

"
        "  test('tracks only CC Desk update download state', () => {
"
        "    const store = useUpdateStore()
"
        "    store.setDownloadState('downloading')
"
        "    store.setDownloadProgress({ downloaded: 25, total: 100, percent: 25 })
"
        "    expect(store.downloadState).toBe('downloading')
"
        "    expect(store.downloadProgress.percent).toBe(25)
"
        "    expect('claudeVersionList' in store).toBe(false)
"
        "  })

"
        "  test('resets transient update state', () => {
"
        "    const store = useUpdateStore()
"
        "    store.setDownloadState('error')
"
        "    store.setDownloadError('failed')
"
        "    store.resetDownload()
"
        "    expect(store.downloadState).toBe('idle')
"
        "    expect(store.downloadError).toBe('')
"
        "  })
"
        "})
