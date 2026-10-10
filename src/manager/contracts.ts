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
  const hasOrdinary = Object.prototype.hasOwnProperty.call(row, 'ordinaryInstall')
  if (hasOrdinary) fields.push('ordinaryInstall')
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
  let ordinaryInstall: ManagerStatus['ordinaryInstall']
  if (hasOrdinary) {
    const value = row.ordinaryInstall
    if (!value || typeof value !== 'object' || Array.isArray(value)) invalid()
    const ordinary = value as Record<string, unknown>
    const names = ['backupLocation', 'installerHandedOff', 'contextPolicy']
    if (Object.keys(ordinary).length !== names.length || names.some(name => !Object.prototype.hasOwnProperty.call(ordinary, name))
      || ordinary.contextPolicy !== 'fresh-settings-backup-manual-restore' || typeof ordinary.installerHandedOff !== 'boolean'
      || !['preparing', 'installing', 'recovery-required', 'pre-context-aborted'].includes(row.phase as string)
      || row.allowedActions.some(action => action !== 'refresh')) invalid()
    if (ordinary.backupLocation !== null && (typeof ordinary.backupLocation !== 'string'
      || ordinary.backupLocation.length > 32768 || !/^[A-Za-z]:\\/.test(ordinary.backupLocation)
      || /[\x00-\x1f\x7f]/.test(ordinary.backupLocation))) invalid()
    if (ordinary.installerHandedOff && (ordinary.backupLocation === null || !['installing', 'recovery-required'].includes(row.phase as string))) invalid()
    ordinaryInstall = Object.freeze({ backupLocation: ordinary.backupLocation as string | null,
      installerHandedOff: ordinary.installerHandedOff, contextPolicy: 'fresh-settings-backup-manual-restore' })
  }
  return Object.freeze({
    transactionId: row.transactionId, generation: row.generation, sourceVersion: version(row.sourceVersion), targetVersion: version(row.targetVersion),
    phase: row.phase as ManagerPhase, blockedReason: row.blockedReason as ManagerBlockReason | null,
    allowedActions: Object.freeze([...row.allowedActions]) as readonly ManagerAction[],
    ...(ordinaryInstall ? { ordinaryInstall } : {}),
  })
}
