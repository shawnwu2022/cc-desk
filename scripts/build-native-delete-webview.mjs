import { build } from 'vite'
import vue from '@vitejs/plugin-vue'
import { readFile, writeFile, mkdir, readdir } from 'node:fs/promises'
import { resolve, relative, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const outDir = resolve(root, '.audit/v027-stage11-webview-dist')
const pkg = JSON.parse(await readFile(resolve(root, 'package.json'), 'utf8'))
const config = JSON.parse(await readFile(resolve(root, 'src-tauri/tauri.conf.json'), 'utf8'))
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex')
const modules = new Set(), prefix = resolve(root, 'src').replaceAll('\\', '/') + '/'
await mkdir(outDir, { recursive: true })
await build({ configFile: false, root: resolve(root, 'tests/native-delete-webview'), base: '/',
  plugins: [vue(), { name: 'record-production-sources', moduleParsed(info) { const file = info.id.split('?')[0].replaceAll('\\', '/'); if (file.startsWith(prefix)) modules.add(file) } }],
  resolve: { alias: { '@': resolve(root, 'src') } },
  define: { __APP_VERSION__: JSON.stringify(pkg.version), __APP_BUILD_COMMIT__: JSON.stringify(execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim()), __INTLIFY_JIT_COMPILATION__: true, __INTLIFY_DROP_MESSAGE_COMPILER__: false },
  build: { outDir, emptyOutDir: false, target: ['chrome100'], minify: 'esbuild', sourcemap: false },
})
const files = []
async function collect(dir) { for (const item of await readdir(dir, { withFileTypes: true })) { const file = resolve(dir, item.name); if (item.isSymbolicLink()) throw Error('ASSET_LINK_UNSUPPORTED'); if (item.isDirectory()) await collect(file); else if (item.name !== 'manifest.json') { const bytes = await readFile(file); files.push({ path: relative(outDir, file).replaceAll('\\', '/'), bytes: bytes.length, sha256: sha256(bytes) }) } } }
await collect(outDir)
const sourceModules = []
for (const file of [...modules].sort()) sourceModules.push({ path: relative(root, file).replaceAll('\\', '/'), sha256: sha256(await readFile(file)) })
for (const required of ['src/components/sessions/ArchivedSessionsDrawer.vue', 'src/components/sessions/NativeSessionRecyclePanel.vue', 'src/api/nativeSessionRecycle.ts', 'src/stores/nativeSessionRecovery.ts', 'src/components/sessions/NativeSessionDeletePanel.vue', 'src/stores/nativeSessionDeletes.ts', 'src/stores/nativeHistory.ts', 'src/api/nativeSessionDelete.ts', 'src/session/adapters/nativeCliAdapter.ts']) if (!sourceModules.some(file => file.path === required)) throw Error('PRODUCTION_MODULE_NOT_BUNDLED')
const drivers = []
for (const path of ['tests/native-delete-webview/main.ts', 'tests/native-delete-webview/index.html', 'scripts/build-native-delete-webview.mjs']) drivers.push({ path, sha256: sha256(await readFile(resolve(root, path))) })
const manifest = { schema: 1, appVersion: pkg.version, buildHead: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(), csp: config.app.security.csp, modules: sourceModules, drivers, files: files.sort((a, b) => a.path.localeCompare(b.path)) }
await writeFile(resolve(outDir, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n')
console.log(JSON.stringify({ output: relative(root, outDir), files: files.length, productionModules: sourceModules.length, manifestSha256: sha256(Buffer.from(JSON.stringify(manifest, null, 2) + '\n')) }))
