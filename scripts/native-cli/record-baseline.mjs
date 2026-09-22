#!/usr/bin/env node

import { spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync } from 'node:fs'
import {
  appendFile,
  mkdir,
  open,
  readFile,
  rename,
  writeFile,
} from 'node:fs/promises'
import {
  isAbsolute,
  join,
  relative,
  resolve,
} from 'node:path'
import { fileURLToPath } from 'node:url'

const VALID_STATUSES = new Set(['PASS', 'FAIL', 'BLOCKED'])
const SHA256_PATTERN = /^[0-9a-f]{64}$/
const COMMIT_PATTERN = /^[0-9a-f]{40}$/
const SAFE_COMMAND_NAME = /^[a-z0-9][a-z0-9._-]{0,63}$/i
const WINDOWS_ABSOLUTE = /^(?:[a-z]:[\\/]|\\\\)/i

export const DEFAULT_LOCK_FILES = Object.freeze([
  'package-lock.json',
  'src-tauri/Cargo.lock',
])

export function defaultBaselineCommands(platform = process.platform) {
  const commands = [
    { name: 'typecheck', argv: ['npm', 'run', 'typecheck'] },
    { name: 'frontend-tests', argv: ['npm', 'run', 'test:ci'] },
  ]

  if (platform === 'win32') {
    commands.push({
      name: 'prepare-conpty',
      argv: [process.execPath, 'scripts/prepare-conpty.mjs'],
    })
  }

  commands.push(
    {
      name: 'rust-fmt',
      argv: ['cargo', 'fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--check'],
    },
    {
      name: 'rust-clippy',
      argv: [
        'cargo',
        'clippy',
        '--locked',
        '--manifest-path',
        'src-tauri/Cargo.toml',
        '--all-targets',
        '--',
        '-D',
        'warnings',
      ],
    },
    {
      name: 'rust-tests',
      argv: ['cargo', 'test', '--locked', '--manifest-path', 'src-tauri/Cargo.toml'],
    },
    { name: 'frontend-build', argv: ['npm', 'run', 'build'] },
  )

  return commands
}

function fail(code) {
  throw new Error(code)
}

function isPlainObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function validateRelativePath(value) {
  if (typeof value !== 'string' || value.length === 0) {
    fail('INVALID_LOG_PATH')
  }

  if (
    isAbsolute(value)
    || WINDOWS_ABSOLUTE.test(value)
    || value.split(/[\\/]+/).includes('..')
  ) {
    fail('LOG_PATH_MUST_BE_RELATIVE')
  }
}

function validateArgv(argv) {
  if (
    !Array.isArray(argv)
    || argv.length === 0
    || argv.some(arg => typeof arg !== 'string' || arg.includes('\0'))
  ) {
    fail('INVALID_ARGV')
  }
}

export function validateBaseline(report) {
  if (!isPlainObject(report)) fail('INVALID_REPORT')
  if (typeof report.commit !== 'string' || !COMMIT_PATTERN.test(report.commit)) {
    fail('INVALID_COMMIT')
  }
  if (typeof report.dirty !== 'boolean') fail('INVALID_DIRTY_STATE')
  if (typeof report.platform !== 'string' || report.platform.length === 0) {
    fail('INVALID_PLATFORM')
  }
  if (!isPlainObject(report.lockHashes)) fail('INVALID_LOCK_HASHES')

  for (const [lockPath, hash] of Object.entries(report.lockHashes)) {
    validateRelativePath(lockPath)
    if (typeof hash !== 'string' || !SHA256_PATTERN.test(hash)) {
      fail('INVALID_LOCK_HASH')
    }
  }

  if (!Array.isArray(report.commands) || report.commands.length === 0) {
    fail('MISSING_COMMAND_RESULTS')
  }

  for (const result of report.commands) {
    if (!isPlainObject(result)) fail('INVALID_COMMAND_RESULT')
    validateArgv(result.argv)
    if (!VALID_STATUSES.has(result.status)) fail('INVALID_COMMAND_STATUS')
    if (result.exitCode !== null && !Number.isInteger(result.exitCode)) {
      fail('INVALID_EXIT_CODE')
    }
    validateRelativePath(result.logPath)

    if (result.status === 'PASS' && result.exitCode === null) {
      fail('MISSING_EXIT_CODE')
    }
    if (result.status === 'PASS' && result.exitCode !== 0) {
      fail('PASS_REQUIRES_ZERO_EXIT')
    }
    if (result.status === 'FAIL' && result.exitCode === 0) {
      fail('FAIL_REQUIRES_NONZERO_EXIT')
    }
  }

  return true
}

async function hashFile(path) {
  const content = await readFile(path)
  return createHash('sha256').update(content).digest('hex')
}

async function runBuffered(program, args, cwd) {
  return new Promise(resolveResult => {
    const child = spawn(program, args, {
      cwd,
      env: process.env,
      shell: false,
      windowsHide: true,
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    let stdout = ''
    let stderr = ''
    let settled = false

    const finish = result => {
      if (settled) return
      settled = true
      resolveResult({ ...result, stdout, stderr })
    }

    child.stdout?.setEncoding('utf8')
    child.stderr?.setEncoding('utf8')
    child.stdout?.on('data', chunk => { stdout += chunk })
    child.stderr?.on('data', chunk => { stderr += chunk })
    child.once('error', error => finish({ exitCode: null, errorCode: error.code ?? 'SPAWN_ERROR' }))
    child.once('close', (exitCode, signal) => finish({ exitCode, signal }))
  })
}

async function runCommandToLog(command, index, repoRoot, outDir) {
  if (!isPlainObject(command) || !SAFE_COMMAND_NAME.test(command.name ?? '')) {
    fail('INVALID_COMMAND_NAME')
  }
  validateArgv(command.argv)

  const logPath = `logs/${String(index + 1).padStart(2, '0')}-${command.name}.log`
  const logAbsolute = join(outDir, ...logPath.split('/'))
  const handle = await open(logAbsolute, 'w')

  let outcome
  try {
    outcome = await new Promise(resolveResult => {
      let settled = false
      const finish = result => {
        if (settled) return
        settled = true
        resolveResult(result)
      }

      const child = spawn(command.argv[0], command.argv.slice(1), {
        cwd: command.cwd ? resolve(repoRoot, command.cwd) : repoRoot,
        env: process.env,
        shell: false,
        windowsHide: true,
        stdio: ['ignore', handle.fd, handle.fd],
      })

      child.once('error', error => finish({
        exitCode: null,
        errorCode: error.code ?? 'SPAWN_ERROR',
      }))
      child.once('close', (exitCode, signal) => finish({ exitCode, signal }))
    })
  } finally {
    await handle.close()
  }

  if (outcome.errorCode) {
    await appendFile(logAbsolute, `\n[spawn-error] ${outcome.errorCode}\n`, 'utf8')
  } else if (outcome.signal) {
    await appendFile(logAbsolute, `\n[signal] ${outcome.signal}\n`, 'utf8')
  }

  const status = outcome.errorCode
    ? 'BLOCKED'
    : outcome.exitCode === 0
      ? 'PASS'
      : 'FAIL'

  return {
    argv: [...command.argv],
    exitCode: outcome.exitCode,
    status,
    logPath,
  }
}

export async function recordBaseline(repoRootInput, outDirInput, options = {}) {
  const repoRoot = resolve(repoRootInput)
  const outDir = resolve(outDirInput)

  const head = await runBuffered('git', ['rev-parse', 'HEAD'], repoRoot)
  if (head.exitCode !== 0 || !COMMIT_PATTERN.test(head.stdout.trim())) {
    fail('GIT_HEAD_UNAVAILABLE')
  }

  const status = await runBuffered('git', ['status', '--short'], repoRoot)
  if (status.exitCode !== 0) fail('GIT_STATUS_UNAVAILABLE')

  const lockFiles = options.lockFiles ?? DEFAULT_LOCK_FILES
  if (!Array.isArray(lockFiles)) fail('INVALID_LOCK_FILES')

  const lockHashes = {}
  for (const lockPath of lockFiles) {
    validateRelativePath(lockPath)
    const absolute = join(repoRoot, ...lockPath.split('/'))
    if (existsSync(absolute)) {
      lockHashes[lockPath] = await hashFile(absolute)
    }
  }

  const commands = options.commands ?? defaultBaselineCommands()
  if (!Array.isArray(commands) || commands.length === 0) {
    fail('MISSING_COMMANDS')
  }

  await mkdir(join(outDir, 'logs'), { recursive: true })
  const commandResults = []
  for (const [index, command] of commands.entries()) {
    commandResults.push(await runCommandToLog(command, index, repoRoot, outDir))
  }

  const report = {
    commit: head.stdout.trim(),
    dirty: status.stdout.trim().length > 0,
    lockHashes,
    commands: commandResults,
    platform: `${process.platform}-${process.arch}`,
  }
  validateBaseline(report)

  const reportPath = join(outDir, 'baseline.json')
  const temporaryPath = `${reportPath}.${process.pid}.${Date.now()}.tmp`
  await writeFile(temporaryPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8')
  await rename(temporaryPath, reportPath)

  return report
}

async function main() {
  const repoRoot = resolve(process.argv[2] ?? process.cwd())
  const outDir = resolve(process.argv[3] ?? join(repoRoot, '.native-cli-baseline'))
  const report = await recordBaseline(repoRoot, outDir)
  const summary = {
    report: relative(process.cwd(), join(outDir, 'baseline.json')) || 'baseline.json',
    commit: report.commit,
    dirty: report.dirty,
    results: report.commands.map(entry => entry.status),
  }
  process.stdout.write(`${JSON.stringify(summary)}\n`)

  if (report.commands.some(entry => entry.status === 'FAIL')) process.exitCode = 1
  else if (report.commands.some(entry => entry.status === 'BLOCKED')) process.exitCode = 2
}

const entryPath = process.argv[1] ? resolve(process.argv[1]) : ''
if (entryPath === fileURLToPath(import.meta.url)) {
  main().catch(error => {
    process.stderr.write(`[baseline-error] ${error instanceof Error ? error.message : 'UNKNOWN'}\n`)
    process.exitCode = 1
  })
}
