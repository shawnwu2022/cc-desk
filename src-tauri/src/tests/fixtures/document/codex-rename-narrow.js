// Verification-only Wry probe. The worker injects this after the real document
// bootstrap and registers the production commands; it does not create a bridge.
async function runCodexRenameProbe(target, fixture) {
  const bridge = window.__CC_DESK_DOCUMENT__
  const control = payload => bridge.invoke('cli_diagnostic_capture', payload)
  const report = (status, stage) => window.__TAURI_INTERNALS__.invoke('codex_rename_finish', { status, stage })
  const matches = (items, title) => {
    const matching = items.filter(item => item.type === 'session' && item.nativeSessionId === fixture.id)
    if (matching.length !== 1) throw Error()
    const row = matching[0]
    if (row.cwd !== fixture.cwd || row.title !== title || row.archived !== false || !row.sessionKey) throw Error()
    return row
  }
  const read = async epoch => {
    const source = await bridge.invoke('native_get_scope', target)
    const list = await bridge.invoke('native_list_resources', { source, resourceKind: 'history', requestEpoch: epoch })
    return { source, items: list.items }
  }
  let stage = 'initial-list'
  let opened = false
  let success = false
  try {
    const before = await read('1')
    const oldRow = matches(before.items, fixture.oldTitle)
    stage = 'capture-start'
    const capture = await control({ action: 'start', serial: 1 })
    opened = true
    if (typeof capture.grant !== 'string' || !capture.grant) throw Error()
    const randomId = () => Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, '0')).join('')
    const operationId = randomId()
    const trace = randomId()
    if (trace === operationId) throw Error()
    stage = 'rename'
    const receipt = await bridge.invoke('cli_rename_session', {
      operationId, source: before.source, sessionKey: oldRow.sessionKey, title: fixture.newTitle,
    }, undefined, capture.grant + ':' + trace)
    if (receipt.state !== 'renamed' || receipt.operationId !== operationId ||
        receipt.sessionKey !== oldRow.sessionKey || receipt.title !== fixture.newTitle) throw Error()
    stage = 'post-rename-list'
    const after = await read('2')
    matches(after.items, fixture.newTitle)
    stage = 'trace'
    const snapshot = await control({ action: 'snapshot' })
    const events = snapshot.report.events
    // Native events deliberately contain no operation payload. This exact trace
    // belongs to the only mutating call above.
    const stages = events.map(event => {
      if (event.id !== trace || Object.keys(event).sort().join(',') !== 'id,ms,stage,status') throw Error()
      return event.stage + ':' + event.status
    })
    if (snapshot.report.dropped !== 0 || stages.join(',') !== 'backend:started,provider:started,provider:ok,backend:returned') throw Error()
    stage = 'redaction'
    const rendered = JSON.stringify(snapshot)
    for (const value of [fixture.id, fixture.cwd, fixture.oldTitle, fixture.newTitle, operationId, capture.grant]) {
      if (!value || rendered.includes(value) || rendered.includes(JSON.stringify(value).slice(1, -1))) throw Error()
    }
    success = true
  } catch {
    // Do not disclose raw WebView errors, paths, request IDs, or fixture inputs.
  } finally {
    if (opened) {
      try {
        if ((await control({ action: 'stop', serial: 2 })).state !== 'stopped') throw Error()
      } catch {
        stage = 'capture-stop'
        success = false
      }
    }
  }
  await report(success ? 'passed' : 'failed', success ? 'complete' : stage)
}
