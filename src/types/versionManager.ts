/** Presentation only. The authenticated backend owns all transaction effects. */
export type ManagerPhase = 'preparing' | 'installing' | 'installed-unconfirmed' | 'historical-active'
  | 'returning' | 'restored' | 'pre-context-aborted' | 'recovery-required'
export type ManagerAction = 'refresh' | 'confirm-historical-version' | 'return-to-previous'
export type ManagerMutation = Exclude<ManagerAction, 'refresh'>
export type ManagerBlockReason = 'SOURCE_STILL_RUNNING' | 'SOURCE_EXIT_UNCONFIRMED' | 'SESSIONS_NOT_QUIESCENT'
  | 'INSTALLER_OUTCOME_UNKNOWN' | 'PAYLOAD_UNVERIFIED' | 'PAYLOAD_CHANGED' | 'RETURN_CONFLICT'
  | 'RECOVERY_EVIDENCE_UNAVAILABLE' | 'STORAGE_UNAVAILABLE' | 'DOCUMENT_CHANGED' | 'MANAGER_HANDOFF_INTERRUPTED'
export interface ManagerStatus {
  readonly transactionId: string
  readonly generation: string
  readonly sourceVersion: string
  readonly targetVersion: string
  readonly phase: ManagerPhase
  readonly blockedReason: ManagerBlockReason | null
  readonly allowedActions: readonly ManagerAction[]
}
