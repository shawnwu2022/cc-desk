#!/usr/bin/env node

import { spawn } from 'node:child_process'
import { realpathSync, renameSync, writeFileSync } from 'node:fs'
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path'

const MAX_CAPTURE_BYTES = 16 * 1024 * 1024
const MAX_OUTPUT_BYTES = 16 * 1024 * 1024
const MAX_DELAY_MS = 60_000
const OUTPUT_MARKER_PATTERN = /^[A-Za-z0-9._-]{1,64}$/

function fail(message) {
  throw new Error(message)
}

function parseInteger(value, name, min, max) {
  if (!/^(?:0|[1-9]\d*)$/.test(value ?? '')) fail(`invalid ${name}`)
  const parsed = Number(value)
  if (!Number.isSafeInteger(parsed) || parsed < min || parsed > max) {
    fail(`invalid ${name}`)
  }
  return parsed
}

function parseArgs(argv) {
  const separator = argv.indexOf('--')
  const options = separator === -1 ? argv : argv.slice(0, separator)
  const rawArgv = separator === -1 ? [] : argv.slice(separator + 1)
  const parsed = {
    report: null,
    readyFile: null,
    captureInput: false,
    captureBytes: null,
    outputBytes: 0,
    outputMarker: null,
    exitCode: 0,
    delayReadMs: 0,
    holdSlaveMs: 0,
    rawArgv,
  }

  for (let index = 0; index < options.length; index += 1) {
    const option = options[index]
    const nextValue = () => {
      if (index + 1 >= options.length) fail(`missing value for ${option}`)
      index += 1
      return options[index]
    }

    switch (option) {
      case '--report':
        if (parsed.report !== null) fail('duplicate --report')
        parsed.report = nextValue()
        break
      case '--ready-file':
        if (parsed.readyFile !== null) fail('duplicate --ready-file')
        parsed.readyFile = nextValue()
        break
      case '--capture-input':
        parsed.captureInput = true
        break
      case '--capture-bytes':
        parsed.captureBytes = parseInteger(
          nextValue(),
          'capture bytes',
          0,
          MAX_CAPTURE_BYTES,
        )
        break
      case '--output-bytes':
        parsed.outputBytes = parseInteger(
          nextValue(),
          'output bytes',
          0,
          MAX_OUTPUT_BYTES,
        )
        break
      case '--output-marker':
        if (parsed.outputMarker !== null) fail('duplicate --output-marker')
        parsed.outputMarker = nextValue()
        break
      case '--exit-code':
        parsed.exitCode = parseInteger(nextValue(), 'exit code', 0, 255)
        break
      case '--delay-read-ms':
        parsed.delayReadMs = parseInteger(
          nextValue(),
          'delay read ms',
          0,
          MAX_DELAY_MS,
        )
        break
      case '--hold-slave-ms':
        parsed.holdSlaveMs = parseInteger(
          nextValue(),
          'hold slave ms',
          0,
          MAX_DELAY_MS,
        )
        break
      default:
        fail(`unknown probe option: ${option}`)
    }
  }

  if (parsed.report === null) fail('missing --report')
  if (!isAbsolute(parsed.report)) fail('report path must be absolute')
  if (parsed.readyFile !== null && !isAbsolute(parsed.readyFile)) {
    fail('ready file path must be absolute')
  }
  if (parsed.captureInput && parsed.captureBytes === null) {
    fail('--capture-input requires --capture-bytes')
  }
  if (!parsed.captureInput && parsed.captureBytes !== null) {
    fail('--capture-bytes requires --capture-input')
  }
  if (!parsed.captureInput && parsed.readyFile !== null) {
    fail('--ready-file requires --capture-input')
  }
  if (parsed.outputBytes > 0 && parsed.outputMarker === null) {
    fail('--output-bytes requires --output-marker')
  }
  if (parsed.outputBytes === 0 && parsed.outputMarker !== null) {
    fail('--output-marker requires --output-bytes')
  }
  if (
    parsed.outputMarker !== null
    && !OUTPUT_MARKER_PATTERN.test(parsed.outputMarker)
  ) {
    fail('invalid output marker')
  }

  return parsed
}

function validateTestPath(outputPath, label) {
  const rootValue = process.env.CC_DESK_TEST_ROOT
  if (!rootValue || !isAbsolute(rootValue)) {
    fail('missing absolute CC_DESK_TEST_ROOT')
  }

  const root = realpathSync(rootValue)
  const parent = realpathSync(dirname(outputPath))
  const fromRoot = relative(root, parent)
  if (fromRoot === '..' || fromRoot.startsWith(`..${sep}`) || isAbsolute(fromRoot)) {
    fail(`${label} outside test root`)
  }
  return resolve(outputPath)
}

function writeReport(reportPath, value) {
  const temporaryPath = `${reportPath}.${process.pid}.${Date.now()}.tmp`
  writeFileSync(temporaryPath, `${JSON.stringify(value, null, 2)}\n`, {
    encoding: 'utf8',
    flag: 'wx',
  })
  renameSync(temporaryPath, reportPath)
}

function reportFailure(error) {
  process.stderr.write(
    `probe error: ${error instanceof Error ? error.message : String(error)}\n`,
  )
  process.exitCode = 2
}

function main() {
  const options = parseArgs(process.argv.slice(2))
  const reportPath = validateTestPath(options.report, 'report path')
  const readyPath = options.readyFile === null
    ? null
    : validateTestPath(options.readyFile, 'ready file path')
  const captured = []
  let capturedLength = 0
  let finished = false
  let ready = false

  const announceReady = () => {
    if (ready || readyPath === null) return
    writeFileSync(readyPath, '', { encoding: 'utf8', flag: 'wx' })
    ready = true
  }

  const finish = () => {
    if (finished) return
    finished = true

    if (process.stdin.isTTY && typeof process.stdin.setRawMode === 'function') {
      process.stdin.setRawMode(false)
    }
    process.stdin.pause()

    const input = Buffer.concat(captured, capturedLength)
    writeReport(reportPath, {
      argv: options.rawArgv,
      cwd: process.cwd(),
      stdinIsTTY: Boolean(process.stdin.isTTY),
      stdoutIsTTY: Boolean(process.stdout.isTTY),
      env: {
        CC_DESK_FIXTURE_VALUE: process.env.CC_DESK_FIXTURE_VALUE ?? null,
      },
      capturedBase64: options.captureInput ? input.toString('base64') : null,
      requestedExitCode: options.exitCode,
    })

    if (options.holdSlaveMs > 0) {
      const helper = spawn(
        process.execPath,
        ['-e', `setTimeout(() => {}, ${options.holdSlaveMs})`],
        {
          stdio: ['ignore', 'inherit', 'inherit'],
          windowsHide: true,
        },
      )
      helper.unref()
    }

    if (options.outputBytes > 0) {
      const begin = Buffer.from(
        `<<CC_DESK_PROBE_OUTPUT_BEGIN:${options.outputMarker}>>`,
        'ascii',
      )
      const payload = Buffer.alloc(options.outputBytes, 0x78)
      const end = Buffer.from(
        `<<CC_DESK_PROBE_OUTPUT_END:${options.outputMarker}>>`,
        'ascii',
      )
      process.stdout.write(Buffer.concat([begin, payload, end]))
    }
    process.exitCode = options.exitCode
  }

  if (!options.captureInput) {
    finish()
    return
  }

  if (process.stdin.isTTY && typeof process.stdin.setRawMode === 'function') {
    process.stdin.setRawMode(true)
  }
  process.stdin.resume()

  const startReading = () => {
    process.stdin.on('data', chunk => {
      try {
        const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)
        const remaining = options.captureBytes - capturedLength
        if (bytes.length > remaining) {
          fail('input exceeded --capture-bytes')
        }
        captured.push(bytes)
        capturedLength += bytes.length
        if (capturedLength === options.captureBytes) finish()
      } catch (error) {
        process.stdin.pause()
        reportFailure(error)
      }
    })
    process.stdin.on('end', () => {
      if (!finished && capturedLength !== options.captureBytes) {
        reportFailure(new Error('input ended before --capture-bytes'))
      }
    })
    announceReady()
  }

  if (options.captureBytes === 0) {
    announceReady()
    finish()
  } else if (options.delayReadMs > 0) {
    setTimeout(startReading, options.delayReadMs)
  } else {
    startReading()
  }
}

try {
  main()
} catch (error) {
  reportFailure(error)
}
