import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { TextEncoder } from 'node:util'
import { describe, expect, it } from 'vitest'

const template = readFileSync('src-tauri/src/cli/document_bootstrap.js', 'utf8')

function realm(fail = false) {
  const calls: { command: string; body: Uint8Array; options: { headers: Record<string, string> } }[] = []
  const context: Record<string, any> = { URL, TextEncoder, location: { href: 'http://tauri.localhost/index.html' } }
  context.window = context
  context.top = context
  context.__TAURI_INTERNALS__ = {}
  Object.defineProperty(context.__TAURI_INTERNALS__, 'invoke', { value: (command: string, body: Uint8Array, options: any) => {
    calls.push({ command, body, options })
    return fail ? Promise.reject({ code: 'TRANSPORT_LOST' }) : Promise.resolve('receipt')
  } })
  runInNewContext(template.replace('__CC_DESK_DOCUMENT_PROOF__', JSON.stringify('1234567890abcdef1234567890abcdef')).replace('__CC_DESK_DOCUMENT_URL__', JSON.stringify(context.location.href)), context)
  return { bridge: context.__CC_DESK_DOCUMENT__, calls }
}

describe('D11 native output channel handoff', () => {
  it('D11_Channel_HeaderKeepsBody_001', async () => {
    const { bridge, calls } = realm()
    const payload = { argv: ['', '中文', 'a b'] }
    await bridge.invoke('cli_start', payload, { toJSON: () => '__CHANNEL__:4294967295' })
    expect(calls).toHaveLength(1)
    expect(calls[0].options.headers['x-cc-desk-output-channel']).toBe('__CHANNEL__:4294967295')
    expect(Array.from(calls[0].body)).toEqual(Array.from(new TextEncoder().encode(JSON.stringify(payload))))
  })

  it('D11_Channel_RejectDescriptor_002', async () => {
    const { bridge, calls } = realm()
    for (const channel of [null, {}, { toJSON: () => '__CHANNEL__:01' }, { toJSON: () => '__CHANNEL__:4294967296' }, { toJSON: () => 'private-value' }]) {
      await expect(bridge.invoke('cli_start', {}, channel)).rejects.toMatchObject({ code: 'INVALID_REQUEST' })
    }
    expect(calls).toHaveLength(0)
  })

  it('D11_Channel_SerializeFailure_003', async () => {
    const { bridge, calls } = realm()
    await expect(bridge.invoke('cli_start', {}, { toJSON() { throw new Error('private-value') } })).rejects.toMatchObject({ code: 'INVALID_REQUEST' })
    expect(calls).toHaveLength(0)
  })

  it('D11_Channel_NoRetryOnLoss_004', async () => {
    const { bridge, calls } = realm(true)
    await expect(bridge.invoke('cli_start', {}, { toJSON: () => '__CHANNEL__:0' })).rejects.toMatchObject({ code: 'TRANSPORT_LOST' })
    expect(calls).toHaveLength(1)
    expect(calls[0].options.headers['x-cc-desk-output-channel']).toBe('__CHANNEL__:0')
  })
})
