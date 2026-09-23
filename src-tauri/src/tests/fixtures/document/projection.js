async function runProjection(targets) {
  const n = window.__TAURI_INTERNALS__, bridge = window.__CC_DESK_DOCUMENT__
  const bytes = x => new TextEncoder().encode(JSON.stringify(x))
  const record = name => n.invoke('d12_record', { name })
  const expectCode = async (p, expected) => { const code = await p.then(() => null, e => e.code); if (code !== expected) throw Error('unexpected rejection') }
  try {
    const a = await bridge.invoke('native_get_scope', targets[0]), b = await bridge.invoke('native_get_scope', targets[1])
    const query = source => ({ source, resourceKind: 'history', requestEpoch: '1' })
    const left = await bridge.invoke('native_list_resources', query(a)), right = await bridge.invoke('native_list_resources', query(b))
    if (left.items[0].title !== 'a' || right.items[0].title !== 'b' || a.sourceRootKey === b.sourceRootKey) throw Error('root isolation')
    await record('independent-roots')
    const proof = await bridge.invoke('d12_save', a), options = { headers: { 'x-cc-desk-document': proof } }
    await expectCode(n.invoke('native_get_scope', bytes(targets[0])), 'FORBIDDEN'); await record('missing-proof')
    await expectCode(n.invoke('native_get_scope', targets[0], options), 'RAW_BODY_REQUIRED'); await record('raw-required')
    await expectCode(bridge.invoke('native_get_scope', { ...targets[0], root: '/outside' }), 'INVALID_REQUEST'); await record('forged-path')
    await expectCode(bridge.invoke('native_list_resources', query({ ...a, sourceRootKey: 'forged' })), 'SCOPE_STALE'); await record('forged-reference')
    await expectCode(n.invoke('native_get_scope', new Uint8Array(4097), options), 'REQUEST_TOO_LARGE'); await record('body-budget')
    await n.invoke('d12_change_profile'); await expectCode(bridge.invoke('native_list_resources', query(a)), 'SCOPE_REVOKED'); await record('profile-revoked')
    await n.invoke('d12_peer')
  } catch { await n.invoke('d12_abort') }
}
