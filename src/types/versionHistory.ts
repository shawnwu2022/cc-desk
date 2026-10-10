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
/** Signature and digest verification alone never authorizes execution. */
export interface PreparationTicket { transactionId: string }
export interface PreparedPackageSummary {
  transactionId: string
  version: string
  verification: 'publisher-verified'
  installReady: false
  blockedReason: 'PACKAGE_IDENTITY_UNVERIFIED'
}
export interface CancelPrepareSummary { transactionId: string; cancelled: true }

export type SwitchReviewPhase = 'preparing' | 'verified' | 'handoff-issued' | 'cancelled' | 'unavailable' | 'aborted'
export type SwitchReviewAction = 'refresh' | 'review' | 'begin-switch' | 'cancel-preparation' | 'prepare-again'
export type SwitchReviewBlock = 'PREPARATION_PENDING' | 'PREPARATION_BUSY' | 'PREPARATION_EXPIRED' | 'PREPARATION_FAILED'
  | 'PAYLOAD_UNVERIFIED' | 'COORDINATOR_UNAVAILABLE' | 'HANDOFF_ISSUED'
export interface SwitchReview {
  preparationId: string
  version: string
  phase: SwitchReviewPhase
  contextPolicy: 'fresh-settings-preserve-current-shared-cli'
  transactionId: string | null
  allowedActions: SwitchReviewAction[]
  blockReason: SwitchReviewBlock | null
}
/** The manager owns this issued transaction; it is not installation success. */
export interface SwitchTicket { transactionId: string }

/** Ordinary installer handoff has a retained data backup, without an automatic return guarantee. */
export type OrdinaryInstallAction = 'refresh' | 'install' | 'cancel-preparation' | 'prepare-again'
export interface OrdinaryInstallReview {
  preparationId: string
  version: string
  phase: SwitchReviewPhase
  contextPolicy: 'fresh-settings-backup-manual-restore'
  transactionId: string | null
  allowedActions: OrdinaryInstallAction[]
  blockReason: Exclude<SwitchReviewBlock, 'PAYLOAD_UNVERIFIED'> | null
  backupLocation: string | null
  installationOutcome: 'not-started' | 'handoff-unknown' | 'installer-started'
}
