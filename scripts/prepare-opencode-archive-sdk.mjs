import { build } from 'esbuild'
import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { createHash } from 'node:crypto'
import { isBuiltin } from 'node:module'
import { spawnSync } from 'node:child_process'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const runtime = path.join(root, 'scripts/opencode-archive-runtime')
const manifest = await fs.readFile(path.join(runtime, 'package.json'))
const lockBytes = await fs.readFile(path.join(runtime, 'package-lock.json'))
const lock = JSON.parse(lockBytes)
const cacheKey = createHash('sha256').update(manifest).update(lockBytes)
  .update(`${process.platform}/${process.arch}/${process.versions.node}`).digest('hex')
const cache = path.join(root, '.cache/opencode-archive-sdk', cacheKey)
const output = path.resolve(process.argv[3] ?? (process.argv[2]
  ? path.join(root, '.audit/opencode-archive-bundle/bridge.mjs')
  : path.join(root, 'src-tauri/resources/opencode-native-archive/bridge.mjs')))

async function exists(filename) {
  try { await fs.access(filename); return true } catch (error) {
    if (error.code === 'ENOENT') return false
    throw error
  }
}

async function install() {
  const marker = path.join(cache, 'installed.json')
  if (await exists(marker) && await exists(path.join(cache, 'node_modules/.package-lock.json'))) return
  await fs.mkdir(cache, { recursive: true })
  const guard = path.join(cache, 'install.lock')
  // 并发准备不共享 npm ci 的可变目录；中断留下的锁由开发者核实后处理。
  try { await fs.mkdir(guard) } catch (error) {
    if (error.code === 'EEXIST') throw Error(`OpenCode SDK install is busy or interrupted: ${guard}`)
    throw error
  }
  try {
    await fs.writeFile(path.join(cache, 'package.json'), manifest)
    await fs.writeFile(path.join(cache, 'package-lock.json'), lockBytes)
    const args = ['ci', '--ignore-scripts', '--no-audit', '--no-fund', '--include=dev',
      '--cache', path.join(root, '.cache/opencode-archive-sdk/npm'),
      '--fetch-retries=1', '--fetch-timeout=60000']
    let command = 'npm'
    let npmCli = process.env.npm_execpath
    if (!npmCli || path.basename(npmCli) !== 'npm-cli.js' || !await exists(npmCli)) {
      npmCli = path.join(path.dirname(await fs.realpath(process.execPath)), 'node_modules/npm/bin/npm-cli.js')
    }
    if (await exists(npmCli)) {
      command = process.execPath
      args.unshift(npmCli)
    } else if (process.platform === 'win32') {
      throw Error('npm CLI was not found beside Node. Run npm run prepare:opencode-archive.')
    }
    const result = spawnSync(command, args, { cwd: cache, stdio: 'inherit', windowsHide: true, timeout: 600000 })
    if (result.error || result.status !== 0) {
      throw Error('OpenCode SDK npm ci failed. Check registry/network access and retry; no global dependencies or CLI settings were changed.', { cause: result.error })
    }
    await fs.writeFile(marker, JSON.stringify({ cacheKey }) + '\n')
  } finally {
    await fs.rmdir(guard)
  }
}

if (process.argv.length > 4) throw Error('Usage: node scripts/prepare-opencode-archive-sdk.mjs [node_modules [output.mjs]]')
if (!process.argv[2]) await install()
const modules = await fs.realpath(process.argv[2] ?? path.join(cache, 'node_modules'))
for (const name of ['@opencode/core', '@opencode/util', '@opencode/schema', 'effect', '@effect/sql-sqlite-node']) {
  const installed = JSON.parse(await fs.readFile(path.join(modules, name, 'package.json'), 'utf8'))
  if (installed.version !== lock.packages[`node_modules/${name}`].version) throw Error(`UNEXPECTED_SDK_VERSION: ${name}`)
}

const sqlite = path.join(root, 'scripts/opencode-archive-sqlite.mjs')
const result = await build({
  absWorkingDir: root,
  plugins: [{ name: 'archive-without-sqlite-extensions', setup(builder) {
    builder.onResolve({ filter: /^node:sqlite$/ }, args => args.importer === sqlite
      ? { path: 'node:sqlite', external: true } : { path: sqlite })
  } }],
  entryPoints: [path.join(root, 'scripts/opencode-archive-bridge.mjs')],
  outfile: output,
  nodePaths: [modules],
  bundle: true,
  platform: 'node',
  target: 'node24',
  format: 'esm',
  minify: true,
  legalComments: 'eof',
  metafile: true,
  write: false,
  external: ['ffi-rs'],
  alias: { 'jsonc-parser': path.join(modules, 'jsonc-parser/lib/esm/main.js') },
  banner: { js: '// OpenCode native archive helper. Third-party terms: LICENSES.txt and NOTICE.txt.\nimport {createRequire,isBuiltin} from "node:module";const builtinRequire=createRequire(import.meta.url);const require=id=>{if(!isBuiltin(id))throw Error("EXTERNAL_MODULE_FORBIDDEN");return builtinRequire(id)};' },
})

const packages = new Map()
const bundledInputs = new Set(Object.values(result.metafile.outputs).flatMap(info =>
  Object.entries(info.inputs).filter(([, entry]) => entry.bytesInOutput > 0).map(([input]) => input)))
for (const input of bundledInputs) {
  const absolute = path.resolve(root, input)
  if (absolute === sqlite || absolute === path.join(root, 'scripts/opencode-archive-bridge.mjs')) continue
  const relative = path.relative(modules, absolute).split(path.sep).join('/')
  if (relative.startsWith('../') || path.isAbsolute(relative)) throw Error(`UNPINNED_BUNDLE_INPUT: ${input}`)
  const parts = relative.split('/')
  const lastModules = parts.lastIndexOf('node_modules')
  const nameIndex = lastModules + 1
  const packagePath = parts.slice(0, nameIndex + (parts[nameIndex].startsWith('@') ? 2 : 1)).join('/')
  if (packages.has(packagePath)) continue
  const directory = path.join(modules, packagePath)
  const pkg = JSON.parse(await fs.readFile(path.join(directory, 'package.json'), 'utf8'))
  const pinned = lock.packages[`node_modules/${packagePath}`]
  if (!pinned || pkg.version !== pinned.version) throw Error(`UNPINNED_BUNDLE_PACKAGE: ${packagePath}`)
  packages.set(packagePath, { directory, pkg, pinned })
}
for (const outputInfo of Object.values(result.metafile.outputs)) {
  for (const imported of outputInfo.imports) {
    if (imported.external && !isBuiltin(imported.path)
      && !(imported.kind === 'require-call' && ['ffi-rs', 'supports-color'].includes(imported.path))) {
      throw Error(`UNEXPECTED_BUNDLE_EXTERNAL: ${imported.path}`)
    }
  }
}

const licenseFallbacks = {
  '@aws-sdk/credential-provider-http@3.972.86': ['aws-LICENSE'],
  '@aws-sdk/credential-provider-login@3.972.92': ['aws-LICENSE'],
  '@aws-sdk/nested-clients@3.998.3': ['aws-LICENSE'],
  '@npmcli/agent@4.0.2': ['isc-LICENSE'],
  '@sigstore/verify@3.1.1': ['sigstore-LICENSE'],
  'drizzle-orm@1.0.0-rc.5-169397b': ['drizzle-LICENSE'],
  'spdx-exceptions@2.5.0': ['ccby-LICENSE'],
  'spdx-license-ids@3.0.24': ['cczero-LICENSE'],
}
const readmeLicenses = new Set(['data-uri-to-buffer@4.0.1', 'spdx-exceptions@2.5.0', 'spdx-license-ids@3.0.24'])
const missingLicenses = []
const licenses = ['Third-party licenses for the CC Desk OpenCode native archive helper.\n']
const notice = [
  'CC Desk OpenCode native archive helper',
  'OpenCode 2.2.6 and Effect 4.1.0 are pinned build dependencies, not a user CLI version allowlist.',
  'Only bundled code and these license notices ship. No node_modules tree, CLI binary, model runtime or native addon is distributed.',
  'The helper uses the official local Session API with the CC Desk node:sqlite adapter. CC Desk bundles/minifies the upstream modules and replaces their node:sqlite wiring.',
  'OpenCode license source: https://github.com/anomalyco/opencode/blob/v2.2.6/LICENSE',
  'Full license texts and upstream notices follow in LICENSES.txt.',
  `Dependency lock SHA-256: ${createHash('sha256').update(lockBytes).digest('hex')}`,
  '',
]
for (const [packagePath, { directory, pkg, pinned }] of [...packages].sort(([a], [b]) => a.localeCompare(b, 'en'))) {
  const entries = await fs.readdir(directory, { withFileTypes: true })
  const legalFiles = []
  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name, 'en'))) {
    if (!/^(licen[cs]e|copying|notice|copyright)([._-]|$)/i.test(entry.name)) continue
    if (entry.isFile()) legalFiles.push(entry.name)
    else if (entry.isDirectory()) {
      for (const child of await fs.readdir(path.join(directory, entry.name), { withFileTypes: true })) {
        if (!child.isFile()) throw Error(`UNSUPPORTED_LICENSE_DIRECTORY: ${packagePath}/${entry.name}`)
        legalFiles.push(`${entry.name}/${child.name}`)
      }
    }
  }
  const identity = `${pkg.name}@${pkg.version}`
  notice.push(`${identity} (${pkg.license ?? pinned.license ?? 'see license text'})`, `  ${pinned.resolved}`)
  licenses.push(`\n${'='.repeat(72)}\n${identity}\n${pinned.resolved}\n`)
  const hasLicense = legalFiles.some(file => /^(licen[cs]e|copying)([._-]|$)/i.test(file))
  if (!hasLicense) {
    const fallback = pkg.name.startsWith('@opencode/') && pkg.version === '2.2.6' && pkg.license === 'MIT'
      ? ['opencode-2.2.6-LICENSE'] : licenseFallbacks[identity]
    if (!fallback && !readmeLicenses.has(identity)) missingLicenses.push(identity)
    if (identity === '@npmcli/agent@4.0.2') licenses.push('Upstream declares ISC but supplies no copyright header; the complete SPDX ISC template follows verbatim, with upstream author metadata retained below.')
    if (pkg.author) licenses.push('Upstream author: ' + (typeof pkg.author === 'string' ? pkg.author : JSON.stringify(pkg.author)))
    for (const file of fallback ?? []) licenses.push(await fs.readFile(path.join(runtime, 'licenses', file), 'utf8'))
    if (readmeLicenses.has(identity)) licenses.push(await fs.readFile(path.join(directory, 'README.md'), 'utf8'))
  }
  for (const file of legalFiles.sort()) {
    licenses.push(`--- ${file} ---\n${await fs.readFile(path.join(directory, file), 'utf8')}`)
  }
}

if (missingLicenses.length) throw Error(`MISSING_THIRD_PARTY_LICENSE: ${missingLicenses.join(", ")}`)

// 许可证缺失或构建失败时不覆盖旧产物，npm 构建链以非零状态停止。
await fs.mkdir(path.dirname(output), { recursive: true })
await fs.writeFile(path.join(path.dirname(output), 'LICENSES.txt'), licenses.join('\n') + '\n')
await fs.writeFile(path.join(path.dirname(output), 'NOTICE.txt'), notice.join('\n') + '\n')
await fs.writeFile(output, result.outputFiles[0].contents)
console.log(JSON.stringify({ output, bytes: result.outputFiles[0].contents.length, packages: packages.size }))
