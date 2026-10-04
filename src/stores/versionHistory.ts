import { computed, ref, shallowRef, shallowReactive } from 'vue'
import { defineStore } from 'pinia'
import { createNativeHistoryClient } from '@/api/tauri'
import type { HistoryClient } from '@/api/versionHistory'
import type { HistoryRelease, HistorySelection, PreparedPackageSummary, SwitchReview, SwitchReviewAction } from '@/types/versionHistory'

/** Only allowlisted categories reach localized UI. Backend diagnostic text is never shown. */
export function historyErrorMessage(failure: unknown,
  fallback: 'historyErrorUnavailable' | 'historyOwnershipUnknown' | 'historyHandoffIssued' = 'historyErrorUnavailable'): string {
  const code = failure && typeof failure === 'object'
    ? ('code' in failure ? (failure as { code: unknown }).code : failure instanceof Error ? failure.message : null) : null
  if (code === 'HISTORY_RATE_LIMITED') return 'historyErrorRateLimit'
  if (['HISTORY_NETWORK_UNAVAILABLE', 'HISTORY_DOWNLOAD_TIMEOUT', 'HISTORY_RELEASE_UNAVAILABLE'].includes(String(code))) return 'historyErrorNetwork'
  if (['HISTORY_PREPARE_BUSY', 'HISTORY_CAPACITY', 'HISTORY_PREPARE_ALREADY_STARTED'].includes(String(code))) return 'historyErrorBusy'
  if (['HISTORY_STORAGE_UNAVAILABLE', 'HISTORY_PREPARATION_UNAVAILABLE'].includes(String(code))) return 'historyErrorStorage'
  if (['HISTORY_SIGNATURE_INVALID', 'HISTORY_DIGEST_MISMATCH', 'HISTORY_SIZE_MISMATCH', 'HISTORY_PACKAGE_CHANGED', 'HISTORY_REDIRECT_BLOCKED'].includes(String(code))) return 'historyErrorVerification'
  if (['FORBIDDEN', 'DOCUMENT_BRIDGE_UNAVAILABLE', 'BACKEND_INSTANCE_CHANGED'].includes(String(code))) return 'historyErrorDocument'
  if (['HISTORY_SELECTION_EXPIRED', 'HISTORY_SELECTION_CHANGED', 'HISTORY_SELECTION_UNKNOWN', 'HISTORY_CURSOR_EXPIRED', 'HISTORY_CURSOR_INVALID', 'HISTORY_CATALOG_CHANGED', 'HISTORY_PREPARE_EXPIRED'].includes(String(code))) return 'historyErrorSelection'
  return fallback
}
interface Preparation {
  client: HistoryClient
  selection: HistorySelection
  ticket: string | null
  beginPending: boolean
  preparePending: boolean
  cancelRequested: boolean
  cancelPending: boolean
  cancelUncertain: boolean
  cancelConfirmed: boolean
  failure: string | null
  switchAttempted: boolean
  epoch: number
}
export const useVersionHistoryStore = defineStore('versionHistory', () => {
  const rows = ref<HistoryRelease[]>([]), nextCursor = ref<string | null>(null), truncated = ref(false), loaded = ref(false)
  const loading = ref(false), selecting = ref(false), selected = ref<HistorySelection | null>(null)
  const prepared = ref<PreparedPackageSummary | null>(null), error = ref<string | null>(null)
  const phase = ref<'idle' | 'preparing' | 'verified' | 'cancelling' | 'cancelled' | 'failed' | 'unknown' | 'switching' | 'handoff-issued' | 'unavailable' | 'aborted'>('idle')
  const review = shallowRef<SwitchReview | null>(null), transactionId = ref<string | null>(null)
  const inspecting = ref(false), switching = ref(false)
  let inspection = 0

  let owner = 0, active = false, request = 0, pages = 0, client: HistoryClient | null = null
  let work: Preparation | null = null
  const busy = computed(() => loading.value || selecting.value || ['preparing', 'cancelling', 'switching'].includes(phase.value) || inspecting.value)
  const hasPreparation = computed(() => ['preparing', 'verified', 'cancelling', 'unknown', 'switching', 'handoff-issued', 'unavailable', 'aborted'].includes(phase.value))
  function owns(key: number, transport = client) {
    if (!active || key !== owner || !transport || transport !== client) return false
    if (!transport.isCurrent()) {
      selected.value = null; prepared.value = null; review.value = null; error.value = 'historyErrorDocument'
      return false
    }
    return true
  }
  function clearView() {
    ++request; ++inspection; inspecting.value = false; review.value = null; pages = 0; rows.value = []; nextCursor.value = null; truncated.value = false; loaded.value = false
    loading.value = false; selecting.value = false; selected.value = null; prepared.value = null; error.value = null
    if (!work) phase.value = 'idle'
  }
  function activate() {
    if (active) deactivate(owner)
    active = true; ++owner; clearView()
    try { client = work ? work.client : createNativeHistoryClient() } catch (failure) { client = null; error.value = historyErrorMessage(failure) }
    if (work && (work.switchAttempted || work.cancelUncertain)) {
      selected.value = work.selection
      phase.value = transactionId.value ? 'handoff-issued' : 'unknown'
      if (!client?.isCurrent()) error.value = 'historyErrorDocument'
    }
    return owner
  }
  function deactivate(key: number) {
    if (!active || key !== owner) return
    active = false; ++owner; client = null; clearView()
    if (work && !work.switchAttempted && !work.cancelUncertain) { work.cancelRequested = true; phase.value = 'cancelling'; void cancelWork(work) }
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
      work = null; review.value = null; prepared.value = null; selected.value = null; phase.value = item.failure ? 'failed' : 'cancelled'
    }
  }
  async function cancelWork(item: Preparation) {
    item.cancelRequested = true; prepared.value = null
    if (work !== item || item.cancelPending || item.cancelConfirmed || item.cancelUncertain || transactionId.value) return
    phase.value = 'cancelling'
    if (item.ticket === null) return
    item.cancelPending = true
    const epoch = ++item.epoch
    ++inspection; inspecting.value = false; review.value = null
    try {
      // Original-client cleanup must freshly prove it still owns preparation.
      const status = await item.client.inspectSwitch(item.ticket, item.selection.version)
      if (work !== item || epoch !== item.epoch) return
      if (status.transactionId) {
        retainIssued(item, status.transactionId)
        if (active && client === item.client) { review.value = status; phase.value = status.phase }
        return
      }
      if (status.phase === 'cancelled') {
        item.cancelConfirmed = true; finishCancellation(item)
        return
      }
      if (!status.allowedActions.includes('cancel-preparation')) {
        phase.value = 'unknown'
        if (active && client === item.client) review.value = status
        return
      }
      await item.client.cancel(item.ticket)
      item.cancelConfirmed = true
      if (work === item) { error.value = item.failure; finishCancellation(item) }
    } catch (failure) { if (work === item) {
      phase.value = transactionId.value ? 'handoff-issued' : 'unknown'
      item.cancelUncertain = true; ++item.epoch; ++inspection; review.value = null
      error.value = !item.client.isCurrent() || historyErrorMessage(failure) === 'historyErrorDocument' ? 'historyErrorDocument' : 'historyErrorCancel'
    } }
    finally { item.cancelPending = false }
  }
  async function prepare(key: number) {
    if (!owns(key) || busy.value || work || !selected.value) return
    const item = shallowReactive<Preparation>({ client: client!, selection: selected.value, ticket: null, beginPending: true,
      preparePending: false, cancelRequested: false, cancelPending: false, cancelConfirmed: false, cancelUncertain: false, failure: null, switchAttempted: false, epoch: 0 })
    work = item; phase.value = 'preparing'; error.value = null; review.value = null; transactionId.value = null
    try {
      const ticket = await item.client.begin(item.selection.selectionToken)
      item.ticket = ticket.transactionId; item.beginPending = false
      if (item.cancelRequested || !owns(key, item.client)) { await cancelWork(item); return }
      item.preparePending = true
      const download = item.client.prepare(ticket.transactionId, item.selection.version)
      void inspect(key)
      const result = await download
      item.preparePending = false
      if (item.cancelRequested || !owns(key, item.client)) { await cancelWork(item); return }
      if (work === item && selected.value?.selectionToken === item.selection.selectionToken) { prepared.value = result; phase.value = 'verified'; await inspect(key) }
    } catch (failure) {
      if (work === item && !item.cancelRequested && owns(key, item.client)) { item.failure = historyErrorMessage(failure); error.value = item.failure }
      // Pre-switch cleanup first checks ownership through its original document client.
      if (item.ticket) await cancelWork(item)
      else if (work === item) { work = null; phase.value = 'failed'; selected.value = null }
    } finally {
      item.beginPending = false; item.preparePending = false
      finishCancellation(item)
    }
  }
  function retainIssued(item: Preparation, id: string) {
    if (transactionId.value && transactionId.value !== id) throw new Error('HISTORY_INVALID_RESPONSE')
    item.switchAttempted = true; item.cancelRequested = false
    transactionId.value = id; phase.value = 'handoff-issued'; review.value = null
  }
  function allowed(action: SwitchReviewAction) {
    if (!active || !work || !client?.isCurrent() || client !== work.client || inspecting.value || switching.value || work.cancelPending) return false
    if (!review.value?.allowedActions.includes(action)) return false
    if (transactionId.value && !['refresh', 'prepare-again'].includes(action)) return false
    return action === 'cancel-preparation' || action === 'refresh' || !work.preparePending
  }
  const canInspect = computed(() => phase.value !== 'idle' && !!work?.ticket && !inspecting.value && !switching.value && !work.cancelPending
    && error.value !== 'historyErrorDocument' && (review.value === null || review.value.allowedActions.includes('refresh')))
  async function inspect(key: number) {
    const item = work
    if (!item?.ticket || !owns(key, item.client) || switching.value || item.cancelPending) return
    const serial = ++inspection, epoch = item.epoch
    inspecting.value = true; review.value = null
    try {
      const result = await item.client.inspectSwitch(item.ticket, item.selection.version)
      if (work !== item || serial !== inspection || epoch !== item.epoch || !owns(key, item.client)) return
      if (transactionId.value && result.transactionId !== transactionId.value) throw new Error('HISTORY_INVALID_RESPONSE')
      if (result.transactionId) retainIssued(item, result.transactionId)
      review.value = result; phase.value = result.phase; error.value = item.failure
      if (result.phase === 'cancelled' && !transactionId.value) {
        item.cancelConfirmed = true; finishCancellation(item)
      }
    } catch (failure) {
      if (work === item && serial === inspection && epoch === item.epoch && active && key === owner) {
        ++item.epoch; review.value = null
        phase.value = transactionId.value ? 'handoff-issued' : 'unknown'
        error.value = historyErrorMessage(failure, transactionId.value ? 'historyHandoffIssued' : 'historyOwnershipUnknown')
      }
    } finally { if (serial === inspection) inspecting.value = false }
  }
  async function beginSwitch(key: number, confirmed: SwitchReview) {
    const item = work
    if (!item?.ticket || !owns(key, item.client) || confirmed !== review.value || !allowed('begin-switch')) return
    item.switchAttempted = true; ++item.epoch; ++inspection
    review.value = null; inspecting.value = false; switching.value = true; phase.value = 'switching'; error.value = null
    try {
      const issued = await item.client.beginSwitch(item.ticket)
      if (work === item) retainIssued(item, issued.transactionId)
    } catch (failure) {
      // Unknown mutation failures need inspection, not the generic request retry hint.
      if (work === item) { ++item.epoch; review.value = null; phase.value = 'unknown'; error.value = historyErrorMessage(failure, 'historyOwnershipUnknown') }
    } finally { if (work === item) switching.value = false }
  }
  function prepareAgain(key: number) {
    if (!owns(key) || !allowed('prepare-again') || review.value?.phase !== 'aborted') return
    work = null; review.value = null; transactionId.value = null; selected.value = null; prepared.value = null
    phase.value = 'idle'; error.value = null
  }
  function cancel(key: number) {
    if (owns(key) && work && allowed('cancel-preparation')) { work.cancelUncertain = false; return cancelWork(work) }
  }
  return { rows, nextCursor, truncated, loaded, loading, selecting, selected, prepared, error, phase, busy, hasPreparation,
    review, transactionId, inspecting, switching, canInspect, allowed, inspect, beginSwitch, prepareAgain,
    activate, deactivate, list, select, prepare, cancel }
})
