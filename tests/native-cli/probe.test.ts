import { spawnSync } from 'node:child_process'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'

const probePath = resolve(process.cwd(), 'tests/fixtures/native-cli/probe.mjs')
const cleanup: string[] = []

function makeRoot(prefix: string): string {
  const root = mkdtempSync(join(tmpdir(), prefix))
  cleanup.push(root)
  return root
}

function fixtureEnv(root: string): NodeJS.ProcessEnv {
  return {
    ...process.env,
    CC_DESK_TEST_ROOT: root,
    CC_DESK_FIXTURE_VALUE: 'fixture-only',
    CC_DESK_SECRET_SHOULD_NOT_APPEAR: 'never-copy-this',
  }
}

function readReport(path: string) {
  return JSON.parse(readFileSync(path, 'utf8'))
}

afterEach(() => {
  while (cleanup.length > 0) {
    rmSync(cleanup.pop()!, { recursive: true, force: true })
  }
})

describe('native CLI probe contract', () => {
  it('D03_Probe_ReportRequired_01', () => {
    const root = makeRoot('cc-desk-probe-required-')
    const result = spawnSync(process.execPath, [probePath], {
      env: fixtureEnv(root),
      encoding: 'utf8',
    })

    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain('missing --report')
  })

  it('D03_Probe_ReportMustStayInsideTestRoot_02', () => {
    const root = makeRoot('cc-desk-probe-root-')
    const outside = makeRoot('cc-desk-probe-outside-')
    const report = join(outside, 'probe.json')
    const result = spawnSync(process.execPath, [probePath, '--report', report], {
      env: fixtureEnv(root),
      encoding: 'utf8',
    })

    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain('report path outside test root')
    expect(existsSync(report)).toBe(false)
  })

  it('D03_Probe_ReportsOnlyFixtureEnvironmentAndExactArgv_03', () => {
    const root = makeRoot('cc-desk-probe-report-')
    const cwd = join(root, '工作 目录')
    const report = join(root, 'reports', 'probe.json')
    mkdirSync(cwd, { recursive: true })
    mkdirSync(dirname(report), { recursive: true })

    const argv = ['--future', 'a b', '', '中文', '--', '-literal']
    const result = spawnSync(
      process.execPath,
      [probePath, '--report', report, '--', ...argv],
      { cwd, env: fixtureEnv(root), encoding: 'utf8' },
    )

    expect(result.status).toBe(0)
    expect(result.stdout).toBe('')
    expect(result.stderr).toBe('')

    const value = readReport(report)
    expect(value.argv).toEqual(argv)
    expect(resolve(value.cwd)).toBe(resolve(cwd))
    expect(value.stdinIsTTY).toBe(false)
    expect(value.stdoutIsTTY).toBe(false)
    expect(value.env).toEqual({ CC_DESK_FIXTURE_VALUE: 'fixture-only' })
    expect(JSON.stringify(value)).not.toContain('never-copy-this')
    expect(value.capturedBase64).toBeNull()
  })

  it('D03_Probe_CapturesRawInputOnlyWhenExplicitlyEnabled_04', () => {
    const root = makeRoot('cc-desk-probe-capture-')
    const report = join(root, 'capture.json')
    const input = Buffer.from([0x00, 0x1b, 0x7f, 0x80, 0xff])
    const result = spawnSync(
      process.execPath,
      [
        probePath,
        '--report', report,
        '--capture-input',
        '--capture-bytes', String(input.length),
      ],
      { env: fixtureEnv(root), input },
    )

    expect(result.status).toBe(0)
    expect(readReport(report).capturedBase64).toBe(input.toString('base64'))
  })

  it('D03_Probe_FramesOutputBeforeConfiguredNonzeroExit_05', () => {
    const root = makeRoot('cc-desk-probe-exit-')
    const report = join(root, 'exit.json')
    const marker = 'node-contract-05'
    const result = spawnSync(
      process.execPath,
      [
        probePath,
        '--report', report,
        '--output-bytes', '257',
        '--output-marker', marker,
        '--exit-code', '7',
      ],
      { env: fixtureEnv(root) },
    )

    const expected = Buffer.concat([
      Buffer.from(`<<CC_DESK_PROBE_OUTPUT_BEGIN:${marker}>>`, 'ascii'),
      Buffer.alloc(257, 0x78),
      Buffer.from(`<<CC_DESK_PROBE_OUTPUT_END:${marker}>>`, 'ascii'),
    ])
    expect(result.status).toBe(7)
    expect(result.stdout).toEqual(expected)
    expect(existsSync(report)).toBe(true)
    expect(readReport(report).requestedExitCode).toBe(7)
  })

  it('D03_Probe_OutputRequiresSafeMarker_06', () => {
    const root = makeRoot('cc-desk-probe-marker-')
    const report = join(root, 'marker.json')
    const result = spawnSync(
      process.execPath,
      [probePath, '--report', report, '--output-bytes', '1'],
      { env: fixtureEnv(root), encoding: 'utf8' },
    )

    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain('--output-bytes requires --output-marker')
    expect(existsSync(report)).toBe(false)
  })
})
