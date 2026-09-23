async function runD11(request) {
  const internals = window.__TAURI_INTERNALS__
  const original = internals.invoke.bind(internals)
  const encode = (value) => new TextEncoder().encode(JSON.stringify(value))
  let proof
  // Observe, do not mock, the bridge's outgoing headers. The native transport
  // still executes every call. Intentional disclosure to the peer is adversarial.
  internals.invoke = (command, body, options) => {
    proof = options.headers['x-cc-desk-document']
    return original(command, body, {
      headers: { ...options.headers, 'x-cc-desk-test-case': 'start' }
    })
  }
  try {
    await window.__CC_DESK_DOCUMENT__.invoke('d11_probe', request)
    internals.invoke = original
    const call = (name, body, token = proof) => original('d11_probe', body, {
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
    await original('d11_peer')
  } catch {
    internals.invoke = original
    await original('d11_abort')
  }
}
