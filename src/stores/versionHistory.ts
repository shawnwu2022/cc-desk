import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { createNativeHistoryClient } from '@/api/tauri'
import type { HistoryClient } from '@/api/versionHistory'
import type { HistoryRelease, HistorySelection, PreparedPackageSummary } from '@/types/versionHistory'

/** Only allowlisted categories reach localized UI. Backend diagnostic text is never shown. */
export function historyErrorMessage(failure: unknown): string {
  const code = failure && typeof failure === 'object'
    ? ('code' in failure ? (failure as { code: unknown }).code : failure instanceof Error ? failure.message : null) : null
  if (code === 'HISTORY_RATE_LIMITED') return 'historyErrorRateLimit'
  if (['HISTORY_NETWORK_UNAVAILABLE', 'HISTORY_DOWNLOAD_TIMEOUT', 'HISTORY_RELEASE_UNAVAILABLE'].includes(String(code))) return 'historyErrorNetwork'
  if (['HISTORY_PREPARE_BUSY', 'HISTORY_CAPACITY', 'HISTORY_PREPARE_ALREADY_STARTED'].includes(String(code))) return 'historyErrorBusy'
  if (['HISTORY_STORAGE_UNAVAILABLE', 'HISTORY_PREPARATION_UNAVAILABLE'].includes(String(code))) return 'historyErrorStorage'
  if (['HISTORY_SIGNATURE_INVALID', 'HISTORY_DIGEST_MISMATCH', 'HISTORY_SIZE_MISMATCH', 'HISTORY_PACKAGE_CHANGED', 'HISTORY_REDIRECT_BLOCKED'].includes(String(code))) return 'historyErrorVerification'
  if (['FORBIDDEN', 'DOCUMENT_BRIDGE_UNAVAILABLE', 'BACKEND_INSTANCE_CHANGED'].includes(String(code))) return 'historyErrorDocument'
  if (['HISTORY_SELECTION_EXPIRED', 'HISTORY_SELECTION_CHANGED', 'HISTORY_SELECTION_UNKNOWN', 'HISTORY_CURSOR_EXPIRED', 'HISTORY_CURSOR_INVALID', 'HISTORY_CATALOG_CHANGED', 'HISTORY_PREPARE_EXPIRED'].includes(String(code))) return 'historyErrorSelection'
  return 'historyErrorUnavailable'
}
interface Preparation {
  client: HistoryClient
  selection: HistorySelection
  ticket: string | null
  beginPending: boolean
  preparePending: boolean
  cancelRequested: boolean
  cancelPending: boolean
  cancelConfirmed: boolean
  failure: string | null
}
export const useVersionHistoryStore = defineStore('versionHistory', () => {
  const rows = ref<HistoryRelease[]>([]), nextCursor = ref<string | null>(null), truncated = ref(false), loaded = ref(false)
  const loading = ref(false), selecting = ref(false), selected = ref<HistorySelection | null>(null)
  const prepared = ref<PreparedPackageSummary | null>(null), error = ref<string | null>(null)
  const phase = ref<'idle' | 'preparing' | 'verified' | 'cancelling' | 'cancelled' | 'failed' | 'cancel-failed'>('idle')
  let owner = 0, active = false, request = 0, pages = 0, client: HistoryClient | null = null
  let work: Preparation | null = null
  const busy = computed(() => loading.value || selecting.value || ['preparing', 'cancelling', 'cancel-failed'].includes(phase.value))
  const hasPreparation = computed(() => ['preparing', 'verified', 'cancelling', 'cancel-failed'].includes(phase.value))
  function owns(key: number, transport = client) {
    if (!active || key !== owner || !transport || transport !== client) return false
    if (!transport.isCurrent()) {
      selected.value = null; prepared.value = null; error.value = 'historyErrorDocument'
      return false
    }
    return true
  }
  function clearView() {
    ++request; pages = 0; rows.value = []; nextCursor.value = null; truncated.value = false; loaded.value = false
    loading.value = false; selecting.value = false; selected.value = null; prepared.value = null; error.value = null
    if (!work) phase.value = 'idle'
  }
  function activate() {
    if (active) deactivate(owner)
    active = true; ++owner; clearView()
    try { client = createNativeHistoryClient() } catch (failure) { client = null; error.value = historyErrorMessage(failure) }
    return owner
  }
  function deactivate(key: number) {
    if (!active || key !== owner) return
    active = false; ++owner; client = null; clearView()
    if (work) { work.cancelRequested = true; phase.value = 'cancelling'; void cancelWork(work) }
  }
  async function list(key: number, more = false) {
    if (!owns(key) || busy.value || work || (more && (!nextCursor.value || pages >= 10))) return
    const transport = client!, serial = ++request, cursor = more ? nextCursor.value : null
    loading.value = true; error.value = null; selected.value = null; prepared.value = null; phase.value = 'idle'
    if (!more) { rows.value = []; loaded.value = false; pages = 0; nextCursor.value = null; truncated.value = false }
    try {
      const page = await transport.list(cursor)
      if (!owns(key, transport) || serial !== request) return
      const combined = more ? [...rows.value, ...page.rows] : page.rows
      if (combined.length > 250 || new Set(combined.map(row => row.releaseId)).size !== combined.length) throw new Error('HISTORY_INVALID_RESPONSE')
      rows.value = combined; ++pages; loaded.value = true
      truncated.value = page.truncated || (pages >= 10 && page.nextCursor !== null)
      nextCursor.value = truncated.value ? null : page.nextCursor
    } catch (failure) { if (owns(key, transport) && serial === request) error.value = historyErrorMessage(failure) }
    finally { if (active && key === owner && serial === request) loading.value = false }
  }
  async function select(key: number, row: HistoryRelease) {
    if (!owns(key) || busy.value || work || !rows.value.includes(row) || !row.selectAllowed || !row.assetId) return
    const transport = client!, serial = ++request
    selecting.value = true; selected.value = null; prepared.value = null; phase.value = 'idle'; error.value = null
    try {
      const result = await transport.select(row.releaseId, row.assetId, row.version)
      if (owns(key, transport) && serial === request) selected.value = result
    } catch (failure) { if (owns(key, transport) && serial === request) error.value = historyErrorMessage(failure) }
    finally { if (active && key === owner && serial === request) selecting.value = false }
  }
  function finishCancellation(item: Preparation) {
    if (work === item && item.cancelConfirmed && !item.beginPending && !item.preparePending) {
      work = null; prepared.value = null; selected.value = null; phase.value = item.failure ? 'failed' : 'cancelled'
    }
  }
  async function cancelWork(item: Preparation) {
    item.cancelRequested = true; prepared.value = null
    if (work !== item || item.cancelPending || item.cancelConfirmed) return
    phase.value = 'cancelling'
    if (item.ticket === null) return
    item.cancelPending = true
    try {
      await item.client.cancel(item.ticket)
      item.cancelConfirmed = true
      if (work === item) { error.value = item.failure; finishCancellation(item) }
    } catch (failure) { if (work === item) {
      phase.value = 'cancel-failed'
      error.value = !item.client.isCurrent() || historyErrorMessage(failure) === 'historyErrorDocument' ? 'historyErrorDocument' : 'historyErrorCancel'
    } }
    finally { item.cancelPending = false }
  }
  async function prepare(key: number) {
    if (!owns(key) || busy.value || work || !selected.value) return
    const item: Preparation = { client: client!, selection: selected.value, ticket: null, beginPending: true,
      preparePending: false, cancelRequested: false, cancelPending: false, cancelConfirmed: false, failure: null }
    work = item; phase.value = 'preparing'; error.value = null
    try {
      const ticket = await item.client.begin(item.selection.selectionToken)
      item.ticket = ticket.transactionId; item.beginPending = false
      if (item.cancelRequested || !owns(key, item.client)) { await cancelWork(item); return }
      item.preparePending = true
      const result = await item.client.prepare(ticket.transactionId, item.selection.version)
      if (item.cancelRequested || !owns(key, item.client)) { await cancelWork(item); return }
      if (work === item && selected.value?.selectionToken === item.selection.selectionToken) { prepared.value = result; phase.value = 'verified' }
    } catch (failure) {
      if (work === item && !item.cancelRequested && owns(key, item.client)) { item.failure = historyErrorMessage(failure); error.value = item.failure }
      // A known ticket is always retired through its original document client.
      if (item.ticket) await cancelWork(item)
      else if (work === item) { work = null; phase.value = 'failed'; selected.value = null }
    } finally {
      item.beginPending = false; item.preparePending = false
      finishCancellation(item)
    }
  }
  function cancel(key: number) { if (active && key === owner && work) return cancelWork(work) }
  return { rows, nextCursor, truncated, loaded, loading, selecting, selected, prepared, error, phase, busy, hasPreparation,
    activate, deactivate, list, select, prepare, cancel }
})
