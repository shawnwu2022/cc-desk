import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { TextEncoder } from 'node:util'
import { describe, expect, it } from 'vitest'

const template = readFileSync('src-tauri/src/cli/document_bootstrap.js', 'utf8')
const expectedUrl = 'http://tauri.localhost/index.html'
const proof = '1234567890abcdef1234567890abcdef'
const script = (token = proof) => template
  .replace('__CC_DESK_DOCUMENT_URL__', JSON.stringify(expectedUrl))
  .replace('__CC_DESK_DOCUMENT_PROOF__', JSON.stringify(token))
  .replace('__CC_DESK_DOCUMENT_INSTANCE__', JSON.stringify('backend-bootstrap'))

function realm(url = expectedUrl, iframe = false) {
  const calls: { command: string; body: Uint8Array; options: { headers: Record<string, string> } }[] = []
  const context: Record<string, any> = { URL, TextEncoder, location: { href: url } }
  context.window = context
  context.top = iframe ? {} : context
  context.__TAURI_INTERNALS__ = { invoke: (command: string, body: Uint8Array, options: any) => {
    calls.push({ command, body, options })
    return Promise.resolve('receipt')
  } }
  runInNewContext(script(), context)
  return { context, calls }
}

describe('D11 document-scoped bootstrap', () => {
  it('D11_Bootstrap_RawUtf8AndPrivateProof_01', async () => {
    const { context, calls } = realm(expectedUrl + '#tab')
    const payload = { argv: ['a b', '', '中文', '$HOME'], requestId: 'request' }
    expect(await context.__CC_DESK_DOCUMENT__.invoke('cli_start', payload)).toBe('receipt')
    expect(calls).toHaveLength(1)
    expect(calls[0].command).toBe('cli_start')
    expect(Array.from(calls[0].body)).toEqual(Array.from(new TextEncoder().encode(JSON.stringify(payload))))
    expect(calls[0].options.headers['x-cc-desk-document']).toBe(proof)
    expect(context.__CC_DESK_DOCUMENT__.instanceId).toBe('backend-bootstrap')
    expect(Object.getOwnPropertyDescriptor(context.__CC_DESK_DOCUMENT__, 'instanceId')?.writable).toBe(false)
    expect(Object.keys(context.__CC_DESK_DOCUMENT__)).toEqual(['invoke'])
    expect(JSON.stringify(context.__CC_DESK_DOCUMENT__)).not.toContain(proof)
  })

  it('D11_Bootstrap_NoSubframeOrForeignDocumentBridge_02', () => {
    for (const url of ['https://example.invalid/', 'http://tauri.localhost/other.html', 'http://tauri.localhost/index.html?other=1']) {
      expect(realm(url).context.__CC_DESK_DOCUMENT__).toBeUndefined()
    }
    expect(realm(expectedUrl, true).context.__CC_DESK_DOCUMENT__).toBeUndefined()
  })

  it('D11_Bootstrap_CannotReplaceCapturedProof_03', async () => {
    const { context, calls } = realm()
    const original = context.__CC_DESK_DOCUMENT__
    runInNewContext(script('different-token'), context)
    expect(context.__CC_DESK_DOCUMENT__).toBe(original)
    expect(Object.isFrozen(original)).toBe(true)
    expect(Object.getOwnPropertyDescriptor(context, '__CC_DESK_DOCUMENT__')?.configurable).toBe(false)
    await original.invoke('cli_get_launch_status', { requestId: 'request' })
    expect(calls[0].options.headers['x-cc-desk-document']).toBe(proof)
  })

  it('D11_Bootstrap_SerializationFailureDoesNotInvoke_04', async () => {
    const { context, calls } = realm()
    const payload: any = {}
    payload.circular = payload
    await expect(context.__CC_DESK_DOCUMENT__.invoke('cli_start', payload)).rejects.toMatchObject({ code: 'INVALID_REQUEST' })
    expect(calls).toHaveLength(0)
  })

  it('D11_Bootstrap_FailureIsNeverRetried_05', async () => {
    const { context } = realm()
    let count = 0
    context.__TAURI_INTERNALS__.invoke = () => { count++; return Promise.reject({ code: 'TRANSPORT_LOST' }) }
    await expect(context.__CC_DESK_DOCUMENT__.invoke('cli_start', {})).rejects.toMatchObject({ code: 'TRANSPORT_LOST' })
    expect(count).toBe(1)
  })
})
