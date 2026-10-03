import { defineStore } from 'pinia'
import { ref } from 'vue'
import { createNativeProjectionClient } from '@/api/tauri'
import { projectionErrorCode } from '@/api/nativeProjection'
import type { ProjectionResult, ResourceKind, ScopeTarget, SourceRef } from '@/types/nativeProjection'

export const useNativeProjectionStore = defineStore('native-projection', () => {
  const source = ref<SourceRef | null>(null)
  const result = ref<ProjectionResult | null>(null)
  const error = ref<string | null>(null)
  const isLoading = ref(false)
  const requestEpoch = ref('0')
  let owner: object = {}
  let sequence = BigInt(0)
  function clear() {
    owner = {}; source.value = null; result.value = null; error.value = null; isLoading.value = false
  }
  async function load(target: ScopeTarget, kind: ResourceKind, options: { query?: string; sessionId?: string; limit?: number; offset?: number } = {}) {
    clear()
    const selected = owner
    if (sequence === BigInt('18446744073709551615')) { error.value = 'SCOPE_EPOCH_EXHAUSTED'; return }
    requestEpoch.value = (++sequence).toString()
    const epoch = requestEpoch.value
    const frozenOptions = { ...options }
    isLoading.value = true
    try {
      const client = createNativeProjectionClient()
      const ref = await client.scope(target)
      if (owner !== selected) return
      const next = await client.read({ ...frozenOptions, source: ref, resourceKind: kind, requestEpoch: epoch })
      if (owner !== selected) return
      source.value = next.source
      result.value = next
      error.value = next.state === 'unavailable' ? next.reason : null
    } catch (failure) {
      if (owner === selected) error.value = projectionErrorCode(failure)
    } finally {
      if (owner === selected) isLoading.value = false
    }
  }
  /** Independent one-page read for an exact owning session. Does not share the
   * compatibility panel's mutable result slot or clear its in-flight request. */
  async function readScoped(target: ScopeTarget, kind: ResourceKind, identity: {
    cli: 'claude' | 'codex'; profileId: string; profileRevision: string
  }, isCurrent: () => boolean = () => true): Promise<ProjectionResult> {
    if (sequence === BigInt('18446744073709551615')) throw { code: 'SCOPE_EPOCH_EXHAUSTED' }
    const epoch = (++sequence).toString()
    const frozenTarget = { ...target }
    const expected = { ...identity }
    const client = createNativeProjectionClient()
    const selected = await client.scope(frozenTarget)
    if (!isCurrent()) throw { code: 'SOURCE_CHANGED' }
    if (selected.cli !== expected.cli || selected.profileId !== expected.profileId || selected.profileRevision !== expected.profileRevision) {
      throw { code: 'SOURCE_INVALID' }
    }
    // The protocol has no cross-page snapshot token. A bounded page explicitly
    // marked hasMore must not be presented as a complete resource observation.
    return client.read({ source: selected, resourceKind: kind, requestEpoch: epoch, limit: 200, offset: 0 })
  }
  return { source, result, error, isLoading, requestEpoch, clear, load, readScoped }
})
