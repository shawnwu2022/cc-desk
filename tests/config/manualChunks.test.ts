import { describe, expect, it } from 'vitest'
import { manualChunkName } from '../../build/manualChunks'

describe('manualChunkName', () => {
  // WebGL addon 走动态 import，manualChunks 不分到静态 chunk。
  it('ManualChunks_WebglDynamicSplit_001', () => {
    expect(manualChunkName('C:/repo/node_modules/@xterm/addon-webgl/lib/addon-webgl.mjs'))
      .toBeUndefined()
  })

  // xterm core 与通用 vendor 保持既有 chunk 归属不变。
  it('ManualChunks_XtermVendorStable_002', () => {
    expect(manualChunkName('C:/repo/node_modules/@xterm/xterm/lib/xterm.mjs'))
      .toBe('xterm-vendor')
    expect(manualChunkName('C:/repo/node_modules/vue/dist/vue.runtime.esm.js'))
      .toBe('vendor')
    expect(manualChunkName('C:/repo/src/App.vue')).toBeUndefined()
  })
})
