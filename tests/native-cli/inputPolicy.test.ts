import { describe, expect, it } from 'vitest'
import {
  decideTerminalKey,
  observeClipboardData,
  planClipboardPaste,
  shouldSupplementImeInput,
  type ClipboardObservation,
} from '@/terminal/inputPolicy'

const bracketed = { bracketedPasteMode: true, ignoreBracketedPasteMode: false }
const plain = { bracketedPasteMode: false, ignoreBracketedPasteMode: false }

function available(text: string, hasImage = false): ClipboardObservation {
  return { status: 'available', text, hasImage }
}

describe('D18 input policy', () => {
  it('D18_Paste_BracketedNormalizesOnlyLineEndingsAndWraps_001', () => {
    const plan = planClipboardPaste(available('a\r\nb\rc\n{"x": 1}'), bracketed)
    expect(plan).toEqual({
      kind: 'send-text',
      payload: '\x1b[200~a\nb\nc\n{"x": 1}\x1b[201~',
      risks: [],
    })
  })

  it('D18_Paste_NonBracketedSingleLineIsByteFaithful_002', () => {
    const text = '{"id":12345678901234567890,"v":"a  b"}'
    expect(planClipboardPaste(available(text), plain)).toEqual({
      kind: 'send-text',
      payload: text,
      risks: [],
    })
  })

  it('D18_Paste_NonBracketedMultilineRequiresExplicitConfirmationWithoutRewrite_003', () => {
    const text = 'first\r\nsecond\rthird'
    expect(planClipboardPaste(available(text), plain)).toEqual({
      kind: 'confirm-text',
      payload: text,
      risks: ['multiline'],
    })
  })

  it('D18_Paste_NonBracketedEscAndEmbeddedPasteEndRequireConfirmation_004', () => {
    const text = 'safe\x1b[31mred\x1b[201~tail'
    expect(planClipboardPaste(available(text), plain)).toEqual({
      kind: 'confirm-text',
      payload: text,
      risks: ['escape', 'paste-end'],
    })
  })

  it('D18_Paste_ClipboardUnavailableNeverMasqueradesAsImage_005', () => {
    expect(planClipboardPaste({ status: 'unavailable' }, bracketed)).toEqual({
      kind: 'hold',
      reason: 'clipboard-unavailable',
    })
  })

  it('D18_Paste_EmptyClipboardNeverSynthesizesImageShortcut_006', () => {
    expect(planClipboardPaste(available(''), bracketed)).toEqual({
      kind: 'hold',
      reason: 'clipboard-empty',
    })
  })

  it('D18_Paste_MixedClipboardRequiresExplicitChoice_007', () => {
    const plan = planClipboardPaste(available('caption\r\nline', true), bracketed)
    expect(plan).toEqual({
      kind: 'choose-mixed',
      textPayload: '\x1b[200~caption\nline\x1b[201~',
      risks: [],
    })
  })

  it('D18_Paste_ImageOnlyIsExplicitAndCarriesNoHardcodedShortcut_008', () => {
    expect(planClipboardPaste(available('', true), bracketed)).toEqual({
      kind: 'image-only',
    })
  })

  it('D18_Keys_PasteGestureHasOneOwnerAndShiftEnterIsNotRewritten_009', () => {
    expect(decideTerminalKey({ type: 'keydown', key: 'v', ctrlKey: true }, false))
      .toBe('defer-to-paste-event')
    expect(decideTerminalKey({ type: 'keydown', key: 'v', metaKey: true }, false))
      .toBe('defer-to-paste-event')
    expect(decideTerminalKey({ type: 'keydown', key: 'Insert', shiftKey: true }, false))
      .toBe('defer-to-paste-event')
    expect(decideTerminalKey({ type: 'keydown', key: 'Enter', shiftKey: true }, false))
      .toBe('pass-through')
  })

  it('D18_Keys_CopyOnlyInterceptsWhenPolicyOwnsCopy_010', () => {
    expect(decideTerminalKey({ type: 'keydown', key: 'c', ctrlKey: true }, true))
      .toBe('copy-selection')
    expect(decideTerminalKey({ type: 'keydown', key: 'c', ctrlKey: true }, false))
      .toBe('pass-through')
    expect(decideTerminalKey({ type: 'keydown', key: 'c', metaKey: true }, true))
      .toBe('copy-selection')
    expect(decideTerminalKey({ type: 'keydown', key: 'd', ctrlKey: true }, false))
      .toBe('pass-through')
  })

  it('D18_IME_SupplementOnlyExactXtermDropBranch_011', () => {
    expect(shouldSupplementImeInput(
      { inputType: 'insertText', composed: true, data: 'ni' },
      { keyDownSeen: true, compositionSeen: false, dataSeen: false },
    )).toBe(true)
    expect(shouldSupplementImeInput(
      { inputType: 'insertText', composed: true, data: '你' },
      { keyDownSeen: true, compositionSeen: true, dataSeen: false },
    )).toBe(false)
  })

  it('D18_IME_DataAlreadyEmittedMustNeverDuplicate_012', () => {
    expect(shouldSupplementImeInput(
      { inputType: 'insertText', composed: true, data: 'I' },
      { keyDownSeen: true, compositionSeen: false, dataSeen: true },
    )).toBe(false)
    expect(shouldSupplementImeInput(
      { inputType: 'insertCompositionText', composed: true, data: 'ni' },
      { keyDownSeen: true, compositionSeen: false, dataSeen: false },
    )).toBe(false)
  })

  it('D18_ClipboardData_MissingIsUnavailable_013', () => {
    expect(observeClipboardData(null)).toEqual({ status: 'unavailable' })
  })

  it('D18_ClipboardData_TextOnlyIsNotImage_014', () => {
    expect(observeClipboardData({
      types: ['text/plain'],
      getData: type => type === 'text/plain' ? 'hello' : '',
      files: [],
    })).toEqual({ status: 'available', text: 'hello', hasImage: false })
  })

  it('D18_ClipboardData_ImageMimeIsImageOnly_015', () => {
    expect(observeClipboardData({
      types: ['image/png'],
      getData: () => '',
      files: [],
    })).toEqual({ status: 'available', text: '', hasImage: true })
  })

  it('D18_ClipboardData_MixedTextAndImageFileIsExplicitlyMixed_016', () => {
    expect(observeClipboardData({
      types: ['text/plain', 'Files'],
      getData: type => type === 'text/plain' ? 'caption' : '',
      files: [{ type: 'image/png' }],
    })).toEqual({ status: 'available', text: 'caption', hasImage: true })
  })
})
