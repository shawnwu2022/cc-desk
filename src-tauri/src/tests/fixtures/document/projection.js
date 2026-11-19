async function runProjection(targets) {
  const n = window.__TAURI_INTERNALS__, bridge = window.__CC_DESK_DOCUMENT__
  const bytes = x => new TextEncoder().encode(JSON.stringify(x))
  const record = name => n.invoke('d12_record', { name })
  const expectCode = async (p, expected) => { const code = await p.then(() => null, e => e.code); if (code !== expected) throw Error('unexpected rejection') }
  const expectDiagnostic = async (p, code, stage) => {
    const value = await p.then(() => null, e => e)
    if (!value || value.code !== code || value.stage !== stage || Object.keys(value).sort().join(',') !== 'code,retryable,stage') throw Error('unexpected diagnostic')
  }
  try {
    const a = await bridge.invoke('native_get_scope', targets[0]), b = await bridge.invoke('native_get_scope', targets[1])
    const query = source => ({ source, resourceKind: 'history', requestEpoch: '1' })
    const left = await bridge.invoke('native_list_resources', query(a)), right = await bridge.invoke('native_list_resources', query(b))
    if (left.items[0].title !== 'a' || right.items[0].title !== 'b' || a.sourceRootKey === b.sourceRootKey) throw Error('root isolation')
    await record('independent-roots')
    const proof = await bridge.invoke('d12_save', a), options = { headers: { 'x-cc-desk-document': proof } }
    const control = payload => bridge.invoke('cli_diagnostic_capture', payload)
    const inspect = {operationId: 'bafa4942-2000-4444-8000-000000000001'}
    const readError = promise => promise.then(() => null, error => JSON.stringify(error))
    const baseline = await readError(bridge.invoke('cli_inspect_session_delete', inspect))
    if (baseline === null) throw Error('read-only inspection must fail without existing operation')
    const opened = await control({action:'start', serial:1})
    const id = 'a'.repeat(32), header = opened.grant + ':' + id
    const wire = (...values) => n.invoke('cli_inspect_session_delete', bytes(inspect), {headers:[
      ['x-cc-desk-document', proof], ...values.map(value => ['x-cc-desk-diag-trace', value])
    ]})
    if (await readError(wire('malformed')) !== baseline || await readError(wire(header, header)) !== baseline) throw Error('diagnostic changed result')
    if ((await control({action:'snapshot'})).report.events.length !== 0) throw Error('malformed or repeated diagnostic accepted')
    if (await readError(bridge.invoke('cli_inspect_session_delete', inspect, undefined, header)) !== baseline) throw Error('diagnostic changed result')
    const captured = await control({action:'snapshot'})
    if (captured.report.events.length !== 2 || captured.report.events.some(event => event.id !== id || event.stage !== 'backend')) throw Error('diagnostic receipt missing')
    if (JSON.stringify(captured).includes(opened.grant)) throw Error('grant leaked')
    if ((await control({action:'stop', serial:2})).state !== 'stopped') throw Error('stop not acknowledged')
    if (await readError(wire(header)) !== baseline || (await control({action:'snapshot'})).report.events.length !== 2) throw Error('closed grant accepted')
    const reopened = await control({action:'start', serial:3})
    if (reopened.grant === opened.grant || await readError(wire(header)) !== baseline ||
        (await control({action:'snapshot'})).report.events.length !== 0) throw Error('previous grant replay accepted')
    await control({action:'stop', serial:4})
    await record('diagnostic-header-no-authority')
    await expectDiagnostic(n.invoke('native_get_scope', bytes(targets[0])), 'FORBIDDEN', 'scope-document-admission'); await record('missing-proof')
    await expectDiagnostic(n.invoke('native_get_scope', targets[0], options), 'RAW_BODY_REQUIRED', 'scope-request-decode'); await record('raw-required')
    await expectDiagnostic(bridge.invoke('native_get_scope', { ...targets[0], root: '/outside' }), 'INVALID_REQUEST', 'scope-request-decode'); await record('forged-path')
    await expectDiagnostic(bridge.invoke('native_list_resources', query({ ...a, sourceRootKey: 'forged' })), 'SCOPE_STALE', 'read-capability'); await record('forged-reference')
    await expectCode(n.invoke('native_get_scope', new Uint8Array(4097), options), 'REQUEST_TOO_LARGE'); await record('body-budget')
    await n.invoke('d12_change_profile'); await expectCode(bridge.invoke('native_list_resources', query(a)), 'SCOPE_REVOKED'); await record('profile-revoked')
    await n.invoke('d12_peer')
  } catch { await n.invoke('d12_abort') }
}
