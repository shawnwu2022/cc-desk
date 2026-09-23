async function runChannelProbe(request) {
  const native = window.__TAURI_INTERNALS__
  const abort = () => native.invoke('d11_channel_abort')
  const received = []
  let acknowledged = false
  const id = native.transformCallback(packet => {
    if (packet.end) {
      if (received.length !== 4) abort()
      native.unregisterCallback(id)
      return
    }
    received.push(packet)
    if (received.length === 4 && !acknowledged) {
      acknowledged = true
      native.invoke('d11_channel_ack', { events: received }).catch(abort)
    } else if (received.length > 4) abort()
  }, false)
  const channel = { toJSON: () => `__CHANNEL__:${id}` }
  try {
    await window.__CC_DESK_DOCUMENT__.invoke('d11_channel_open', request, channel)
    await window.__CC_DESK_DOCUMENT__.invoke('d11_channel_duplicate', {}, channel)
  } catch {
    await abort()
  }
}
