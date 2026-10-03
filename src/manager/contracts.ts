import type { ManagerAction, ManagerBlockReason, ManagerPhase, ManagerStatus } from '../types/versionManager'

const phases: readonly ManagerPhase[] = ['preparing', 'installing', 'installed-unconfirmed', 'historical-active',
  'returning', 'restored', 'pre-context-aborted', 'recovery-required']
const actions: readonly ManagerAction[] = ['refresh', 'confirm-historical-version', 'return-to-previous']
const reasons: readonly ManagerBlockReason[] = ['SOURCE_STILL_RUNNING', 'SOURCE_EXIT_UNCONFIRMED', 'SESSIONS_NOT_QUIESCENT',
  'INSTALLER_OUTCOME_UNKNOWN', 'PAYLOAD_UNVERIFIED', 'PAYLOAD_CHANGED', 'RETURN_CONFLICT',
  'RECOVERY_EVIDENCE_UNAVAILABLE', 'STORAGE_UNAVAILABLE', 'DOCUMENT_CHANGED', 'MANAGER_HANDOFF_INTERRUPTED']
function invalid(): never { throw new Error('MANAGER_INVALID_RESPONSE') }
function version(value: unknown): string {
  if (typeof value !== 'string' || value.length > 32 || !/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(value)
    || value.split('.').some(part => BigInt(part) > BigInt('4294967295'))) invalid()
  return value
}
export function parseManagerStatus(value: unknown): ManagerStatus {
  if (!value || typeof value !== 'object' || Array.isArray(value)) invalid()
  const row = value as Record<string, unknown>
  const fields = ['transactionId', 'generation', 'sourceVersion', 'targetVersion', 'phase', 'blockedReason', 'allowedActions']
  if (Object.keys(row).length !== fields.length || fields.some(field => !Object.prototype.hasOwnProperty.call(row, field))) invalid()
  if (typeof row.transactionId !== 'string'
    || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(row.transactionId)
    || row.transactionId === '00000000-0000-0000-0000-000000000000') invalid()
  if (typeof row.generation !== 'string' || !/^(0|[1-9][0-9]{0,19})$/.test(row.generation)
    || BigInt(row.generation) > BigInt('18446744073709551615')) invalid()
  if (!phases.includes(row.phase as ManagerPhase) || row.blockedReason !== null && !reasons.includes(row.blockedReason as ManagerBlockReason)) invalid()
  if (!Array.isArray(row.allowedActions) || row.allowedActions.length < 1 || row.allowedActions.length > 3
    || row.allowedActions[0] !== 'refresh' || new Set(row.allowedActions).size !== row.allowedActions.length
    || row.allowedActions.some(action => !actions.includes(action))
    || row.allowedActions.includes('confirm-historical-version') && row.phase !== 'installed-unconfirmed'
    || row.allowedActions.includes('return-to-previous') && !['installed-unconfirmed', 'historical-active', 'recovery-required'].includes(row.phase as string)) invalid()
  return Object.freeze({
    transactionId: row.transactionId, generation: row.generation, sourceVersion: version(row.sourceVersion), targetVersion: version(row.targetVersion),
    phase: row.phase as ManagerPhase, blockedReason: row.blockedReason as ManagerBlockReason | null,
    allowedActions: Object.freeze([...row.allowedActions]) as readonly ManagerAction[],
  })
}
