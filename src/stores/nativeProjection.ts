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
  return { source, result, error, isLoading, requestEpoch, clear, load }
})
