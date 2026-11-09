import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { createNativeVersionDiagnosticsClient, type VersionDiagnosticsClient, type VersionProbeReason } from '@/api/cliVersionDiagnostics'
import type { NativeCliKind } from '@/types/cli'

export interface DiagnosticProfile { cli: NativeCliKind; id: string; revision: string; name: string }
type State = 'not-collected' | 'collecting' | 'cancelling' | 'known' | 'unknown' | 'cancelled'
type Row = DiagnosticProfile & { source: string; state: State; version: string | null; reason: VersionProbeReason | null }
type Pending = { client: VersionDiagnosticsClient; row: Row; discarded: boolean; uncertain: boolean; cancelling: boolean }

export function useCliVersionDiagnostics(selected: () => readonly DiagnosticProfile[], active: () => boolean) {
  const rows = ref<Row[]>([]), pending = new Map<string, Pending>(), pendingCount = ref(0)
  const busy = computed(() => pendingCount.value !== 0)
  let sourceNumber = 0
  const aliases = new Map<string, string>()
  function source(profile: DiagnosticProfile) {
    const key = JSON.stringify([profile.cli, profile.id, profile.revision])
    let alias = aliases.get(key)
    if (!alias) { alias = `selected-profile-${++sourceNumber}`; aliases.set(key, alias) }
    return alias
  }
  function reset() {
    rows.value = selected().map(profile => ({ ...profile, source: source(profile), state: 'not-collected', version: null, reason: null }))
  }
  async function discard() {
    const cancellations: Promise<unknown>[] = []
    for (const [id, request] of pending) {
      if (request.cancelling || request.discarded && !request.uncertain) continue
      request.discarded = true
      request.cancelling = true
      request.row.state = 'cancelling'; request.row.version = null; request.row.reason = null
      // requested 只表示已接收请求；原探测回执收口前继续保留 busy。
      cancellations.push(request.client.cancel(id).then(receipt => {
        if (receipt.status === 'settled' && request.uncertain) {
          pending.delete(id); pendingCount.value = pending.size
          if (active() && rows.value.includes(request.row)) request.row.state = 'cancelled'
        }
      }).catch(() => undefined).finally(() => { request.cancelling = false }))
    }
    await Promise.all(cancellations)
  }
  function invalidate(reason: VersionProbeReason | null = null) {
    void discard(); reset()
    if (reason) for (const row of rows.value) { row.state = 'unknown'; row.reason = reason }
  }
  watch(() => JSON.stringify(selected().map(profile => [profile.cli, profile.id, profile.revision, profile.name])), () => {
    void discard(); reset()
  }, { flush: 'sync', immediate: true })
  watch(active, value => { if (!value) { void discard(); reset() } }, { flush: 'sync' })
  onBeforeUnmount(() => { void discard(); reset() })

  async function collect() {
    if (!active() || busy.value) return
    let client: VersionDiagnosticsClient
    try { client = createNativeVersionDiagnosticsClient() }
    catch { for (const row of rows.value) { row.state = 'unknown'; row.reason = 'unavailable'; row.version = null }; return }
    const tasks = rows.value.map(async row => {
      let requestId: string
      try { requestId = crypto.randomUUID() }
      catch { row.state = 'unknown'; row.reason = 'unavailable'; row.version = null; return }
      const request: Pending = { client, row, discarded: false, uncertain: false, cancelling: false }
      pending.set(requestId, request); pendingCount.value = pending.size
      row.state = 'collecting'; row.version = null; row.reason = null
      try {
        const receipt = await client.probe({ requestId, profileId: row.id, expectedProfileRevision: row.revision, cli: row.cli })
        request.uncertain = receipt.reason === 'termination-unknown'
        if (!active() || !rows.value.includes(row)) return
        if (request.uncertain) { row.state = 'unknown'; row.reason = 'termination-unknown'; return }
        if (request.discarded) {
          row.state = receipt.state === 'cancelled' ? 'cancelled' : 'not-collected'
          return
        }
        row.state = receipt.state; row.version = receipt.version; row.reason = receipt.reason
      } catch {
        if (active() && rows.value.includes(row)) { row.state = 'unknown'; row.version = null; row.reason = 'unavailable' }
      } finally { if (!request.uncertain) pending.delete(requestId); pendingCount.value = pending.size }
    })
    await Promise.all(tasks)
  }
  const summary = computed(() => {
    const safe = (cli: NativeCliKind) => {
      const row = rows.value.find(item => item.cli === cli)
      return row ? { source: row.source, state: row.state, version: row.version, reason: row.reason }
        : { source: null, state: 'not-collected' as const, version: null, reason: null }
    }
    return { claude: safe('claude'), codex: safe('codex'), opencode: safe('opencode') }
  })
  return { rows, busy, collect, cancel: discard, summary, invalidate }
}
