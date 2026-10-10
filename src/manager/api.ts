import type { ManagerMutation, ManagerStatus } from '../types/versionManager'
import { parseManagerStatus } from './contracts'

export interface VersionManagerBridge { invoke(command: string, payload: unknown): Promise<unknown> }
declare global { interface Window { __CC_DESK_VERSION_MANAGER__?: VersionManagerBridge } }
export interface VersionManagerClient {
  isCurrent(): boolean
  inspect(): Promise<ManagerStatus>
  canAct(action: ManagerMutation, status: ManagerStatus): boolean
  act(action: ManagerMutation, status: ManagerStatus): Promise<ManagerStatus>
}
interface DocumentState {
  latest: ManagerStatus | null
  spentThrough: bigint
  pending: boolean
  revoked: boolean
  sequence: number
  published: number
}
// A Vue remount cannot forget an in-flight or uncertain mutation. No authority
// or recovery state is persisted to disk or transferred to another document.
const documents = new WeakMap<VersionManagerBridge, DocumentState>()
function failure(code: string): never { throw new Error(code) }
export function createVersionManagerClient(): VersionManagerClient | null {
  const bridge = window.__CC_DESK_VERSION_MANAGER__
  if (!bridge || typeof bridge.invoke !== 'function') return null
  const invoke = bridge.invoke
  let state = documents.get(bridge)
  if (!state) {
    state = { latest: null, spentThrough: BigInt(-1), pending: false, revoked: false, sequence: 0, published: 0 }
    documents.set(bridge, state)
  }
  const owned = state
  function isCurrent() {
    return !owned.revoked && window.__CC_DESK_VERSION_MANAGER__ === bridge && bridge!.invoke === invoke
  }
  async function request(command: string, payload: unknown) {
    if (!isCurrent()) failure('MANAGER_DOCUMENT_CHANGED')
    const sequence = ++owned.sequence
    let result: unknown
    try { result = await invoke.call(bridge, command, payload) } catch (error) {
      if (owned.latest && error && typeof error === 'object' && 'code' in error && error.code === 'FORBIDDEN') owned.revoked = true
      if (!isCurrent()) failure('MANAGER_DOCUMENT_CHANGED')
      throw error
    }
    if (!isCurrent()) failure('MANAGER_DOCUMENT_CHANGED')
    if (sequence < owned.published) failure('MANAGER_STALE_RESPONSE')
    const status = parseManagerStatus(result)
    const previous = owned.latest
    if (previous && (status.transactionId !== previous.transactionId || status.sourceVersion !== previous.sourceVersion
      || status.targetVersion !== previous.targetVersion
      || !!status.ordinaryInstall !== !!previous.ordinaryInstall
      || previous.ordinaryInstall?.backupLocation !== null && previous.ordinaryInstall?.backupLocation !== undefined
        && status.ordinaryInstall?.backupLocation !== previous.ordinaryInstall.backupLocation
      || previous.ordinaryInstall?.installerHandedOff && !status.ordinaryInstall?.installerHandedOff)) {
      owned.revoked = true
      failure('MANAGER_DOCUMENT_CHANGED')
    }
    if (previous && BigInt(status.generation) < BigInt(previous.generation)) failure('MANAGER_STALE_RESPONSE')
    owned.latest = status
    owned.published = sequence
    return status
  }
  function canAct(action: ManagerMutation, status: ManagerStatus) {
    return isCurrent() && !owned.pending && owned.latest === status && status.allowedActions.includes(action)
      && BigInt(status.generation) > owned.spentThrough
  }
  return {
    isCurrent,
    inspect: () => request('inspect_version_switch', {}),
    canAct,
    async act(action, status) {
      if (!canAct(action, status)) failure('MANAGER_ACTION_UNAVAILABLE')
      owned.pending = true
      owned.spentThrough = BigInt(status.generation)
      try {
        return await request(action === 'confirm-historical-version' ? 'confirm_historical_version' : 'restore_previous_version',
          { expectedGeneration: status.generation })
      } finally { owned.pending = false }
    },
  }
}
