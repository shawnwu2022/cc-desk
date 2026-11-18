async function observeHostScope(request) {
  const bridge = window.__CC_DESK_DOCUMENT__
  const native = window.__TAURI_INTERNALS__
  let outcome = 'START_FAILED'
  const callback = native.transformCallback(() => {}, false)
  try {
    const channel = { toJSON: () => `__CHANNEL__:${callback}` }
    await bridge.invoke('cli_start', request, channel)
    try {
      const scope = await bridge.invoke('remote_host_pairing_scope', {runId:request.runId,generation:request.generation})
      outcome = scope && Object.keys(scope).sort().join(',') === 'generation,run,session'
        && scope.run === request.runId && scope.generation === String(request.generation)
        && typeof scope.session === 'string' && /^[0-9a-f]{32}$/.test(scope.session) ? 'OK' : 'SCOPE_SHAPE_INVALID'
    } catch (error) {
      const codes = ['REMOTE_HOST_UNAVAILABLE','NATIVE_CANONICAL_UNAVAILABLE','RUN_NOT_READY','FORBIDDEN','DOCUMENT_BRIDGE_UNAVAILABLE','INVALID_REQUEST']
      outcome = codes.includes(error?.code) ? error.code : 'UNKNOWN'
    }
  } finally {
    native.unregisterCallback(callback)
    await native.invoke('host_scope_observed', {outcome})
  }
}
