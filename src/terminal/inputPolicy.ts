export interface ClipboardSnapshot {
  text?: string
  textError?: unknown
  types?: readonly string[]
}

export type ClipboardClassification =
  | { kind: 'text'; text: string }
  | { kind: 'image' }
  | { kind: 'empty' }
  | { kind: 'unavailable' }

export interface PasteShortcutLike {
  key: string
  ctrlKey: boolean
  metaKey: boolean
  altKey: boolean
}

export interface ImeInputLike {
  inputType: string
  composed: boolean
  data: string | null
}

export interface ImeInputPolicy {
  keyDown(): void
  keyUp(): void
  compositionStart(): void
  xtermData(): void
  input(event: ImeInputLike): string | undefined
}

function hasImageMime(types: readonly string[] | undefined): boolean {
  return types?.some(type => type.toLowerCase().startsWith('image/')) ?? false
}

/**
 * Clipboard classification is evidence-based. An empty text value or a failed
 * text read is never sufficient evidence that the clipboard contains an image.
 * Positive image MIME evidence is required before routing an image-paste key.
 */
export function classifyClipboardSnapshot(snapshot: ClipboardSnapshot): ClipboardClassification {
  if (snapshot.text) {
    return { kind: 'text', text: snapshot.text }
  }

  if (hasImageMime(snapshot.types)) {
    return { kind: 'image' }
  }

  if (snapshot.textError !== undefined) {
    return { kind: 'unavailable' }
  }

  return { kind: 'empty' }
}

/**
 * Only the ordinary platform paste chord is intercepted here. Modified chords
 * remain xterm/CLI input rather than being silently reinterpreted by Desk.
 */
export function isPasteShortcut(event: PasteShortcutLike): boolean {
  if (event.altKey) return false
  if (event.ctrlKey === event.metaKey) return false
  return event.key.toLowerCase() === 'v'
}

/**
 * Mirrors only the xterm 5.5 composed-input gap already observed by Desk.
 * A fallback value is emitted at most once per keydown cycle and only when
 * xterm has not emitted data and no real composition lifecycle was observed.
 */
export function createImeInputPolicy(): ImeInputPolicy {
  let keyDownSeen = false
  let compositionSeen = false
  let dataSeen = false
  let fallbackSent = false

  return {
    keyDown() {
      keyDownSeen = true
      compositionSeen = false
      dataSeen = false
      fallbackSent = false
    },
    keyUp() {
      keyDownSeen = false
    },
    compositionStart() {
      compositionSeen = true
    },
    xtermData() {
      dataSeen = true
    },
    input(event) {
      if (
        event.inputType !== 'insertText'
        || !event.composed
        || !event.data
        || !keyDownSeen
        || compositionSeen
        || dataSeen
        || fallbackSent
      ) {
        return undefined
      }
      fallbackSent = true
      return event.data
    },
  }
}
