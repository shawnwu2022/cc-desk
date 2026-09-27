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
