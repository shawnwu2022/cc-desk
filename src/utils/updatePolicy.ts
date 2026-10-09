export type UpdateChannel = 'stable' | 'candidate' | 'test-only' | 'unverified'
/** Known artifact metadata may prove exclusion only. It never proves promotion. */
export function excludedArtifactChannel(metadata: unknown): UpdateChannel {
  if (!metadata || typeof metadata !== 'object' || Array.isArray(metadata)) return 'unverified'
  const own = (key: string) => Object.prototype.hasOwnProperty.call(metadata, key) ? (metadata as Record<string, unknown>)[key] : undefined
  if (own('product') !== 'CC Desk' || own('publishable') !== false || own('updaterPublication') !== false) return 'unverified'
  return own('channel') === 'test-only' ? 'test-only' : own('channel') === 'candidate' ? 'candidate' : 'unverified'
}
/** Signed candidates only; there is currently no authorized promotion/provenance contract. */
export function isOrdinaryUpdateEligible(_info: unknown): boolean { return false }
