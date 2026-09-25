import { describe, expect, it } from 'vitest'
import { Terminal } from '@xterm/xterm'
import { createXtermModeEpoch } from '@/terminal/modeEpoch'

function write(term: Terminal, data: string): Promise<void> {
  return new Promise(resolve => term.write(data, resolve))
}

describe('D19 xterm mode epoch', () => {
  it('D19_ModeEpoch_PlainOutputDoesNotAdvance_014', async () => {
    const term = new Terminal()
    const modes = createXtermModeEpoch(term)

    expect(modes.current()).toBe('1')
    await write(term, 'hello')
    expect(modes.current()).toBe('1')

    modes.dispose()
    term.dispose()
  })

  it('D19_ModeEpoch_BracketedPasteTransitionsAdvanceExactlyOnce_015', async () => {
    const term = new Terminal()
    const modes = createXtermModeEpoch(term)

    await write(term, '\x1b[?2004h')
    expect(term.modes.bracketedPasteMode).toBe(true)
    expect(modes.current()).toBe('2')

    await write(term, '\x1b[?2004h')
    expect(modes.current()).toBe('2')

    await write(term, '\x1b[?2004l')
    expect(term.modes.bracketedPasteMode).toBe(false)
    expect(modes.current()).toBe('3')

    modes.dispose()
    term.dispose()
  })

  it('D19_ModeEpoch_FocusAndApplicationKeyTransitionsAdvance_016', async () => {
    const term = new Terminal()
    const modes = createXtermModeEpoch(term)

    await write(term, '\x1b[?1004h')
    expect(term.modes.sendFocusMode).toBe(true)
    expect(modes.current()).toBe('2')

    await write(term, '\x1b[?1h')
    expect(term.modes.applicationCursorKeysMode).toBe(true)
    expect(modes.current()).toBe('3')

    modes.dispose()
    term.dispose()
  })

  it('D19_ModeEpoch_DisposeStopsObservation_017', async () => {
    const term = new Terminal()
    const modes = createXtermModeEpoch(term)
    modes.dispose()

    await write(term, '\x1b[?2004h')
    expect(modes.current()).toBe('1')

    term.dispose()
  })
})
