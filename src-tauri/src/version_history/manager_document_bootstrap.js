(() => {
  'use strict'
  if (window.top !== window || Object.prototype.hasOwnProperty.call(window, '__CC_DESK_VERSION_MANAGER__')) return
  const url = new URL(window.location.href)
  url.hash = ''
  if (url.href !== __MANAGER_URL__) return
  const proof = __MANAGER_PROOF__
  const encoder = new TextEncoder()
  const allowed = new Set(['inspect_version_switch', 'confirm_historical_version', 'restore_previous_version'])
  const bridge = Object.freeze({
    async invoke(command, payload) {
      if (!allowed.has(command)) throw { code: 'FORBIDDEN' }
      let body
      try {
        const json = JSON.stringify(payload)
        if (typeof json !== 'string') throw new Error()
        body = encoder.encode(json)
        if (body.length > 1024) throw new Error()
      } catch {
        throw { code: 'INVALID_REQUEST' }
      }
      const internals = window.__TAURI_INTERNALS__
      if (!internals || typeof internals.invoke !== 'function') throw { code: 'DOCUMENT_BRIDGE_UNAVAILABLE' }
      return internals.invoke(command, body, { headers: { 'x-cc-desk-version-manager': proof } })
    }
  })
  Object.defineProperty(window, '__CC_DESK_VERSION_MANAGER__', {
    value: bridge, enumerable: false, configurable: false, writable: false
  })
})()
