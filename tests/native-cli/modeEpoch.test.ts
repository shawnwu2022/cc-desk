import { describe, expect, it, vi } from 'vitest'
import { createXtermModeEpoch } from '@/terminal/modeEpoch'

describe('Native parsed mode epochs', () => {
  // 九个公开模式逐项变化都必须使已保留的输入失去旧 epoch。
  it.each([
    ['applicationCursorKeysMode', true], ['applicationKeypadMode', true], ['bracketedPasteMode', true],
    ['insertMode', true], ['mouseTrackingMode', 'x10'], ['originMode', true],
    ['reverseWraparoundMode', true], ['sendFocusMode', true], ['wraparoundMode', false],
  ])('Native_TrackPublicMode_001 %s', (name, value) => {
    const modes: any = {
      applicationCursorKeysMode: false, applicationKeypadMode: false, bracketedPasteMode: false,
      insertMode: false, mouseTrackingMode: 'none', originMode: false,
      reverseWraparoundMode: false, sendFocusMode: false, wraparoundMode: true,
    }
    let parsed!: () => void
    const tracker = createXtermModeEpoch({ modes, onWriteParsed: callback => { parsed = callback; return { dispose() {} } } })
    expect(tracker.current()).toBe('1')
    modes[name] = value; parsed()
    expect(tracker.current()).toBe('2')
    parsed()
    expect(tracker.current()).toBe('2')
    tracker.dispose()
  })

  // 跨 parsed 批次的 A→B→A 不能被用户输入时的相同终值隐藏。
  it('Native_TrackParsedRoundTrip_002', () => {
    const modes = {
      applicationCursorKeysMode: false, applicationKeypadMode: false, bracketedPasteMode: false,
      insertMode: false, mouseTrackingMode: 'none', originMode: false,
      reverseWraparoundMode: false, sendFocusMode: false, wraparoundMode: true,
    }
    let parsed!: () => void
    const tracker = createXtermModeEpoch({ modes, onWriteParsed: callback => { parsed = callback; return { dispose() {} } } })
    modes.sendFocusMode = true; parsed()
    modes.sendFocusMode = false; parsed()
    expect(tracker.current()).toBe('3')
    parsed(); parsed()
    expect(tracker.current()).toBe('3')
    tracker.dispose()
  })

  // current 在 parsed 回调前采样，随后回调和普通输出不重复增加 epoch。
  it('Native_SampleCurrentOnce_003', () => {
    const modes = {
      applicationCursorKeysMode: false, applicationKeypadMode: false, bracketedPasteMode: false,
      insertMode: false, mouseTrackingMode: 'none', originMode: false,
      reverseWraparoundMode: false, sendFocusMode: false, wraparoundMode: true,
    }
    let parsed!: () => void
    const tracker = createXtermModeEpoch({ modes, onWriteParsed: callback => { parsed = callback; return { dispose() {} } } })
    modes.bracketedPasteMode = true
    expect(tracker.current()).toBe('2')
    expect(tracker.current()).toBe('2')
    parsed(); parsed()
    expect(tracker.current()).toBe('2')
    tracker.dispose()
  })

  // dispose 解除监听并永久拒绝 current；迟到 parsed 回调不能重新开放。
  it('Native_DisposedEpochCloses_004', () => {
    const modes = {
      applicationCursorKeysMode: false, applicationKeypadMode: false, bracketedPasteMode: false,
      insertMode: false, mouseTrackingMode: 'none', originMode: false,
      reverseWraparoundMode: false, sendFocusMode: false, wraparoundMode: true,
    }
    let parsed!: () => void
    let subscriptions = 0
    const tracker = createXtermModeEpoch({ modes, onWriteParsed: callback => {
      parsed = callback; subscriptions++
      return { dispose() { subscriptions-- } }
    } })
    expect(subscriptions).toBe(1)
    tracker.dispose(); tracker.dispose()
    expect(subscriptions).toBe(0)
    modes.bracketedPasteMode = true
    expect(() => parsed()).not.toThrow()
    expect(() => tracker.current()).toThrow('NATIVE_MODE_EPOCH_DISPOSED')
  })

  // 只在测试中将初始计数置为 MAX-1；真实 u64 上限和生产 API 都保持不变。
  it.each(['parsed', 'current'] as const)('Native_ExhaustEpochClosed_005 %s', source => {
    const modes = {
      applicationCursorKeysMode: false, applicationKeypadMode: false, bracketedPasteMode: false,
      insertMode: false, mouseTrackingMode: 'none', originMode: false,
      reverseWraparoundMode: false, sendFocusMode: false, wraparoundMode: true,
    }
    let parsed!: () => void
    const originalBigInt = BigInt
    const initialize = vi.spyOn(globalThis, 'BigInt').mockImplementation(value => {
      if (value === 1) { initialize.mockRestore(); return originalBigInt('18446744073709551614') }
      return originalBigInt(value)
    })
    let tracker: ReturnType<typeof createXtermModeEpoch>
    try {
      tracker = createXtermModeEpoch({ modes, onWriteParsed: callback => { parsed = callback; return { dispose() {} } } })
    } finally { initialize.mockRestore() }
    expect(tracker.current()).toBe('18446744073709551614')
    modes.sendFocusMode = true; parsed()
    expect(tracker.current()).toBe('18446744073709551615')
    parsed()
    expect(tracker.current()).toBe('18446744073709551615')
    modes.sendFocusMode = false
    if (source === 'parsed') expect(() => parsed()).not.toThrow()
    expect(() => tracker.current()).toThrow('NATIVE_MODE_EPOCH_EXHAUSTED')
    modes.sendFocusMode = true; parsed()
    expect(() => tracker.current()).toThrow('NATIVE_MODE_EPOCH_EXHAUSTED')
    tracker.dispose()
  })
})
