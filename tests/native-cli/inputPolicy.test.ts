import { describe, expect, it } from 'vitest'
import {
  classifyClipboardSnapshot,
  createImeInputPolicy,
  isPasteShortcut,
  readClipboardSnapshot,
} from '@/terminal/inputPolicy'

describe('D18 clipboard arbitration', () => {
  it('D18_Clipboard_TextWinsOverImageEvidence_001', () => {
    expect(classifyClipboardSnapshot({
      text: 'hello',
      types: ['text/plain', 'image/png'],
    })).toEqual({ kind: 'text', text: 'hello' })
  })

  it('D18_Clipboard_EmptyTextIsNotGuessedAsImage_002', () => {
    expect(classifyClipboardSnapshot({
      text: '',
      types: [],
    })).toEqual({ kind: 'empty' })
  })

  it('D18_Clipboard_ReadFailureIsNotGuessedAsImage_003', () => {
    expect(classifyClipboardSnapshot({
      textError: new Error('permission denied'),
      types: [],
    })).toEqual({ kind: 'unavailable' })
  })

  it('D18_Clipboard_ImageRequiresPositiveMimeEvidence_004', () => {
    expect(classifyClipboardSnapshot({
      text: '',
      types: ['image/png'],
    })).toEqual({ kind: 'image' })
  })

  it('D18_Clipboard_AsyncProbeNeedsPositiveImageEvidence_005', async () => {
    await expect(readClipboardSnapshot(
      async () => '',
      async () => { throw new Error('no image') },
    )).resolves.toEqual({ kind: 'empty' })

    await expect(readClipboardSnapshot(
      async () => { throw new Error('text denied') },
      async () => ({ width: 1, height: 1 }),
    )).resolves.toEqual({ kind: 'image' })

    await expect(readClipboardSnapshot(
      async () => { throw new Error('text denied') },
      async () => { throw new Error('no image') },
    )).resolves.toEqual({ kind: 'unavailable' })
  })

  it('D18_Keyboard_OnlyCanonicalPasteShortcutIsIntercepted_006', () => {
    expect(isPasteShortcut({ key: 'v', ctrlKey: true, metaKey: false, altKey: false })).toBe(true)
    expect(isPasteShortcut({ key: 'V', ctrlKey: false, metaKey: true, altKey: false })).toBe(true)
    expect(isPasteShortcut({ key: 'v', ctrlKey: false, metaKey: false, altKey: false })).toBe(false)
    expect(isPasteShortcut({ key: 'v', ctrlKey: true, metaKey: true, altKey: false })).toBe(false)
    expect(isPasteShortcut({ key: 'v', ctrlKey: true, metaKey: false, altKey: true })).toBe(false)
  })
})

describe('D18 IME provenance policy', () => {
  it('D18_IME_ComposedLeakIsForwardedExactlyOnceAsExplicitUserText_007', () => {
    const policy = createImeInputPolicy()
    policy.keyDown()
    expect(policy.input({
      inputType: 'insertText',
      composed: true,
      data: '拼音',
    })).toBe('拼音')
    expect(policy.input({
      inputType: 'insertText',
      composed: true,
      data: '拼音',
    })).toBeUndefined()
  })

  it('D18_IME_XtermDataSuppressesFallbackDuplicate_008', () => {
    const policy = createImeInputPolicy()
    policy.keyDown()
    policy.xtermData()
    expect(policy.input({
      inputType: 'insertText',
      composed: true,
      data: 'I',
    })).toBeUndefined()
  })

  it('D18_IME_RealCompositionLifecycleStaysWithXterm_009', () => {
    const policy = createImeInputPolicy()
    policy.keyDown()
    policy.compositionStart()
    expect(policy.input({
      inputType: 'insertText',
      composed: true,
      data: '中',
    })).toBeUndefined()
  })
})
