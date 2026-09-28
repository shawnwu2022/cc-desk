#!/usr/bin/env node
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { readFileSync, realpathSync, statSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

function fail(code) {
  const error = new Error(code)
  error.code = code
  throw error
}

export function recordCanaryIdentity({ cli, channel, binaryPath }) {
  if (!['claude', 'codex'].includes(cli)) fail('CLI_KIND_REQUIRED')
  if (!['pinned', 'stable'].includes(channel)) fail('CANARY_CHANNEL_REQUIRED')
  const resolved = realpathSync(binaryPath)
  if (!statSync(resolved).isFile()) fail('CLI_BINARY_REQUIRED')
  const result = spawnSync(resolved, ['--version'], {
    encoding: 'utf8',
    timeout: 30_000,
    windowsHide: true,
    env: process.env,
  })
  if (result.error || result.signal || result.status !== 0) fail('CLI_VERSION_PROBE_FAILED')
  const version = String(result.stdout || result.stderr || '').trim()
  if (!version || version.length > 4096) fail('CLI_VERSION_INVALID')
  return {
    schemaVersion: 1,
    status: 'NOT_CERTIFIED',
    certificationRequired: true,
    cli,
    channel,
    version,
    binarySha256: createHash('sha256').update(readFileSync(resolved)).digest('hex'),
  }
}

function main() {
  const [cli, channel, binaryPath, outputPath] = process.argv.slice(2)
  if (!cli || !channel || !binaryPath || !outputPath) fail('CANARY_ARGUMENTS_REQUIRED')
  const record = recordCanaryIdentity({ cli, channel, binaryPath })
  writeFileSync(path.resolve(outputPath), JSON.stringify(record, null, 2) + '\n')
  process.stdout.write(`${cli} ${channel}: identity recorded; certification required\n`)
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    main()
  } catch (error) {
    process.stderr.write(String(error?.code || 'CANARY_IDENTITY_FAILED') + '\n')
    process.exit(1)
  }
}
