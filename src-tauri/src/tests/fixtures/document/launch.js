async function runLaunchProbe(request, mode) {
  const native = window.__TAURI_INTERNALS__
  const bridge = window.__CC_DESK_DOCUMENT__
  let stage = 'initial'
  const fail = () => native.invoke('d11_launch_abort', { stage })
  const received = []
  const id = native.transformCallback(packet => {
    if (packet.end) { native.unregisterCallback(id); return }
    received.push(packet.message)
    if (received.length > 1024) fail()
  }, false)
  const channel = { toJSON: () => `__CHANNEL__:${id}` }
  try {
    if (mode === 'closed') {
      stage = 'gate'
      const code = await bridge.invoke('cli_start', request, channel).then(() => 'ACCEPTED', e => e.code)
      if (code !== 'NATIVE_RUNTIME_NOT_READY') throw new Error('gate')
      const query = await bridge.invoke('cli_get_launch_status', {requestId: request.requestId}).then(() => 'FOUND', e => e.code)
      if (query !== 'LAUNCH_NOT_FOUND') throw new Error('gate query')
      const cancelled = await bridge.invoke('cli_cancel_launch', request)
      const replay = await bridge.invoke('cli_start', request, channel)
      if (cancelled.phase !== 'cancelled' || JSON.stringify(replay) !== JSON.stringify(cancelled)) throw new Error('gate cancellation')
      await bridge.invoke('d11_launch_closed', cancelled)
      return
    }
    stage = 'cancel-before-start'
    const cancelledRequest = { ...request, requestId: `${request.requestId}-cancelled`, tabId: `${request.tabId}-cancelled`, runId: `${request.runId}-cancelled` }
    const cancelled = await bridge.invoke('cli_cancel_launch', cancelledRequest)
    const fenced = await bridge.invoke('cli_start', cancelledRequest, channel)
    if (cancelled.phase !== 'cancelled' || JSON.stringify(fenced) !== JSON.stringify(cancelled)) throw new Error('cancellation fence')
    await bridge.invoke('d11_launch_cancelled', cancelled)
    stage = 'concurrent-start'
    // Intentionally ignore every start response, then recover using the original ID.
    await Promise.all(Array.from({length:100}, () => bridge.invoke('cli_start',request,channel)))
    stage = 'status'
    const receipt = await bridge.invoke('cli_get_launch_status', {requestId:request.requestId})
    if (receipt.instanceId !== bridge.instanceId) throw new Error('instance')
    await bridge.invoke('d11_launch_validate', receipt)
    stage = 'replay-after-delete'
    const nonce = '0123456789abcdef0123456789abcdef'
    let serial = 0; const control = action => bridge.invoke('cli_diagnostic_capture', action === 'snapshot' ? { action } : { action, serial: ++serial })
    const { grant } = await control('start')
    const replayStart = await bridge.invoke('cli_start', request, channel, grant + ':' + nonce)
    const replay = await bridge.invoke('cli_get_launch_status', {requestId:request.requestId})
    const { report } = await control('snapshot')
    if (replayStart.run.runId !== replay.run.runId
        || report.events.length !== 1
        || report.events[0].id !== nonce
        || report.events[0].stage !== 'backend' || report.events[0].status !== 'returned') {
      throw new Error('diagnostic replay')
    }
    await control('stop')
    await control('start')
    await bridge.invoke('cli_start', request, channel, grant + ':' + nonce)
    if ((await control('snapshot')).report.events.length !== 0) throw new Error('stale diagnostic grant')
    await control('stop')
    await bridge.invoke('d11_launch_replayed', replay)
    window.finishLaunchProbe = async () => {
      try {
        stage = 'stale-run'
        await bridge.invoke('d11_launch_stale', {runId:request.runId,generation:request.generation+1})
        stage = 'native-bytes'
        const deadline = Date.now()+15000
        for (;;) {
          const bytes = received.flatMap(e => e.bytes)
          if (new TextDecoder().decode(Uint8Array.from(bytes)).includes('D11_REAL_READY')) break
          if (Date.now()>deadline) throw new Error('bytes timeout')
          await new Promise(resolve => setTimeout(resolve,10))
        }
        await bridge.invoke('d11_launch_bytes', received)
      } catch { await fail() }
    }
    // Let Rust create the peer only after the continuation has been installed.
    stage = 'peer'
    await bridge.invoke('d11_launch_peer', {})
  } catch { await fail() }
}
