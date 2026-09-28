#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { readFileSync, realpathSync, statSync, writeFileSync } from 'node:fs'
import { basename, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { spawnSync } from 'node:child_process'

const MAX_OUTPUT = 1024 * 1024
const CLI = new Set(['claude', 'codex'])
const CHANNEL = new Set(['pinned', 'stable'])

function fail(code) {
  const error = new Error(code)
  error.code = code
  throw error
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function safeEnv(source = process.env) {
  const allowed = [
    'PATH', 'Path', 'LANG', 'LC_ALL', 'TERM', 'SHELL',
    'SystemRoot', 'WINDIR', 'ComSpec', 'PATHEXT', 'TEMP', 'TMP', 'TMPDIR',
  ]
  const output = {}
  for (const name of allowed) {
    const value = source[name]
    if (typeof value === 'string' && value.length > 0 && !value.includes('\0')) {
      output[name] = value
    }
  }
  output.DISABLE_AUTOUPDATER = '1'
  return output
}

function run(binary, args, env) {
  const result = spawnSync(binary, args, {
    encoding: 'utf8',
    env,
    timeout: 30_000,
    maxBuffer: MAX_OUTPUT,
    windowsHide: true,
  })
  if (result.error) fail(result.error.code === 'ETIMEDOUT' ? 'CANARY_COMMAND_TIMEOUT' : 'CANARY_COMMAND_FAILED')
  if (result.status !== 0) fail('CANARY_COMMAND_FAILED')
  const stdout = result.stdout ?? ''
  const stderr = result.stderr ?? ''
  if (Buffer.byteLength(stdout) + Buffer.byteLength(stderr) > MAX_OUTPUT) {
    fail('CANARY_OUTPUT_TOO_LARGE')
  }
  return { stdout, stderr }
}

export function probeCliIdentity({
  cli,
  binaryPath,
  channel,
  expectedVersion = null,
  sourceEnv = process.env,
}) {
  if (!CLI.has(cli)) fail('CANARY_CLI_INVALID')
  if (!CHANNEL.has(channel)) fail('CANARY_CHANNEL_INVALID')
  if (expectedVersion !== null
    && (typeof expectedVersion !== 'string' || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(expectedVersion))) {
    fail('CANARY_EXPECTED_VERSION_INVALID')
  }

  const binary = resolve(binaryPath)
  let real
  let bytes
  try {
    real = realpathSync(binary)
    if (!statSync(real).isFile()) fail('CANARY_BINARY_INVALID')
    bytes = readFileSync(real)
  } catch {
    fail('CANARY_BINARY_UNAVAILABLE')
  }

  const env = safeEnv(sourceEnv)
  const version = run(binary, ['--version'], env)
  const versionText = (version.stdout + version.stderr).trim()
  if (!versionText || versionText.length > 4096) fail('CANARY_VERSION_INVALID')
  if (expectedVersion !== null && !versionText.includes(expectedVersion)) {
    fail('CANARY_PINNED_VERSION_MISMATCH')
  }

  const help = run(binary, ['--help'], env)
  const helpBytes = Buffer.from((help.stdout ?? '') + (help.stderr ?? ''))
  if (helpBytes.length === 0) fail('CANARY_HELP_EMPTY')

  return {
    schemaVersion: 1,
    status: 'PASS',
    certification: false,
    cli,
    channel,
    expectedVersion,
    observedVersion: versionText,
    binaryName: basename(binary),
    binarySha256: sha256(bytes),
    binaryBytes: bytes.byteLength,
    helpSha256: sha256(helpBytes),
    helpBytes: helpBytes.byteLength,
  }
}

function parseArgs(argv) {
  const values = {}
  for (let index = 0; index < argv.length; index += 1) {
    const name = argv[index]
    if (!['--cli', '--binary', '--channel', '--expected-version', '--output'].includes(name)) {
      fail('CANARY_OPTION_INVALID')
    }
    if (index + 1 >= argv.length) fail('CANARY_OPTION_VALUE_REQUIRED')
    values[name.slice(2)] = argv[++index]
  }
  for (const required of ['cli', 'binary', 'channel', 'output']) {
    if (!values[required]) fail('CANARY_OPTION_REQUIRED')
  }
  return values
}

function main() {
  const args = parseArgs(process.argv.slice(2))
  const result = probeCliIdentity({
    cli: args.cli,
    binaryPath: args.binary,
    channel: args.channel,
    expectedVersion: args['expected-version'] ?? null,
  })
  writeFileSync(resolve(args.output), JSON.stringify(result, null, 2) + '\n')
}

const isEntryPoint = process.argv[1]
  ? import.meta.url === pathToFileURL(resolve(process.argv[1])).href
  : false

if (isEntryPoint) {
  try {
    main()
  } catch (error) {
    process.stderr.write(String(error?.code ?? error?.message ?? 'CANARY_FAILED') + '\n')
    process.exitCode = 1
  }
}
