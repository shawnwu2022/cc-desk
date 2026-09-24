import { expect, it } from 'vitest'
import { mkdtempSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

function capture(auth: boolean) {
  const dir = mkdtempSync(join(tmpdir(), 'observer-reporter-'))
  const record = join(dir, 'record.json')
  try {
    writeFileSync(join(dir, 'curl'), `#!${process.execPath}\nlet n=0;process.stdin.on('data',b=>n+=b.length);process.stdin.on('end',()=>require('node:fs').writeFileSync(${JSON.stringify(record)},JSON.stringify({n,args:process.argv,secret:process.env.CC_DESK_OBSERVER_CAPABILITY,accidentallyExported:process.env.capability})));\n`, { mode: 0o700 })
    const result = spawnSync('bash', [resolve('src-tauri/plugin/scripts/report-hook.sh')], {
      env: { ...process.env, PATH: `${dir}:${process.env.PATH}`, CC_BOX_HOOK_PORT: '12345', capability: 'preexisting-export', CC_DESK_OBSERVER_RUN: 'run', CC_DESK_OBSERVER_GENERATION: '1', CC_DESK_OBSERVER_CAPABILITY: auth ? '0123456789abcdef0123456789abcdef' : '' },
      input: Buffer.alloc(1024 * 1024, 32), timeout: 6000,
    })
    expect(result.status).toBe(0)
    return existsSync(record) ? JSON.parse(readFileSync(record, 'utf8')) : null
  } finally { rmSync(dir, { recursive: true, force: true }) }
}
it('D13_Reporter_EmptyCapabilityNeverUsesLegacyFallback_01', () => {
  expect(capture(false)).toBeNull()
})
it('D13_Reporter_BoundsBodyAndDoesNotExposeTokenInCurlArgvOrEnvironment_02', () => {
  const value = capture(true)
  expect(value.n).toBeLessThanOrEqual(65537)
  expect(value.n).toBeGreaterThan(65536)
  expect(JSON.stringify(value)).not.toContain('0123456789abcdef0123456789abcdef')
})
