import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'node:path'
import { visualFixturePlugin } from './build/visualFixture'
import pkg from './package.json'

// Dedicated test-only module graph; ordinary Vitest and production never alias host APIs.
export default defineConfig({
  plugins: [visualFixturePlugin(__dirname, true), vue()],
  resolve: { alias: { '@': resolve(__dirname, 'src') } },
  define: { __APP_VERSION__: JSON.stringify(pkg.version), __APP_BUILD_COMMIT__: JSON.stringify('unknown') },
  test: { environment: 'jsdom', include: ['tests/visual/fixture.test.ts'], setupFiles: ['tests/test-setup.ts'] },
})
