/** Backend observation identities and selection tokens carry no executable authority. */
export type HistoryPlatform = 'windows-x86_64' | 'darwin-aarch64' | 'linux-x86_64' | 'unsupported'
export type HistoryBlockReason =
  | 'PLATFORM_UNSUPPORTED' | 'PLATFORM_ASSET_MISSING' | 'PACKAGE_FORMAT_UNSUPPORTED'
  | 'PACKAGING_BOUNDARY_UNKNOWN' | 'SIGNATURE_MISSING' | 'DIGEST_UNAVAILABLE'
  | 'ASSET_AMBIGUOUS' | 'ASSET_METADATA_INVALID' | 'RELEASE_NOT_HISTORICAL'
export interface HistoryRelease {
  releaseId: string
  assetId: string | null
  version: string
  publishedAt: string
  platform: HistoryPlatform
  availablePlatforms: Exclude<HistoryPlatform, 'unsupported'>[]
  packageFormat: 'nsis' | null
  verification: 'awaiting-verification'
  selectAllowed: boolean
  installReady: false
  dataModes: { freshSettings: 'available' | 'unavailable'; keepCurrentData: 'unavailable' }
  blockedReason: HistoryBlockReason | null
}
export interface HistoryCatalogPage {
  rows: HistoryRelease[]
  nextCursor: string | null
  truncated: boolean
}
export interface HistorySelection {
  selectionToken: string
  releaseId: string
  assetId: string
  version: string
  expiresAt: string
  verification: 'awaiting-verification'
  installReady: false
}
export interface ListHistoryRequest { cursor: string | null }
export interface SelectHistoryRequest { releaseId: string; assetId: string }
