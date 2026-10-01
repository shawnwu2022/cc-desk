import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { TextEncoder } from 'node:util'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { createWorkspaceSourceWarnings } from '@/utils/workspaceSourceWarnings'

const template = readFileSync('src-tauri/src/cli/document_bootstrap.js', 'utf8')
const target = { kind: 'profile', profileId: 'codex', expectedProfileRevision: '1', projectId: 'project' }
const source = { scopeId: 'scope-1', instanceId: 'instance', cli: 'codex', sourceRootKey: 'local:windows:1:2:3', identityEpoch: '1', profileId: 'codex', profileRevision: '1', target, basis: 'configured-profile' }
const context = { cli: 'codex' as const, profileId: 'codex', profileRevision: '1', projectId: 'project', projectPath: 'C:\\work\\private-project' }
function bridge(invoke: (command: string, body: Uint8Array, options: { headers: Record<string, string> }) => Promise<unknown>) {
  const realm: Record<string, any> = { URL, TextEncoder, location: { href: 'http://tauri.localhost/' }, __TAURI_INTERNALS__: { invoke } }
  realm.window = realm; realm.top = realm
  runInNewContext(template.replace('__CC_DESK_DOCUMENT_URL__', JSON.stringify(realm.location.href))
    .replace('__CC_DESK_DOCUMENT_PROOF__', JSON.stringify('1234567890abcdef1234567890abcdef'))
    .replace('__CC_DESK_DOCUMENT_INSTANCE__', JSON.stringify('instance')), realm)
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: realm.__CC_DESK_DOCUMENT__ })
}
beforeEach(() => setActivePinia(createPinia()))
afterEach(() => { Reflect.deleteProperty(window, '__CC_DESK_DOCUMENT__') })

describe('Workspace diagnostics through the real document and projection clients', () => {
  it('keeps the Rust safe-error envelope code after authenticated raw transport rejects a scope', async () => {
    const calls: string[] = []
    bridge(async (command, bytes, options) => {
      calls.push(command)
      expect(JSON.parse(new TextDecoder().decode(bytes))).toEqual(target)
      expect(options.headers['x-cc-desk-document']).toHaveLength(32)
      throw { code: 'FORBIDDEN', field: 'private-field', retryable: false, message: 'do-not-display' }
    })
    const history = useNativeHistoryStore()
    const warnings = createWorkspaceSourceWarnings()
    await history.load(context).catch(failure => warnings.add('codex-history', failure))
    expect(history.get(context)?.error).toBe('FORBIDDEN')
    expect(warnings.items).toEqual([{ source: 'codex-history', code: 'FORBIDDEN' }])
    expect(calls).toEqual(['native_get_scope'])
  })

  it('keeps an authenticated unavailable response distinct from a malformed wire response', async () => {
    let malformed = false
    bridge(async (command, bytes) => {
      const request = JSON.parse(new TextDecoder().decode(bytes))
      if (command === 'native_get_scope') return source
      return { source, resourceKind: 'history', requestEpoch: request.requestEpoch, observedAt: '1', state: 'unavailable',
        reason: malformed ? 'FORBIDDEN private-path' : 'SOURCE_TOO_LARGE', items: [], hasMore: false }
    })
    const history = useNativeHistoryStore()
    const warnings = createWorkspaceSourceWarnings()
    const entry = await history.load(context)
    warnings.add('codex-history', { code: entry.error })
    expect(warnings.items).toEqual([{ source: 'codex-history', code: 'SOURCE_TOO_LARGE' }])
    malformed = true
    await history.load({ ...context, force: true }).catch(failure => warnings.add('codex-history', failure))
    expect(warnings.items[1]).toEqual({ source: 'codex-history', code: 'INVALID_PROJECTION' })
    expect(JSON.stringify(warnings.items)).not.toContain('private')
  })

  it('deduplicates bounded code and source pairs and never retains arbitrary failures', () => {
    const warnings = createWorkspaceSourceWarnings()
    for (let i = 0; i < 100; ++i) warnings.add('codex-history', { code: 'FORBIDDEN', field: '/secret', message: 'raw payload' })
    expect(warnings.items).toEqual([{ source: 'codex-history', code: 'FORBIDDEN' }])
    for (const failure of [new Error('/private/token'), { code: 'SOURCE_TOO_LARGE /private' }, 'FORBIDDEN', { message: 'private' }]) warnings.add('codex-history', failure)
    expect(warnings.items[1]).toEqual({ source: 'codex-history', code: 'SOURCE_UNAVAILABLE' })
    for (const code of ['SCOPE_UNKNOWN', 'SCOPE_STALE', 'SCOPE_REVOKED', 'SCOPE_CAPACITY', 'SCOPE_EPOCH_EXHAUSTED', 'SCOPE_UNAVAILABLE', 'SOURCE_UNSUPPORTED', 'SOURCE_INVALID', 'SOURCE_INVALID_TEXT', 'SOURCE_PATH_REJECTED', 'SOURCE_CHANGED', 'SOURCE_NOT_REGULAR']) warnings.add('codex-history', { code })
    expect(warnings.items).toHaveLength(12)
    expect(warnings.truncated).toBe(true)
    expect(JSON.stringify(warnings)).not.toMatch(/private|secret|payload/)
  })
})
