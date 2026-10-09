async function runD11(request) {
  'use strict'
  const invoke = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__)
  const encode = (value) => new TextEncoder().encode(JSON.stringify(value))
  try {
    // This test-only native handler returns the validated proof so later cases
    // can intentionally reuse it. Never replace or mock Tauri's frozen invoke.
    const proof = await window.__CC_DESK_DOCUMENT__.invoke('d11_probe', request)
    if (typeof proof !== 'string' || !/^[0-9a-f]{32}$/.test(proof)) throw new Error()
    const call = (name, body, token = proof) => invoke('d11_probe', body, {
      headers: {
        'x-cc-desk-test-case': name,
        ...(token === null ? {} : { 'x-cc-desk-document': token })
      }
    })
    const query = { requestId: 'probe-request' }
    await call('query', encode(query))
    await call('missing-proof', new TextEncoder().encode('not-json'), null)
    await call('wrong-proof', encode(request), '0'.repeat(32))
    await call('combined-proof', encode(request), proof + ', ' + proof)
    await call('forged-owner', encode({ ...request, ownerWindowId: 'main' }))
    await call('json', request)
    const boundary = new Uint8Array(1024).fill(32)
    boundary.set(encode(query))
    await call('query-boundary', boundary)
    await call('query-overflow', new Uint8Array(1025).fill(32))
    await invoke('d11_peer')
  } catch {
    await invoke('d11_abort')
  }
}
