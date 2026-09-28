#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, realpathSync, statSync, writeFileSync } from 'node:fs'
import { dirname, isAbsolute, join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const MAX_PACKAGE_JSON = 1024 * 1024
const MAX_OUTPUT = 1024 * 1024

function fail(code) {
  throw new Error(code)
}

function safeText(value, code) {
  if (typeof value !== 'string' || value.length === 0 || value.includes('\0')) fail(code)
  return value
}

function packageDirectory(packageRoot, packageName) {
  const root = resolve(packageRoot)
  const parts = packageName.split('/')
  if (
    parts.length > 2
    || parts.some(part => !/^[@A-Za-z0-9._-]+$/.test(part))
    || (parts.length === 2 && !parts[0].startsWith('@'))
  ) {
    fail('CANARY_PACKAGE_NAME_INVALID')
  }
  return join(root, 'node_modules', ...parts)
}

function readPackageJson(packageDir) {
  const path = join(packageDir, 'package.json')
  const stat = statSync(path)
  if (!stat.isFile() || stat.size <= 0 || stat.size > MAX_PACKAGE_JSON) {
    fail('CANARY_PACKAGE_JSON_INVALID')
  }
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch {
    fail('CANARY_PACKAGE_JSON_INVALID')
  }
}

function binaryRelative(pkg, cli) {
  if (typeof pkg.bin === 'string') return pkg.bin
  if (pkg.bin && typeof pkg.bin === 'object' && !Array.isArray(pkg.bin)) {
    if (typeof pkg.bin[cli] === 'string') return pkg.bin[cli]
    const values = Object.values(pkg.bin).filter(value => typeof value === 'string')
    if (values.length === 1) return values[0]
  }
  fail('CANARY_BINARY_UNAVAILABLE')
}

function resolveBinary(packageDir, relativePath) {
  safeText(relativePath, 'CANARY_BINARY_INVALID')
  if (isAbsolute(relativePath) || relativePath.split(/[\\/]+/).includes('..')) {
    fail('CANARY_BINARY_INVALID')
  }
  const root = realpathSync(packageDir)
  const full = realpathSync(join(root, relativePath))
  const prefix = root.endsWith('/') || root.endsWith('\\') ? root : `${root}/`
  const normalizedRoot = prefix.replaceAll('\\', '/')
  const normalizedFull = full.replaceAll('\\', '/')
  if (!normalizedFull.startsWith(normalizedRoot)) fail('CANARY_BINARY_ESCAPE')
  const stat = statSync(full)
  if (!stat.isFile() || stat.size <= 0 || stat.size > 128 * 1024 * 1024) {
    fail('CANARY_BINARY_INVALID')
  }
  return full
}

function hashFile(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

function safeEnvironment(source = process.env, workDir) {
  const result = {}
  for (const name of ['PATH', 'Path', 'LANG', 'LC_ALL', 'TERM', 'SystemRoot', 'WINDIR', 'ComSpec', 'PATHEXT', 'TEMP', 'TMP', 'TMPDIR']) {
    const value = source[name]
    if (typeof value === 'string' && value.length > 0 && !value.includes('\0')) result[name] = value
  }

  const home = resolve(workDir, 'home')
  const config = resolve(workDir, 'config')
  const codex = resolve(config, 'codex')
  const claude = resolve(config, 'claude')
  for (const path of [home, config, codex, claude]) {
    mkdirSync(path, { recursive: true, mode: 0o700 })
  }
  result.HOME = home
  result.USERPROFILE = home
  result.XDG_CONFIG_HOME = config
  result.CODEX_HOME = codex
  result.CLAUDE_CONFIG_DIR = claude
  return result
}

function execute(binary, args, cwd, env) {
  const javascript = /\.(?:cjs|mjs|js)$/i.test(binary)
  const program = javascript ? process.execPath : binary
  const argv = javascript ? [binary, ...args] : args
  const outcome = spawnSync(program, argv, {
    cwd,
    env,
    shell: false,
    windowsHide: true,
    encoding: 'utf8',
    timeout: 30_000,
    maxBuffer: MAX_OUTPUT,
  })
  if (outcome.error || outcome.signal || outcome.status !== 0) {
    fail('CANARY_EXECUTION_FAILED')
  }
  return String(outcome.stdout || outcome.stderr || '').trim().slice(0, 4096)
}

export function probeInstalledCli({
  packageRoot,
  packageName,
  cli,
  lane,
  requestedSpec,
  workDir,
  sourceEnv = process.env,
}) {
  if (!['claude', 'codex'].includes(cli)) fail('CANARY_CLI_INVALID')
  if (!['pinned', 'latest-stable'].includes(lane)) fail('CANARY_LANE_INVALID')
  safeText(requestedSpec, 'CANARY_SPEC_INVALID')
  const packageDir = packageDirectory(packageRoot, packageName)
  const pkg = readPackageJson(packageDir)
  safeText(pkg.version, 'CANARY_VERSION_INVALID')
  const binary = resolveBinary(packageDir, binaryRelative(pkg, cli))
  const env = safeEnvironment(sourceEnv, workDir)
  const versionOutput = execute(binary, ['--version'], workDir, env)
  execute(binary, ['--help'], workDir, env)

  return {
    schemaVersion: 1,
    probeStatus: 'PASS',
    certificationStatus: 'NOT_RUN',
    platform: process.platform,
    arch: process.arch,
    cli,
    lane,
    packageName,
    requestedSpec,
    resolvedVersion: pkg.version,
    binarySha256: hashFile(binary),
    versionOutput,
  }
}

function parseArgs(argv) {
  const allowed = new Set([
    '--package-root', '--package-name', '--cli', '--lane', '--requested-spec', '--work-dir', '--out',
  ])
  const values = {}
  for (let i = 0; i < argv.length; i += 1) {
    const key = argv[i]
    if (!allowed.has(key)) fail('UNKNOWN_OPTION')
    if (i + 1 >= argv.length || values[key]) fail('INVALID_OPTION')
    values[key] = argv[++i]
  }
  for (const key of ['--package-root', '--package-name', '--cli', '--lane', '--requested-spec', '--work-dir', '--out']) {
    if (!values[key]) fail('REQUIRED_OPTION_MISSING')
  }
  return values
}

function main() {
  try {
    const args = parseArgs(process.argv.slice(2))
    const report = probeInstalledCli({
      packageRoot: resolve(args['--package-root']),
      packageName: args['--package-name'],
      cli: args['--cli'],
      lane: args['--lane'],
      requestedSpec: args['--requested-spec'],
      workDir: resolve(args['--work-dir']),
    })
    const out = resolve(args['--out'])
    mkdirSync(dirname(out), { recursive: true })
    writeFileSync(out, `${JSON.stringify(report, null, 2)}\n`, { mode: 0o600 })
    process.stdout.write(`${JSON.stringify({
      probeStatus: report.probeStatus,
      certificationStatus: report.certificationStatus,
      platform: report.platform,
      arch: report.arch,
      cli: report.cli,
      lane: report.lane,
      resolvedVersion: report.resolvedVersion,
      binarySha256: report.binarySha256,
    })}\n`)
  } catch (error) {
    const reason = error instanceof Error && /^[A-Z0-9_]+$/.test(error.message)
      ? error.message
      : 'CANARY_FAILED'
    process.stderr.write(`[native-canary-error] ${reason}\n`)
    process.exitCode = 1
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main()
