import { reactive } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import { TerminalRendererRegistry } from '@/utils/rendererRegistry'

describe('TerminalRendererRegistry', () => {
  // raw 与 proxy 身份并发调用 runOnce：只执行一次，两次拿到同一 Promise。
  it('RendererRegistry_ProxyRawSingleRun_001', async () => {
    const raw: object = { marker: 'terminal' }
    const proxy = reactive(raw)
    const run = vi.fn(async () => {})
    const registry = new TerminalRendererRegistry()

    const p1 = registry.runOnce(proxy, run)
    const p2 = registry.runOnce(raw, run)

    expect(p1).toBe(p2)
    await Promise.all([p1, p2])
    expect(run).toHaveBeenCalledTimes(1)
  })

  // runOnce 完成后再调用：复用缓存 Promise，不重复执行。
  it('RendererRegistry_ReuseAfterDone_002', async () => {
    const raw: object = {}
    const run = vi.fn(async () => {})
    const registry = new TerminalRendererRegistry()

    await registry.runOnce(raw, run)
    await registry.runOnce(reactive(raw), run)

    expect(run).toHaveBeenCalledTimes(1)
  })

  // runOnce 的 run 抛错：Promise 不 reject，缓存后不重复执行。
  it('RendererRegistry_SwallowRunError_003', async () => {
    const raw: object = {}
    let calls = 0
    const registry = new TerminalRendererRegistry()

    await expect(
      registry.runOnce(raw, async () => {
        calls += 1
        throw new Error('boom')
      }),
    ).resolves.toBeUndefined()
    await registry.runOnce(reactive(raw), async () => {
      calls += 1
    })

    expect(calls).toBe(1)
  })

  // markDisposed 用 raw 标记后，proxy 身份查询 isDisposed 也为 true（身份归一）。
  it('RendererRegistry_DisposedProxyView_004', () => {
    const raw: object = {}
    const proxy = reactive(raw)
    const registry = new TerminalRendererRegistry()

    expect(registry.isDisposed(proxy)).toBe(false)
    registry.markDisposed(raw)
    expect(registry.isDisposed(proxy)).toBe(true)
    expect(registry.isDisposed(raw)).toBe(true)
  })

  // setTimer 覆盖旧 stop 时自动执行旧值；clearTimer 用 proxy 身份也能停掉 raw 设置的 timer。
  it('RendererRegistry_TimerProxyCleanup_005', () => {
    vi.useFakeTimers()
    const raw: object = {}
    const proxy = reactive(raw)
    const registry = new TerminalRendererRegistry()
    const first = vi.fn()
    const second = vi.fn()

    const firstHandle = setInterval(first, 1_000)
    registry.setTimer(raw, () => clearInterval(firstHandle))
    const secondHandle = setInterval(second, 1_000)
    registry.setTimer(proxy, () => clearInterval(secondHandle)) // 覆盖，first 被停
    vi.advanceTimersByTime(2_000)
    expect(first).not.toHaveBeenCalled()
    expect(second).toHaveBeenCalled()

    registry.clearTimer(proxy) // proxy 身份清理 raw/proxy 任一设置的 timer
    const before = second.mock.calls.length
    vi.advanceTimersByTime(2_000)
    expect(second.mock.calls.length).toBe(before)
    vi.useRealTimers()
  })
})
