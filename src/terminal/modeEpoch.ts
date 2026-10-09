import type { U64String } from '@/types/cli'

interface XtermModeTerminal {
  readonly modes: {
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
  onWriteParsed(handler: () => void): { dispose(): void }
}

export interface XtermModeEpoch {
  current(): U64String
  dispose(): void
}

const MAX_U64 = BigInt('18446744073709551615')

function fingerprint(modes: XtermModeTerminal['modes']): string {
  return [
    modes.applicationCursorKeysMode, modes.applicationKeypadMode, modes.bracketedPasteMode,
    modes.insertMode, modes.mouseTrackingMode, modes.originMode,
    modes.reverseWraparoundMode, modes.sendFocusMode, modes.wraparoundMode,
  ].join('|')
}

/** Samples all public xterm modes at parsed-batch and input boundaries. Changes
 * within one parsed batch that return to the same state are not observable. */
export function createXtermModeEpoch(terminal: XtermModeTerminal): XtermModeEpoch {
  let disposed = false
  let exhausted = false
  let epoch = BigInt(1)
  let last = fingerprint(terminal.modes)
  const sample = () => {
    if (disposed || exhausted) return
    const next = fingerprint(terminal.modes)
    if (next === last) return
    if (epoch === MAX_U64) {
      exhausted = true
      return
    }
    last = next
    epoch += BigInt(1)
  }
  const subscription = terminal.onWriteParsed(sample)

  return {
    current() {
      if (disposed) throw new Error('NATIVE_MODE_EPOCH_DISPOSED')
      sample()
      if (exhausted) throw new Error('NATIVE_MODE_EPOCH_EXHAUSTED')
      return epoch.toString()
    },
    dispose() {
      if (disposed) return
      disposed = true
      subscription.dispose()
    },
  }
}
