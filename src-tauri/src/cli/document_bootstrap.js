(() => {
  'use strict'
  // A new native document needs new authority; this bridge cannot reauthorize it.
  if (window.top !== window || Object.prototype.hasOwnProperty.call(window, '__CC_DESK_DOCUMENT__')) return
  const actualUrl = new URL(window.location.href)
  actualUrl.hash = ''
  if (actualUrl.href !== __CC_DESK_DOCUMENT_URL__) return
  const proof = __CC_DESK_DOCUMENT_PROOF__
  const encoder = new TextEncoder()
  const bridge = {
    async invoke(command, payload, channel) {
      let body
      const headers = { 'x-cc-desk-document': proof }
      try {
        const json = JSON.stringify(payload)
        if (typeof json !== 'string') throw new Error()
        body = encoder.encode(json)
        if (channel !== undefined) {
          const descriptor = channel.toJSON()
          if (typeof descriptor !== 'string' ||
              !/^__CHANNEL__:(0|[1-9][0-9]{0,9})$/.test(descriptor) ||
              Number(descriptor.slice(12)) > 4294967295) throw new Error()
          headers['x-cc-desk-output-channel'] = descriptor
        }
      } catch {
        throw { code: 'INVALID_REQUEST' }
      }
      const internals = window.__TAURI_INTERNALS__
      if (!internals || typeof internals.invoke !== 'function') {
        throw { code: 'DOCUMENT_BRIDGE_UNAVAILABLE' }
      }
      // No retry/fallback. Channel metadata does not alter the frozen request.
      return internals.invoke(command, body, { headers })
    }
  }
  Object.defineProperty(bridge, 'instanceId', { value: __CC_DESK_DOCUMENT_INSTANCE__ })
  Object.freeze(bridge)
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', {
    value: bridge,
    enumerable: false,
    configurable: false,
    writable: false
  })
})()
