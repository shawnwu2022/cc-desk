(() => {
  'use strict'
  // Static initialization scripts also run on navigation and, on Windows,
  // subframes. Only the original top-level document gets this bridge; the
  // backend permanently revokes the proof on any subsequent document load.
  if (window.top !== window || Object.prototype.hasOwnProperty.call(window, '__CC_DESK_DOCUMENT__')) return
  const actualUrl = new URL(window.location.href)
  actualUrl.hash = ''
  if (actualUrl.href !== __CC_DESK_DOCUMENT_URL__) return
  const proof = __CC_DESK_DOCUMENT_PROOF__
  const encoder = new TextEncoder()
  const bridge = Object.freeze({
    async invoke(command, payload) {
      let body
      try {
        const json = JSON.stringify(payload)
        if (typeof json !== 'string') throw new Error()
        body = encoder.encode(json)
      } catch {
        throw { code: 'INVALID_REQUEST' }
      }
      const internals = window.__TAURI_INTERNALS__
      if (!internals || typeof internals.invoke !== 'function') {
        throw { code: 'DOCUMENT_BRIDGE_UNAVAILABLE' }
      }
      // Preserve the original request and error; never retry or fall back to
      // an invoke without document admission. No caller-supplied options.
      return internals.invoke(command, body, {
        headers: { 'x-cc-desk-document': proof }
      })
    }
  })
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', {
    value: bridge,
    enumerable: false,
    configurable: false,
    writable: false
  })
})()
