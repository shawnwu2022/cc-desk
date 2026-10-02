import type { HistoryBlockReason, HistoryCatalogPage, HistoryPlatform, HistoryRelease, HistorySelection } from '@/types/versionHistory'

const reasons = new Set<HistoryBlockReason>([
  'PLATFORM_UNSUPPORTED', 'PLATFORM_ASSET_MISSING', 'PACKAGE_FORMAT_UNSUPPORTED', 'PACKAGING_BOUNDARY_UNKNOWN',
  'SIGNATURE_MISSING', 'DIGEST_UNAVAILABLE', 'ASSET_AMBIGUOUS', 'ASSET_METADATA_INVALID', 'RELEASE_NOT_HISTORICAL',
])
const platforms = ['windows-x86_64', 'darwin-aarch64', 'linux-x86_64', 'unsupported'] as const
function invalid(): never { throw new Error('HISTORY_INVALID_RESPONSE') }
function object(value: unknown, fields: string[]): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return invalid()
  const record = value as Record<string, unknown>
  if (Object.keys(record).length !== fields.length || fields.some(field => !Object.prototype.hasOwnProperty.call(record, field))) return invalid()
  return record
}
function token(value: unknown): string {
  return typeof value === 'string' && /^[a-f0-9]{32}$/.test(value) ? value : invalid()
}
function assetId(value: unknown): string {
  if (typeof value !== 'string' || !/^[1-9][0-9]{0,19}$/.test(value) || BigInt(value) > BigInt('18446744073709551615')) return invalid()
  return value
}
function version(value: unknown): string {
  if (typeof value !== 'string' || value.length > 32 || !/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(value)
    || value.split('.').some(part => BigInt(part) > BigInt('4294967295'))) return invalid()
  return value
}
function timestamp(value: unknown): string {
  if (typeof value !== 'string' || value.length > 40 || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/.test(value)
    || !Number.isFinite(Date.parse(value))) return invalid()
  return value
}
function bool(value: unknown): boolean { return typeof value === 'boolean' ? value : invalid() }
function pending(record: Record<string, unknown>) {
  if (record.verification !== 'awaiting-verification' || record.installReady !== false) return invalid()
}
function release(value: unknown): HistoryRelease {
  const row = object(value, ['releaseId', 'assetId', 'version', 'publishedAt', 'platform', 'availablePlatforms', 'packageFormat', 'verification', 'selectAllowed', 'installReady', 'dataModes', 'blockedReason'])
  pending(row)
  if (!platforms.includes(row.platform as HistoryPlatform) || (row.packageFormat !== null && row.packageFormat !== 'nsis')) return invalid()
  if (!Array.isArray(row.availablePlatforms) || row.availablePlatforms.length > 3
    || row.availablePlatforms.some(value => !platforms.slice(0, 3).includes(value))
    || new Set(row.availablePlatforms).size !== row.availablePlatforms.length) return invalid()
  const modes = object(row.dataModes, ['freshSettings', 'keepCurrentData'])
  if (modes.keepCurrentData !== 'unavailable' || !['available', 'unavailable'].includes(modes.freshSettings as string)) return invalid()
  const selectAllowed = bool(row.selectAllowed)
  if (row.blockedReason !== null && !reasons.has(row.blockedReason as HistoryBlockReason)) return invalid()
  if (selectAllowed ? row.blockedReason !== null || row.assetId === null || row.packageFormat !== 'nsis'
    || row.platform !== 'windows-x86_64' || !row.availablePlatforms.includes('windows-x86_64') || modes.freshSettings !== 'available'
    : row.blockedReason === null || modes.freshSettings !== 'unavailable') return invalid()
  return {
    releaseId: token(row.releaseId), assetId: row.assetId === null ? null : assetId(row.assetId), version: version(row.version),
    publishedAt: timestamp(row.publishedAt), platform: row.platform as HistoryPlatform,
    availablePlatforms: [...row.availablePlatforms] as HistoryRelease['availablePlatforms'], packageFormat: row.packageFormat,
    verification: 'awaiting-verification', selectAllowed, installReady: false,
    dataModes: { freshSettings: modes.freshSettings as 'available' | 'unavailable', keepCurrentData: 'unavailable' },
    blockedReason: row.blockedReason as HistoryBlockReason | null,
  }
}
export function parseHistoryCatalogPage(value: unknown): HistoryCatalogPage {
  const page = object(value, ['rows', 'nextCursor', 'truncated'])
  if (!Array.isArray(page.rows) || page.rows.length > 25) return invalid()
  const rows = page.rows.map(release)
  if (new Set(rows.map(row => row.releaseId)).size !== rows.length) return invalid()
  const truncated = bool(page.truncated)
  if (truncated && page.nextCursor !== null) return invalid()
  return { rows, nextCursor: page.nextCursor === null ? null : token(page.nextCursor), truncated }
}
export function parseHistorySelection(value: unknown): HistorySelection {
  const selected = object(value, ['selectionToken', 'releaseId', 'assetId', 'version', 'expiresAt', 'verification', 'installReady'])
  pending(selected)
  return { selectionToken: token(selected.selectionToken), releaseId: token(selected.releaseId), assetId: assetId(selected.assetId),
    version: version(selected.version), expiresAt: timestamp(selected.expiresAt), verification: 'awaiting-verification', installReady: false }
}

export function parsePreparationTicket(value: unknown): import('@/types/versionHistory').PreparationTicket {
  const ticket = object(value, ['transactionId'])
  return { transactionId: token(ticket.transactionId) }
}
export function parsePreparedPackageSummary(value: unknown): import('@/types/versionHistory').PreparedPackageSummary {
  const ready = object(value, ['transactionId', 'version', 'verification', 'installReady', 'blockedReason'])
  if (ready.verification !== 'publisher-verified' || ready.installReady !== false || ready.blockedReason !== 'PACKAGE_IDENTITY_UNVERIFIED') invalid()
  return { transactionId: token(ready.transactionId), version: version(ready.version), verification: 'publisher-verified', installReady: false, blockedReason: 'PACKAGE_IDENTITY_UNVERIFIED' }
}
export function parseCancelPrepareSummary(value: unknown): import('@/types/versionHistory').CancelPrepareSummary {
  const cancelled = object(value, ['transactionId', 'cancelled'])
  if (cancelled.cancelled !== true) invalid()
  return { transactionId: token(cancelled.transactionId), cancelled: true }
}
