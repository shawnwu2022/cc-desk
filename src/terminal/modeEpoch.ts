import type { U64String } from '@/types/cli'

const MAX_U64 = (1n << 64n) - 1n

export interface Disposable {
  dispose(): void
}

interface XtermModesLike {
  applicationCursorKeysMode: boolean
  applicationKeypadMode: boolean
  bracketedPasteMode: boolean
  insertMode: boolean
  mouseTrackingMode: string
  originMode: boolean
  reverseWraparoundMode: boolean
  sendFocusMode: boolean
  wraparoundMode: boolean
}

interface XtermModeTerminal {
  readonly modes: XtermModesLike
  onWriteParsed(handler: () => void): Disposable
}

export interface XtermModeEpoch extends Disposable {
  current(): U64String
}

function fingerprint(modes: XtermModesLike): string {
  return [
    modes.applicationCursorKeysMode ? '1' : '0',
    modes.applicationKeypadMode ? '1' : '0',
    modes.bracketedPasteMode ? '1' : '0',
    modes.insertMode ? '1' : '0',
    modes.mouseTrackingMode,
    modes.originMode ? '1' : '0',
    modes.reverseWraparoundMode ? '1' : '0',
    modes.sendFocusMode ? '1' : '0',
    modes.wraparoundMode ? '1' : '0',
  ].join('|')
}

/**
 * Tracks xterm's parsed terminal-mode state without interpreting output bytes.
 *
 * D16 binds user intents to this epoch. When CLI output changes any public xterm
 * mode, subsequent user actions receive a new epoch and already-reserved input
 * cannot be silently retargeted across the mode transition.
 */
export function createXtermModeEpoch(terminal: XtermModeTerminal): XtermModeEpoch {
  let disposed = false
  let epoch = 1n
  let last = fingerprint(terminal.modes)

  const subscription = terminal.onWriteParsed(() => {
    if (disposed) return
    const next = fingerprint(terminal.modes)
    if (next === last) return
    if (epoch === MAX_U64) {
      // There is no safe epoch to allocate. Freeze at the terminal limit; any
      // caller that needs another transition must replace the run/terminal.
      return
    }
    last = next
    epoch += 1n
  })

  return {
    current() {
      return epoch.toString() as U64String
    },

    dispose() {
      if (disposed) return
      disposed = true
      subscription.dispose()
    },
  }
}
