export function manualChunkName(id: string): string | undefined {
  if (id.includes('@xterm/addon-webgl')) return undefined
  if (
    id.includes('@codemirror')
    || id.includes('@lezer')
    || id.includes('vue-codemirror')
    || id.includes('/node_modules/codemirror/')
  ) {
    return 'editor-vendor'
  }
  if (id.includes('@xterm')) return 'xterm-vendor'
  if (id.includes('node_modules')) return 'vendor'
  return undefined
}
