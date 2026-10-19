import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { observeTerminalTitleChanges } from '@/terminal/titleRefresh'

beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())

function fixture() {
  let receive!: (value: string) => void
  const release = vi.fn(), refresh = vi.fn()
  const dispose = observeTerminalTitleChanges({ onTitleChange(listener) { receive = listener; return { dispose: release } } }, () => refresh)
  return { receive: (value: string) => receive(value), refresh, release, dispose }
}

it('TerminalTitle_BurstAndSteadyTitle_001 — 忙闲图标回到原名不补读，改名只通知一次', async () => {
  const f = fixture()
  f.receive('✳ A'); await vi.advanceTimersByTimeAsync(500)
  expect(f.refresh).toHaveBeenCalledOnce()
  f.receive('◐ A'); await vi.advanceTimersByTimeAsync(100); f.receive('✳ A')
  await vi.advanceTimersByTimeAsync(3000)
  expect(f.refresh).toHaveBeenCalledOnce()
  f.receive('◐ A'); f.receive('◐ B'); f.receive('✳ B')
  await vi.advanceTimersByTimeAsync(500)
  expect(f.refresh).toHaveBeenCalledTimes(2)
  expect(f.refresh).toHaveBeenLastCalledWith()
  await vi.advanceTimersByTimeAsync(120_000)
  expect(f.refresh).toHaveBeenCalledTimes(2)
  f.dispose()
})

it('TerminalTitle_BudgetAndDispose_002 — 高频标题有界，释放取消未发通知', async () => {
  const f = fixture()
  for (let i = 0; i < 10; i++) { f.receive(`title-${i}`); await vi.advanceTimersByTimeAsync(600) }
  expect(f.refresh.mock.calls.length).toBeLessThanOrEqual(3)
  f.receive('latest'); f.dispose()
  const count = f.refresh.mock.calls.length
  await vi.advanceTimersByTimeAsync(10_000)
  expect(f.refresh).toHaveBeenCalledTimes(count)
  expect(f.release).toHaveBeenCalledOnce()
})

it('TerminalTitle_InvalidOrUnowned_003 — 无 owner 或无界标题不排队', async () => {
  let receive!: (value: string) => void
  const capture = vi.fn(() => undefined)
  const dispose = observeTerminalTitleChanges({ onTitleChange(listener) { receive = listener; return { dispose() {} } } }, capture)
  receive(''); receive('x'.repeat(1025))
  expect(capture).not.toHaveBeenCalled()
  receive('valid')
  expect(capture).toHaveBeenCalledOnce()
  expect(vi.getTimerCount()).toBe(0)
  dispose()
})
