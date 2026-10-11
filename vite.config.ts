import { execFileSync } from 'node:child_process'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'
import pkg from './package.json'
import { manualChunkName } from './build/manualChunks'
import { visualFixturePlugin } from './build/visualFixture'

let buildCommit = 'unknown'
try {
  const value = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: __dirname, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim()
  if (/^[0-9a-f]{40}$/.test(value)) buildCommit = value
} catch { /* Source archives have no Git identity; do not invent one. */ }
// https://vitejs.dev/config/
export default defineConfig(({ command, mode }) => ({
  plugins: [visualFixturePlugin(__dirname, command === 'serve' && mode === 'visual' && process.env.CC_DESK_VISUAL_FIXTURE === '1'), vue()],
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src')
    }
  },
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
    __APP_BUILD_COMMIT__: JSON.stringify(buildCommit),
    // 原始翻译字符串使用 AST 解释执行，避免首次渲染依赖 CSP 禁止的 new Function。
    __INTLIFY_JIT_COMPILATION__: true,
    __INTLIFY_DROP_MESSAGE_COMPILER__: false,
  },
  test: {
    globals: true,
    environment: 'jsdom',
    include: ['tests/**/*.test.ts'],
    exclude: ['tests/visual/**'],
    setupFiles: ['tests/test-setup.ts'],
  },
  // Vite options tailored for Tauri development
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**']
    }
  },
  build: {
    // Tauri uses Chromium on Windows and WebKit on macOS and Linux
    target: ['es2021', 'chrome100', 'safari13'],
    // Don't minify for debug builds
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    // Produce sourcemaps for debug builds
    sourcemap: !!process.env.TAURI_DEBUG,
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        versionManager: resolve(__dirname, 'version-manager.html'),
      },
      output: {
        manualChunks: manualChunkName
      }
    }
  }
}))
