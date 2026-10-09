// Architectural checks supplement (not replace) the native WebView integration tests.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
describe('D12 production boundary', () => {
  it('registers both authenticated production commands without a native delete', () => {
    const lib = readFileSync('src-tauri/src/lib.rs', 'utf8')
    expect(lib).toContain('commands::native_get_scope')
    expect(lib).toContain('commands::native_list_resources')
    expect(lib).not.toContain('commands::native_delete_resource')
  })
  it('authenticates native documents before decoding both new requests', () => {
    const source = readFileSync('src-tauri/src/cli/native_runtime.rs', 'utf8')
    for (const name of ['projection_scope', 'projection_read']) {
      const part = source.slice(source.indexOf(`fn ${name}<`)).split('\n    pub(crate)')[0]
      expect(part.indexOf('admit_native')).toBeGreaterThan(0)
      expect(part.indexOf('admit_native')).toBeLessThan(part.indexOf('decode_projection'))
    }
  })
})
