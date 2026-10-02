// Synthetic catalogue transport for the explicit isolated visual fixture only.
// No native bridge, filesystem, network or installer operation is available here.
import wire from '../../tests/fixtures/version-history-preparation-wire.json'
import { invoke as blocked } from './tauriStub'
export function installHistoryFixture() {
  if (Object.prototype.hasOwnProperty.call(window, '__CC_DESK_DOCUMENT__')) throw new Error('VISUAL_DOCUMENT_ALREADY_PRESENT')
  const row = { releaseId: 'a'.repeat(32), assetId: '576637999', version: '0.17.7', publishedAt: '2026-09-20T10:45:00Z',
    platform: 'windows-x86_64', availablePlatforms: ['windows-x86_64'], packageFormat: 'nsis', verification: 'awaiting-verification',
    selectAllowed: true, installReady: false, dataModes: { freshSettings: 'available', keepCurrentData: 'unavailable' }, blockedReason: null }
  const bridge = {
    instanceId: 'isolated-visual-history',
    async invoke(command: string, payload: unknown) {
      const matches = (expected: unknown) => JSON.stringify(payload) === JSON.stringify(expected)
      if (command === 'list_history' && matches({ cursor: null })) return { rows: [row,
        { ...row, releaseId: 'c'.repeat(32), version: '0.17.6', assetId: null, packageFormat: null, selectAllowed: false,
          dataModes: { freshSettings: 'unavailable', keepCurrentData: 'unavailable' }, blockedReason: 'SIGNATURE_MISSING' }], nextCursor: null, truncated: false }
      if (command === 'select_history' && matches({ releaseId: row.releaseId, assetId: row.assetId })) return {
        selectionToken: 'b'.repeat(32), releaseId: row.releaseId, assetId: row.assetId, version: row.version,
        expiresAt: '2026-10-02T20:00:00Z', verification: 'awaiting-verification', installReady: false }
      if (command === 'begin_prepare_history' && matches({ selectionToken: 'b'.repeat(32) })) return wire.ticket
      if (command === 'prepare_history' && matches({ transactionId: wire.ticket.transactionId })) return wire.prepared
      if (command === 'cancel_prepare_history' && matches({ transactionId: wire.ticket.transactionId })) return wire.cancelled
      return blocked()
    },
  }
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { value: bridge, configurable: true })
  return () => { if ((window as any).__CC_DESK_DOCUMENT__ === bridge) delete (window as any).__CC_DESK_DOCUMENT__ }
}
