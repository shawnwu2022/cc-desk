import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import { bindNativePaste, buildPastePayload, commitPaste } from '@/utils/pasteText'

interface PasteFixture {
  name: string
  source: string
  prepared: string
  repeat: number
  prefix?: string
  suffix?: string
  separator?: string
  isJson?: boolean
}

const fixtures: PasteFixture[] = JSON.parse(readFileSync(
  resolve(__dirname, '../../src-tauri/tests/fixtures/devtools-paste-framing.json'), 'utf8',
))
const compose = (fixture: PasteFixture, text: string) =>
  (fixture.prefix ?? '') + Array(fixture.repeat).fill(text).join(fixture.separator ?? '') + (fixture.suffix ?? '')
const expectedPayload = (fixture: PasteFixture) =>
  '\x1b[200~' + compose(fixture, fixture.prepared) + '\x1b[201~'

describe('DevTools paste framing contract', () => {
  // 同一黄金样本用于 Rust 真 ConPTY 测试；禁止把标记丢失当成传输成功。
  it.each(fixtures)('DevtoolsPayload_Golden_001 $name', fixture => {
    const source = compose(fixture, fixture.source)
    if (fixture.isJson) expect(() => JSON.parse(source)).not.toThrow()
    expect(buildPastePayload(source, true, false)).toBe(expectedPayload(fixture))
  })

  it.each(fixtures)('DevtoolsPaste_KeyboardCommit_002 $name', async fixture => {
    const delivered: string[] = []
    await commitPaste(
      async () => compose(fixture, fixture.source),
      () => ({ ptyId: 'same-pty' }),
      text => buildPastePayload(text, true, false),
      async (id, payload) => { delivered.push(id, payload) },
    )
    expect(delivered).toEqual(['same-pty', expectedPayload(fixture)])
  })

  it.each(fixtures)('DevtoolsPaste_NativeMenu_003 $name', async fixture => {
    const container = document.createElement('div')
    const terminal = document.createElement('div')
    const textarea = document.createElement('textarea')
    container.append(terminal)
    terminal.append(textarea)
    const delivered: string[] = []
    const instance = {
      ptyId: 'same-pty',
      term: { element: terminal, modes: { bracketedPasteMode: true }, options: {} },
    }
    const unbind = bindNativePaste({
      container,
      getTabId: () => 'tab',
      getInstance: () => instance,
      write: async (id, payload) => { delivered.push(id, payload) },
      imageFallback: () => '\x1bv',
    })
    try {
      const event = new Event('paste', { bubbles: true, cancelable: true })
      Object.defineProperty(event, 'clipboardData', {
        value: { getData: () => compose(fixture, fixture.source) },
      })
      textarea.dispatchEvent(event)
      await vi.waitFor(() => expect(delivered).toEqual(['same-pty', expectedPayload(fixture)]))
      expect(event.defaultPrevented).toBe(true)
    } finally {
      unbind()
    }
  })
})
