import { expect, it } from 'vitest'
import { mkdtempSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const token = '0123456789abcdef0123456789abcdef'
const script = 'src-tauri/plugin/scripts/report-hook.sh'
function capture(auth: boolean, input: Buffer = Buffer.from('{"hook_event_name":"Stop"}')) {
  const dir = mkdtempSync(join(tmpdir(), 'observer-reporter-'))
  const record = join(dir, 'record.json')
  try {
    writeFileSync(join(dir, 'curl'), `#!${process.execPath}\nconst parts=[];process.stdin.on('data',b=>parts.push(b));process.stdin.on('end',()=>require('node:fs').writeFileSync(${JSON.stringify(record)},JSON.stringify({config:Buffer.concat(parts).toString(),args:process.argv,secret:process.env.CC_DESK_OBSERVER_CAPABILITY,accidentallyExported:process.env.capability,bodyExported:process.env.payload})));\n`, { mode: 0o700 })
    const result = spawnSync('bash', [resolve(script)], {
      env: { ...process.env, PATH: `${dir}:${process.env.PATH}`, CC_BOX_HOOK_PORT: '12345', capability: 'preexisting-export', payload: 'preexisting-export', CC_DESK_OBSERVER_RUN: 'run', CC_DESK_OBSERVER_GENERATION: '1', CC_DESK_OBSERVER_CAPABILITY: auth ? token : '' },
      input, timeout: 6000,
    })
    expect(result.status).toBe(0)
    expect(result.stdout.length).toBe(0)
    expect(result.stderr.length).toBe(0)
    return existsSync(record) ? JSON.parse(readFileSync(record, 'utf8')) : null
  } finally { rmSync(dir, { recursive: true, force: true }) }
}
function body(config: string): string {
  const lines = config.split('\n').filter(line => line.startsWith('data-raw = '))
  expect(lines).toHaveLength(1)
  const quoted = lines[0].slice('data-raw = '.length)
  expect(quoted.startsWith('"') && quoted.endsWith('"')).toBe(true)
  const escapes: Record<string, string> = { '\\': '\\', '"': '"', n: '\n', r: '\r', t: '\t', v: '\v' }
  return quoted.slice(1, -1).replace(/\\([\\"nrtv])/g, (_, c: string) => escapes[c])
}
it('D13_Reporter_EmptyCapabilityNeverUsesLegacyFallback_01', () => {
  expect(capture(false)).toBeNull()
})
it('D13_Reporter_BoundsBodyAndDoesNotExposeTokenInCurlArgvOrEnvironment_02', () => {
  expect(capture(true, Buffer.alloc(65537, 32))).toBeNull()
  const value = capture(true, Buffer.alloc(65536, 32))
  expect(value.args.slice(-2)).toEqual(['--config', '-'])
  const { config, ...processData } = value
  expect(JSON.stringify(processData)).not.toContain(token)
  expect(value.accidentallyExported).toBeUndefined()
  expect(value.bodyExported).toBeUndefined()
  expect(config).toContain(`X-CC-Desk-Capability: ${token}`)
  expect(body(config)).toBe(' '.repeat(65536))
})
it('D13_Reporter_ConfigPipePreservesJsonBytesAndCannotInjectOptions_03', () => {
  const input = '\r\n\t' + JSON.stringify({ hook_event_name: 'UserPromptSubmit', prompt: '中文🙂\\ \\\"\nurl = "file:///must-not-read"\n--next\r\t' }) + '\n\n'
  const value = capture(true, Buffer.from(input))
  expect(value.args).not.toContain(input)
  expect(body(value.config)).toBe(input)
  expect(value.config.split('\n').filter((line: string) => line.startsWith('url'))).toHaveLength(0)
})
it('D13_Reporter_NulInputIsRejectedNotSilentlyRemoved_04', () => {
  expect(capture(true, Buffer.from('{"hook_event_name":"Stop"}\0'))).toBeNull()
})
it('D13_Reporter_CheckoutPinsLfOnWindows_05', () => {
  const result = spawnSync('git', ['check-attr', 'eol', '--', script], { encoding: 'utf8' })
  expect(result.status).toBe(0)
  expect(result.stdout.trim()).toBe(`${script}: eol: lf`)
})
