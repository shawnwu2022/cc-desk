import type { HistoryCatalogPage, HistorySelection, PreparationTicket, PreparedPackageSummary, CancelPrepareSummary, SwitchReview, SwitchTicket, OrdinaryInstallReview } from '@/types/versionHistory'
import { parseHistoryCatalogPage, parseHistorySelection, parsePreparationTicket, parsePreparedPackageSummary, parseCancelPrepareSummary, parseSwitchReview, parseSwitchTicket, parseOrdinaryInstallReview } from '@/utils/versionHistoryContracts'

export interface HistoryBridge { readonly instanceId: string; invoke(command: string, payload: unknown): Promise<unknown> }
export interface HistoryClient {
  isCurrent(): boolean
  list(cursor: string | null): Promise<HistoryCatalogPage>
  select(releaseId: string, assetId: string, version: string): Promise<HistorySelection>
  begin(selectionToken: string): Promise<PreparationTicket>
  prepare(transactionId: string, version: string): Promise<PreparedPackageSummary>
  cancel(transactionId: string): Promise<CancelPrepareSummary>
  inspectSwitch(preparationId: string, version: string): Promise<SwitchReview>
  beginSwitch(preparationId: string): Promise<SwitchTicket>
  inspectHistoricalInstall(preparationId: string, version: string): Promise<OrdinaryInstallReview>
  beginHistoricalInstall(preparationId: string): Promise<SwitchTicket>
}
function invalid(): never { throw new Error('HISTORY_INVALID_RESPONSE') }
function token(value: string) { if (!/^[a-f0-9]{32}$/.test(value)) invalid() }

/** Captures one admitted document transport. Never reacquires a bridge for a token. */
export function createHistoryClient(bridge: HistoryBridge, current: () => boolean): HistoryClient {
  const instance = bridge.instanceId
  function isCurrent() { try { return !!instance && bridge.instanceId === instance && current() } catch { return false } }
  async function invoke(command: string, payload: unknown) {
    if (!isCurrent()) throw new Error('DOCUMENT_BRIDGE_UNAVAILABLE')
    const result = await bridge.invoke(command, payload)
    if (!isCurrent()) throw new Error('BACKEND_INSTANCE_CHANGED')
    return result
  }
  return {
    isCurrent,
    async list(cursor) {
      if (cursor !== null) token(cursor)
      return parseHistoryCatalogPage(await invoke('list_history', { cursor }))
    },
    async select(releaseId, assetId, version) {
      token(releaseId)
      if (!/^[1-9][0-9]{0,19}$/.test(assetId) || BigInt(assetId) > BigInt('18446744073709551615')) invalid()
      const value = parseHistorySelection(await invoke('select_history', { releaseId, assetId }))
      if (value.releaseId !== releaseId || value.assetId !== assetId || value.version !== version) invalid()
      return value
    },
    async begin(selectionToken) {
      token(selectionToken)
      return parsePreparationTicket(await invoke('begin_prepare_history', { selectionToken }))
    },
    async prepare(transactionId, version) {
      token(transactionId)
      const value = parsePreparedPackageSummary(await invoke('prepare_history', { transactionId }))
      if (value.transactionId !== transactionId || value.version !== version) invalid()
      return value
    },
    async inspectSwitch(preparationId, version) {
      token(preparationId)
      const value = parseSwitchReview(await invoke('inspect_switch', { preparationId }))
      if (value.preparationId !== preparationId || value.version !== version) invalid()
      return value
    },
    async beginSwitch(preparationId) {
      token(preparationId)
      return parseSwitchTicket(await invoke('begin_switch', { preparationId, dataMode: 'fresh-settings' }))
    },
    async inspectHistoricalInstall(preparationId, version) {
      token(preparationId)
      const value = parseOrdinaryInstallReview(await invoke('inspect_historical_install', { transactionId: preparationId }))
      if (value.preparationId !== preparationId || value.version !== version) invalid()
      return value
    },
    async beginHistoricalInstall(preparationId) {
      token(preparationId)
      return parseSwitchTicket(await invoke('begin_historical_install', { transactionId: preparationId }))
    },
    async cancel(transactionId) {
      token(transactionId)
      const value = parseCancelPrepareSummary(await invoke('cancel_prepare_history', { transactionId }))
      if (value.transactionId !== transactionId) invalid()
      return value
    },
  }
}
