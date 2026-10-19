import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'node:path'

const root = resolve(__dirname, '../../..')
const host = resolve(__dirname, 'host.ts')

// 独立浏览器入口只替换宿主边界；生产组件、xterm、FitAddon 与样式均保持真实实现。
export default defineConfig(({ command, mode }) => {
  if (command !== 'serve' || mode !== 'terminal-fit') throw new Error('TERMINAL_FIT_FIXTURE_DISABLED')
  return {
  root,
  cacheDir: '.cache/terminal-fit-vite',
  plugins: [{
    name: 'terminal-fit-host', enforce: 'pre',
    resolveId(source) {
      source = source.replace(/\\/g, '/')
      if (source.endsWith('/terminal/nativeLaunchEntry') || source.endsWith('/terminal/deskNativeTerminal') || source.endsWith('/terminal/nativeLaunchEntry.ts') || source.endsWith('/terminal/deskNativeTerminal.ts')) return host
      if (source.startsWith('@tauri-apps/')) return host
      if (source.endsWith('/src/api/tauri') || source === '@/api/tauri') return '\0terminal-fit-api'
    },
    load(id) {
      if (id === '\0terminal-fit-api') return `export * from ${JSON.stringify(resolve(root, 'src/api/tauri.ts'))}; export { cliResize } from ${JSON.stringify(host)};`
    },
  }, vue()],
  resolve: { alias: { '@': resolve(root, 'src') } },
  define: { __APP_VERSION__: JSON.stringify('terminal-fit-test'), __APP_BUILD_COMMIT__: JSON.stringify('test') },
  server: { host: '127.0.0.1', port: 4175, strictPort: true },
  }
})
