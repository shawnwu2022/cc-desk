export type ClipboardObservation =
  | {
      status: 'available'
      text: string
      hasImage: boolean
    }
  | {
      status: 'unavailable'
    }

export interface ClipboardDataLike {
  types?: ArrayLike<string>
  getData: (type: string) => string
  files?: ArrayLike<{ type?: string }>
}

export type PasteRisk = 'multiline' | 'escape' | 'paste-end'

export type PastePlan =
  | {
      kind: 'send-text'
      payload: string
      risks: []
    }
  | {
      kind: 'confirm-text'
      payload: string
      risks: PasteRisk[]
    }
  | {
      kind: 'choose-mixed'
      textPayload: string
      risks: PasteRisk[]
    }
  | {
      kind: 'image-only'
    }
  | {
      kind: 'hold'
      reason: 'clipboard-empty' | 'clipboard-unavailable'
    }

export interface PasteModeSnapshot {
  bracketedPasteMode: boolean
  ignoreBracketedPasteMode: boolean
}

export interface TerminalKeyLike {
  type: string
  key: string
  ctrlKey?: boolean
  metaKey?: boolean
  shiftKey?: boolean
}

export type TerminalKeyDecision =
  | 'copy-selection'
  | 'defer-to-paste-event'
  | 'pass-through'

export interface ImeInputLike {
  inputType: string
  composed: boolean
  data: string | null
}

export interface ImePolicyState {
  keyDownSeen: boolean
  compositionSeen: boolean
  dataSeen: boolean
}

const BRACKETED_PASTE_START = '\x1b[200~'
const BRACKETED_PASTE_END = '\x1b[201~'

function normalizeBracketedLineEndings(text: string): string {
  return text.replace(/\r\n?/g, '\n')
}

function collectNonBracketedRisks(text: string): PasteRisk[] {
  const risks: PasteRisk[] = []
  if (/[\r\n]/.test(text)) risks.push('multiline')
  if (text.includes('\x1b')) risks.push('escape')
  if (text.includes(BRACKETED_PASTE_END)) risks.push('paste-end')
  return risks
}

function planTextPayload(text: string, mode: PasteModeSnapshot): {
  payload: string
  risks: PasteRisk[]
} {
  const bracketed = mode.bracketedPasteMode && !mode.ignoreBracketedPasteMode
  if (bracketed) {
    return {
      payload: BRACKETED_PASTE_START + normalizeBracketedLineEndings(text) + BRACKETED_PASTE_END,
      risks: [],
    }
  }

  return {
    payload: text,
    risks: collectNonBracketedRisks(text),
  }
}

export function observeClipboardData(
  data: ClipboardDataLike | null | undefined,
): ClipboardObservation {
  if (!data) return { status: 'unavailable' }

  let text: string
  try {
    text = data.getData('text/plain')
  } catch {
    return { status: 'unavailable' }
  }

  const types = Array.from(data.types ?? [], value => String(value).toLowerCase())
  const files = Array.from(data.files ?? [])
  const hasImage = types.some(type => type.startsWith('image/'))
    || files.some(file => String(file.type ?? '').toLowerCase().startsWith('image/'))

  return {
    status: 'available',
    text,
    hasImage,
  }
}

export function planClipboardPaste(
  observation: ClipboardObservation,
  mode: PasteModeSnapshot,
): PastePlan {
  if (observation.status === 'unavailable') {
    return { kind: 'hold', reason: 'clipboard-unavailable' }
  }

  const { text, hasImage } = observation
  if (!text) {
    if (hasImage) return { kind: 'image-only' }
    return { kind: 'hold', reason: 'clipboard-empty' }
  }

  const planned = planTextPayload(text, mode)

  if (hasImage) {
    return {
      kind: 'choose-mixed',
      textPayload: planned.payload,
      risks: planned.risks,
    }
  }

  if (planned.risks.length > 0) {
    return {
      kind: 'confirm-text',
      payload: planned.payload,
      risks: planned.risks,
    }
  }

  return {
    kind: 'send-text',
    payload: planned.payload,
    risks: [],
  }
}

export function decideTerminalKey(
  event: TerminalKeyLike,
  hasSelection: boolean,
): TerminalKeyDecision {
  if (event.type !== 'keydown') return 'pass-through'

  const key = event.key.toLowerCase()
  const modifierPaste = key === 'v' && Boolean(event.ctrlKey || event.metaKey)
  const shiftInsert = event.key === 'Insert' && Boolean(event.shiftKey)
  if (modifierPaste || shiftInsert) return 'defer-to-paste-event'

  const modifierCopy = key === 'c' && Boolean(event.ctrlKey || event.metaKey)
  if (modifierCopy && hasSelection) return 'copy-selection'

  return 'pass-through'
}

export function shouldSupplementImeInput(
  event: ImeInputLike,
  state: ImePolicyState,
): boolean {
  return event.inputType === 'insertText'
    && event.composed
    && Boolean(event.data)
    && state.keyDownSeen
    && !state.compositionSeen
    && !state.dataSeen
}
