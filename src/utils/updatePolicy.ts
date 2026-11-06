export type ReleaseChannel = 'stable' | 'test' | 'preview'
export type PublishedReleaseKind = 'stable' | 'beta' | 'rc' | 'preview'
export type UpdateChannel = PublishedReleaseKind | 'candidate' | 'test-only' | 'unverified'

export function isReleaseChannel(value: unknown): value is ReleaseChannel {
  return value === 'stable' || value === 'test' || value === 'preview'
}

function releaseKind(version: unknown): PublishedReleaseKind | null {
  if (typeof version !== 'string' || version.length > 64) return null
  const numeric = '(?:0|[1-9]\\d*)'
  const match = version.match(new RegExp(`^${numeric}\\.${numeric}\\.${numeric}(?:-(beta|rc|preview)(?:\\.${numeric})?)?$`))
  if (!match) return null
  return match[1] === 'beta' || match[1] === 'rc' || match[1] === 'preview' ? match[1] : 'stable'
}
/** Known artifact metadata may prove exclusion only. It never proves promotion. */
export function excludedArtifactChannel(metadata: unknown): UpdateChannel {
  if (!metadata || typeof metadata !== 'object' || Array.isArray(metadata)) return 'unverified'
  const own = (key: string) => Object.prototype.hasOwnProperty.call(metadata, key) ? (metadata as Record<string, unknown>)[key] : undefined
  if (own('product') !== 'CC Desk' || own('publishable') !== false || own('updaterPublication') !== false) return 'unverified'
  return own('channel') === 'test-only' ? 'test-only' : own('channel') === 'candidate' ? 'candidate' : 'unverified'
}
/** A display receipt from the backend; installation rechecks its retained admission. */
export function isOrdinaryUpdateEligible(info: unknown): boolean {
  if (!info || typeof info !== 'object') return false
  const value = info as Record<string, unknown>
  const release = value.officialRelease as Record<string, unknown> | undefined
  const selected = value.selectedChannel ?? 'stable', kind = releaseKind(value.version)
  const allowed = isReleaseChannel(selected) && kind !== null && value.channel === kind
    && (kind === 'stable' || selected === 'preview' || selected === 'test' && kind !== 'preview')
  return value.hasUpdate === true && allowed && value.installEligible === true
    && value.publishable !== false && value.updaterPublication !== false
    && typeof value.admissionId === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(value.admissionId)
    && !!release && typeof release.id === 'number' && Number.isSafeInteger(release.id) && release.id > 0
    && release.tag === `v${value.version}` && typeof release.sourceSha === 'string' && /^[0-9a-f]{40}$/.test(release.sourceSha)
}
