import { describe, expect, it, vi } from 'vitest'
import { createActivityDiagnostics } from '@/utils/activityDiagnostics'

describe('ActivityDiagnostics', () => {
  it('ActivityDiagnostics_Epoch_001 allows only future events and rejects stale operations', () => {
    const sink = createActivityDiagnostics()
    expect(sink.start('archive', 'codex')).toBeUndefined()
    sink.enable()
    const op = sink.start('archive', 'codex')!
    op.record('backend', 'ok')
    sink.disable()
    op.record('list', 'ok')
    expect(sink.snapshot().events.map(e => e.stage)).toEqual(['ui', 'backend'])
    sink.enable()
    op.record('list', 'ok')
    expect(sink.snapshot().events).toEqual([])
  })

  it('ActivityDiagnostics_Budget_002 keeps bounded safe fields and a loss count', () => {
    const sink = createActivityDiagnostics()
    sink.enable()
    for (let i = 0; i < 700; ++i) {
      sink.start('delete', 'claude')?.record('provider', 'unknown')
    }
    const snapshot = sink.snapshot()
    expect(snapshot.events.length).toBe(256)
    expect(snapshot.dropped).toBe(1144)
    expect(JSON.stringify(snapshot).length).toBeLessThan(64 * 1024)
    expect(Object.keys(snapshot.events[0]).sort()).toEqual(['cli','id','ms','operation','result','stage'].sort())
  })
})

it('ActivityDiagnostics_Privacy_003 never serializes errors or accepts untyped events', async () => {
  const sink = createActivityDiagnostics()
  sink.enable()
  const first = sink.start('rename', 'claude')!
  const second = sink.start('archive', 'codex')!
  const secret = new Error('CANARY: title body full path args and credential')
  await expect(first.observe('adapter', async () => { throw secret })).rejects.toBe(secret)
  second.record('backend', 'ok')
  second.record('list', secret.message as never)
  const { events } = sink.snapshot()
  expect(events.map(event => event.id)).toEqual([events[0].id, events[1].id, events[0].id, events[1].id])
  expect(events[0].id).not.toBe(events[1].id)
  expect(events[0].id).toMatch(/^[0-9a-f]{32}$/)
  expect(JSON.stringify(events)).not.toContain('CANARY')
})

it('ActivityDiagnostics_ClockFault_005 keeps original outcome when recording fails', async () => {
  const sink = createActivityDiagnostics()
  sink.enable()
  const op = sink.start('archive', 'codex')!
  const clock = vi.spyOn(performance, 'now').mockImplementation(() => { throw new Error('diagnostic unavailable') })
  try {
    expect(sink.start('archive', 'codex')).toBeUndefined()
    await expect(op.observe('list', async () => 17)).resolves.toBe(17)
    const expected = new Error('business failure')
    await expect(op.observe('adapter', async () => { throw expected })).rejects.toBe(expected)
  } finally { clock.mockRestore() }
  await op.observe('list', async () => undefined)
  expect(sink.snapshot().events.map(e => e.result)).toEqual(['started','returned'])
})
