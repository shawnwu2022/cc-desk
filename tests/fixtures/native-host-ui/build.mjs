import { build } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'node:path'

// 仅构建实际面板与最小夹具，不载入 App 的安装版启动副作用。
await build({
  configFile: false,
  plugins: [vue()],
  resolve: { alias: { '@': resolve('src') } },
  define: { 'process.env.NODE_ENV': '"production"' },
  build: {
    outDir: '.audit/host-ui-isolation-20261114/ui',
    emptyOutDir: false,
    lib: { entry: 'tests/fixtures/native-host-ui/entry.ts', name: 'HostUiFixture', formats: ['iife'], fileName: () => 'host-ui.js' },
    rollupOptions: { output: { inlineDynamicImports: true } },
  },
})
