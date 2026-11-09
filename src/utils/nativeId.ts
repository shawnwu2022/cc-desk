export function createNativeId(prefix: string): string {
  if (!/^[A-Za-z0-9_-]{1,32}$/.test(prefix)) {
    throw new Error('INVALID_ID_PREFIX')
  }
  const cryptoApi = globalThis.crypto
  if (!cryptoApi || typeof cryptoApi.getRandomValues !== 'function') {
    throw new Error('SECURE_RANDOM_UNAVAILABLE')
  }
  const bytes = new Uint8Array(16)
  cryptoApi.getRandomValues(bytes)
  const suffix = Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')
  return `${prefix}-${suffix}`
}

/** 与已有原生 ID 共用安全随机源，兼容仅提供 getRandomValues 的 WebView。 */
export function createNativeUuid(): string {
  const hex = createNativeId('uuid').slice(5)
  const variant = ((Number.parseInt(hex[16]!, 16) & 3) | 8).toString(16)
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-4${hex.slice(13, 16)}-${variant}${hex.slice(17, 20)}-${hex.slice(20)}`
}
