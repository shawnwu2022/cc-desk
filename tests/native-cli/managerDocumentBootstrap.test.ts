import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { TextEncoder } from 'node:util'
import { describe, expect, it } from 'vitest'

const template = readFileSync('src-tauri/src/version_history/manager_document_bootstrap.js', 'utf8')
const expectedUrl = 'http://tauri.localhost/version-manager.html'
const proof = '1234567890abcdef1234567890abcdef'
function realm(url = expectedUrl, child = false) {
  const calls: unknown[][] = []
  const context: any = { URL, TextEncoder, location: { href: url }, __TAURI_INTERNALS__: {
    invoke: (...args: unknown[]) => { calls.push(args); return Promise.resolve('receipt') },
  } }
  context.window = context
  context.top = child ? {} : context
  const script = template.replace('__MANAGER_PROOF__', JSON.stringify(proof)).replace('__MANAGER_URL__', JSON.stringify(expectedUrl))
  runInNewContext(script, context)
  return { context, calls, script }
}

describe('independent version manager document bridge', () => {
  it('ManagerBridge_RestrictedRawRequests_001', async () => {
    const { context, calls } = realm()
    expect(await context.__CC_DESK_VERSION_MANAGER__.invoke('inspect_version_switch', {})).toBe('receipt')
    expect(calls).toHaveLength(1)
    expect(calls[0][0]).toBe('inspect_version_switch')
    expect(new TextDecoder().decode(calls[0][1] as Uint8Array)).toBe('{}')
    expect(calls[0][2]).toEqual({ headers: { 'x-cc-desk-version-manager': proof } })
    for (const command of ['cli_start', 'spawn_new_instance', 'plugin:shell|execute', 'prepare_history', 'start_manager']) {
      await expect(context.__CC_DESK_VERSION_MANAGER__.invoke(command, {})).rejects.toMatchObject({ code: 'FORBIDDEN' })
    }
    expect(calls).toHaveLength(1)
    expect(context.__CC_DESK_DOCUMENT__).toBeUndefined()
  })
  it('ManagerBridge_ExactDocument_002', () => {
    for (const url of ['https://example.invalid/version-manager.html', 'http://tauri.localhost/index.html', `${expectedUrl}?transactionId=anything`]) {
      expect(realm(url).context.__CC_DESK_VERSION_MANAGER__).toBeUndefined()
    }
    expect(realm(expectedUrl, true).context.__CC_DESK_VERSION_MANAGER__).toBeUndefined()
  })
  it('ManagerBridge_NoReplacementNoProofProjection_003', () => {
    const { context, script } = realm()
    const original = context.__CC_DESK_VERSION_MANAGER__
    runInNewContext(script, context)
    expect(context.__CC_DESK_VERSION_MANAGER__).toBe(original)
    expect(Object.isFrozen(original)).toBe(true)
    expect(Object.keys(original)).toEqual(['invoke'])
    expect(JSON.stringify(original)).not.toContain(proof)
    expect(Object.getOwnPropertyDescriptor(context, '__CC_DESK_VERSION_MANAGER__')?.configurable).toBe(false)
  })
  it('ManagerBridge_BoundsAndNoReplay_004', async () => {
    const { context, calls } = realm()
    await expect(context.__CC_DESK_VERSION_MANAGER__.invoke('restore_previous_version', { value: 'x'.repeat(1024) })).rejects.toMatchObject({ code: 'INVALID_REQUEST' })
    const cyclic: Record<string, unknown> = {}
    cyclic.self = cyclic
    await expect(context.__CC_DESK_VERSION_MANAGER__.invoke('restore_previous_version', cyclic)).rejects.toMatchObject({ code: 'INVALID_REQUEST' })
    expect(calls).toHaveLength(0)
    let attempts = 0
    context.__TAURI_INTERNALS__.invoke = async () => { attempts++; throw { code: 'TRANSPORT_LOST' } }
    await expect(context.__CC_DESK_VERSION_MANAGER__.invoke('restore_previous_version', { expectedGeneration: '2' })).rejects.toMatchObject({ code: 'TRANSPORT_LOST' })
    expect(attempts).toBe(1)
  })
})
