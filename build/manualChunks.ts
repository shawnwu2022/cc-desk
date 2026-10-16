export function manualChunkName(id: string): string | undefined {
  if (id.includes('@xterm/addon-webgl')) return undefined
  if (id.includes('@xterm')) return 'xterm-vendor'
  if (id.includes('node_modules')) return 'vendor'
  return undefined
}
