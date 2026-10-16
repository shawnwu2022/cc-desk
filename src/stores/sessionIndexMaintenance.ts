import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { sessionIndexMaintenance } from '@/api/tauri'
import { createNativeId } from '@/utils/nativeId'

export interface IndexScanControl { cancelled(): boolean; progress(done: number, total: number): void }
type Publish = (() => Promise<unknown> | unknown) | null
export const useSessionIndexMaintenanceStore = defineStore('sessionIndexMaintenance', () => {
  const state = ref<'idle'|'preparing'|'scanning'|'committing'|'completed'|'cancelled'|'failed'>('idle')
  const done = ref(0), total = ref(0), error = ref<string | null>(null)
  const busy = computed(() => ['preparing','scanning','committing'].includes(state.value))
  const needsReconciliation = ref(false)
  const needsRefresh = ref(false)
  const canCancel = computed(() => ['preparing','scanning'].includes(state.value))
  let scan: ((control: IndexScanControl) => Promise<void>) | null = null
  let publish: Publish = null
  let cancelled = false, task: Promise<void> | null = null
  let pending: { id: string; cancelled: boolean } | null = null
  function configure(next: NonNullable<typeof scan>, after: NonNullable<typeof publish>) {
    scan = next; publish = after
    return () => { if (scan === next) { cancelled = true; scan = null; publish = null } }
  }
  function cancel() { if (canCancel.value) cancelled = true }
  async function complete(after: Publish) {
    pending = null; needsReconciliation.value = false
    state.value = 'completed'
    try {
      if (!after) throw new Error('INDEX_REFRESH_UNAVAILABLE')
      await after()
      needsRefresh.value = false; error.value = null
    } catch { needsRefresh.value = true; error.value = 'indexRebuildRefreshFailed' }
  }
  async function reconcile() {
    if (!pending) return
    const transaction = pending
    try {
      const receipt = await sessionIndexMaintenance(transaction.id, 'status')
      if (receipt.state === 'committed') { await complete(publish); return }
      // 只有后端确认尚未发布的同一事务才允许取消，未知回执不重放写操作。
      if (receipt.state === 'staging' || receipt.state === 'absent') await sessionIndexMaintenance(transaction.id, 'cancel')
      pending = null; needsReconciliation.value = false
      state.value = transaction.cancelled ? 'cancelled' : 'failed'
      error.value = transaction.cancelled ? null : 'indexRebuildFailed'
    } catch {
      state.value = 'failed'; needsReconciliation.value = pending !== null
      error.value = pending ? 'indexRebuildCommitUnknown' : 'indexRebuildFailed'
    }
  }
  function run(): Promise<void> {
    if (task) return task
    if (needsRefresh.value) {
      state.value = 'committing'
      task = complete(publish).finally(() => { task = null })
      return task
    }
    if (pending) {
      state.value = 'committing'; error.value = null
      task = reconcile().finally(() => { task = null })
      return task
    }
    const worker = scan
    if (!worker) { state.value = 'failed'; error.value = 'indexRebuildUnavailable'; return Promise.resolve() }
    const id = createNativeId('index-rebuild')
    cancelled = false; done.value = 0; total.value = 0; error.value = null; state.value = 'preparing'
    task = (async () => {
      let staged = false, committing = false
      try {
        // begin 回执丢失也可能已创建事务，先保留身份再发送。
        pending = { id, cancelled: false }
        await sessionIndexMaintenance(id, 'begin'); staged = true
        if (!cancelled) {
          state.value = 'scanning'
          await worker({ cancelled: () => cancelled, progress: (completed, count) => { done.value = completed; total.value = count } })
        }
        if (cancelled) { pending.cancelled = true; await sessionIndexMaintenance(id, 'cancel'); pending = null; state.value = 'cancelled'; return }
        state.value = 'committing'; committing = true
        await sessionIndexMaintenance(id, 'commit')
        await complete(publish)
      } catch {
        // 在途扫描必须先结束；提交回执未知时不擅自回滚或重放。
        if (pending) pending.cancelled = cancelled
        if (staged && !committing) {
          try {
            await sessionIndexMaintenance(id, 'cancel'); pending = null
            state.value = cancelled ? 'cancelled' : 'failed'
            error.value = cancelled ? null : 'indexRebuildFailed'
          } catch { await reconcile() }
        } else if (pending) { await reconcile() }
        else { state.value = 'failed'; error.value = 'indexRebuildFailed' }
      } finally { needsReconciliation.value = pending !== null; task = null }
    })()
    return task
  }
  return { state, done, total, error, busy, canCancel, needsReconciliation, needsRefresh, configure, run, cancel }
})
