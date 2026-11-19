import { afterEach, expect, it, vi } from 'vitest'
import { activityDiagnostics } from '@/utils/activityDiagnostics'
import { createCaptureControl } from '@/utils/diagnosticsCaptureControl'

afterEach(() => { activityDiagnostics.disable(); vi.useRealTimers() })

it('ActivityDiagnostics_Ack_006 stops accepting immediately and never reopens on a late ACK', async () => {
  let reply: (value: unknown) => void = () => undefined
  const invoke = vi.fn(async (request: { action: string }) => request.action === 'start'
    ? new Promise(resolve => { reply = resolve })
    : { state: 'stopped' })
  const control = createCaptureControl(invoke)
  const starting = control.enable()
  expect(control.state.value).toBe('starting')
  await control.disable()
  reply({ state: 'started', grant: 'a'.repeat(32) })
  await starting
  expect(control.state.value).toBe('off')
  expect(invoke.mock.calls).toEqual([[{ action: 'start', serial: 1 }], [{ action: 'stop', serial: 2 }]])
  expect(activityDiagnostics.start('archive', 'codex')).toBeUndefined()
})

it('ActivityDiagnostics_UnknownAck_007 stays locked until a stop acknowledgement is received', async () => {
  vi.useFakeTimers()
  let stopReply: (value: unknown) => void = () => undefined
  const invoke = vi.fn(async (request: { action: string }) => request.action === 'start'
    ? { state: 'started', grant: 'a'.repeat(32) }
    : new Promise(resolve => { stopReply = resolve }))
  const control = createCaptureControl(invoke)
  await control.enable()
  const operation = activityDiagnostics.start('archive', 'codex')!
  expect(operation.traceHeader()).toMatch(/^a{32}:[0-9a-f]{32}$/)
  const stopping = control.disable()
  expect(control.state.value).toBe('stopping')
  expect(operation.traceHeader()).toBeUndefined()
  await vi.advanceTimersByTimeAsync(5000)
  await stopping
  expect(control.state.value).toBe('unknown')
  await control.enable()
  expect(control.state.value).toBe('unknown')
  await control.disable()
  expect(invoke).toHaveBeenCalledTimes(2)
  stopReply({ state: 'stopped' })
  await vi.advanceTimersByTimeAsync(0)
  expect(control.state.value).toBe('off')
})

it('ActivityDiagnostics_Report_008 filters unmatched IDs and strips raw provider values', async () => {
  let id: string
  const control = createCaptureControl(async request => request.action === 'start'
    ? { state: 'started', grant: 'a'.repeat(32) }
    : { state: 'snapshot', report: { dropped: 1, events: [
      { id, stage: 'backend', status: 'returned', ms: 5, secret: 'NEVER_EXPORT' },
      { id: 'b'.repeat(32), stage: 'backend', status: 'returned', ms: 6 },
      { id, stage: 'provider', status: 'NEVER_EXPORT', ms: 7 },
    ] } })
  await control.enable()
  id = activityDiagnostics.start('rename', 'claude')!.traceHeader()!.slice(33)
  const report = await control.readReport()
  expect(report.backend).toEqual({ available: true, dropped: 1, omitted: 2, events: [{ id, stage: 'backend', status: 'returned', ms: 5 }] })
  expect(JSON.stringify(report)).not.toMatch(/a{32}|NEVER_EXPORT/)
})

it('ActivityDiagnostics_PreviewBudget_010 allows one snapshot IPC even after local timeout', async () => {
  vi.useFakeTimers()
  let complete: (value: unknown) => void = () => undefined
  const invoke = vi.fn(() => new Promise(resolve => { complete = resolve }))
  const control = createCaptureControl(invoke)
  const first = control.readReport()
  expect(control.snapshotPending.value).toBe(true)
  await control.readReport()
  await vi.advanceTimersByTimeAsync(5000)
  expect((await first).backend.available).toBe(false)
  await control.readReport()
  expect(invoke).toHaveBeenCalledExactlyOnceWith({ action: 'snapshot' })
  expect(control.snapshotPending.value).toBe(true)
  complete({ state: 'snapshot', report: { dropped: 0, events: [] } })
  await vi.advanceTimersByTimeAsync(0)
  expect(control.snapshotPending.value).toBe(false)
})
