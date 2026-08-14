import { describe, expect, it } from 'vitest'
import { manualChunkName } from '../../build/manualChunks'

describe('manualChunkName', () => {
  // WebGL addon 走动态 import，manualChunks 不分到静态 chunk。
  it('ManualChunks_WebglDynamicSplit_001', () => {
    expect(manualChunkName('C:/repo/node_modules/@xterm/addon-webgl/lib/addon-webgl.mjs'))
      .toBeUndefined()
  })

  // CodeMirror/Lezer/vue-codemirror 归 editor-vendor（延迟加载的编辑器依赖）。
  it('ManualChunks_EditorDeferredVendor_002', () => {
    expect(manualChunkName('C:/repo/node_modules/@codemirror/view/dist/index.js'))
      .toBe('editor-vendor')
    expect(manualChunkName('C:/repo/node_modules/@lezer/common/dist/index.js'))
      .toBe('editor-vendor')
    expect(manualChunkName('C:/repo/node_modules/vue-codemirror/dist/index.js'))
      .toBe('editor-vendor')
    expect(manualChunkName('C:/repo/node_modules/codemirror/dist/index.js'))
      .toBe('editor-vendor')
  })

  // xterm core 与通用 vendor 保持既有 chunk 归属不变。
  it('ManualChunks_XtermVendorStable_003', () => {
    expect(manualChunkName('C:/repo/node_modules/@xterm/xterm/lib/xterm.mjs'))
      .toBe('xterm-vendor')
    expect(manualChunkName('C:/repo/node_modules/vue/dist/vue.runtime.esm.js'))
      .toBe('vendor')
    expect(manualChunkName('C:/repo/src/App.vue')).toBeUndefined()
  })
})
