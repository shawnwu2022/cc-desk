import { defineStore } from 'pinia'
import { reactive } from 'vue'
import { useNativeSessionDeletesStore, sessionDeleteTargetKey, type SessionDeleteOperation } from './nativeSessionDeletes'
import { nativeHistoryContextKey, useNativeHistoryStore } from './nativeHistory'
import { useUnifiedSessionsStore } from './unifiedSessions'
import { createNativeUuid } from '@/utils/nativeId'
import type { NativeDeleteBinding } from '@/utils/nativeSessionDelete'
import type { SessionDeletePreviewRequest } from '@/types/nativeSessionDelete'

export interface NativeDeleteBatchItem {
  binding: NativeDeleteBinding
  request: SessionDeletePreviewRequest
  key: string
  operation?: SessionDeleteOperation
  state: 'pending' | 'ready' | 'executing' | 'result' | 'skipped'
}
export interface NativeDeleteBatch {
  id: string
  phase: 'previewing' | 'review' | 'confirmation' | 'executing' | 'result'
  items: NativeDeleteBatchItem[]
  cancelled: boolean
}
type Owns = (item: NativeDeleteBatchItem) => boolean

export const useNativeSessionDeleteBatchesStore = defineStore('native-session-delete-batches', () => {
  const batches = reactive(new Map<string, NativeDeleteBatch>())
  const operations = useNativeSessionDeletesStore()
  const refreshed = new Set<string>()
  function owned(owns: Owns, item: NativeDeleteBatchItem) {
    try { return owns(item) } catch { return false }
  }
  function record(item: NativeDeleteBatchItem) {
    if (item.operation) return item.operation
    const value = operations.records.get(item.key)
    if (value?.request.operationId === item.request.operationId) { item.operation = value; return value }
    return undefined
  }
  function create(bindings: NativeDeleteBinding[]): NativeDeleteBatch {
    if (!bindings.length || [...batches.values()].some(batch => batch.phase !== 'result')) throw new Error('DELETE_IN_PROGRESS')
    const unique = new Map(bindings.map(binding => [binding.physicalKey, binding]))
    const batch = reactive<NativeDeleteBatch>({
      id: createNativeUuid(), phase: 'previewing', cancelled: false,
      items: [...unique.values()].map(value => {
        const binding = JSON.parse(JSON.stringify(value)) as NativeDeleteBinding
        const request = { operationId: createNativeUuid(), source: binding.source, sessionKey: binding.sessionKey }
        return { binding, request, key: sessionDeleteTargetKey(request), state: 'pending' }
      }),
    })
    batches.set(batch.id, batch)
    return batch
  }
  function skip(item: NativeDeleteBatchItem) {
    if (!['pending', 'ready'].includes(item.state)) return
    if (record(item)) operations.cancel(item.key)
    item.state = 'skipped'
  }
  function cancel(batch: NativeDeleteBatch) {
    batch.cancelled = true
    batch.items.forEach(skip)
    // 已发出的请求由原账本收口，不能因关闭对话框丢失未知结果。
    if (batch.phase !== 'executing') batch.phase = 'result'
  }
  async function prepare(batch: NativeDeleteBatch, owns: Owns) {
    if (batch.phase !== 'previewing' || batch.cancelled) return
    for (const item of batch.items) {
      if (batch.cancelled || !owned(owns, item)) { cancel(batch); return }
      const old = operations.records.get(item.key)
      if (old) {
        if (operations.canReviewAgain(item.key)) operations.restartReview(item.key)
        else { cancel(batch); return }
      }
      await operations.prepare(item.request, { title: item.binding.title, projectPath: item.binding.origin.projectPath, origin: item.binding.origin })
      if (batch.cancelled || !owned(owns, item)) { cancel(batch); return }
      const operation = record(item)
      if (operation?.phase !== 'review' || operation.preview?.nativeSessionId !== item.binding.nativeSessionId) {
        if (operation?.phase === 'review') operations.cancel(item.key)
        item.state = 'result'
        cancel(batch)
        return
      }
      item.state = 'ready'
    }
    if (!batch.cancelled && batch.items.every(item => owned(owns, item))) batch.phase = 'review'
    else cancel(batch)
  }
  function beginConfirmation(batch: NativeDeleteBatch, owns: Owns) {
    if (batch.phase !== 'review' || batch.items.some(item => !owned(owns, item) || record(item)?.phase !== 'review')) {
      cancel(batch); return
    }
    batch.items.forEach(item => operations.beginConfirmation(item.key))
    batch.phase = 'confirmation'
  }
  async function refreshAcknowledged(batch: NativeDeleteBatch) {
    const origins = new Map<string, NativeDeleteBinding['origin']>()
    for (const item of batch.items) {
      if (record(item)?.receipt?.state !== 'acknowledged' || refreshed.has(item.request.operationId)) continue
      refreshed.add(item.request.operationId)
      origins.set(nativeHistoryContextKey(item.binding.origin), item.binding.origin)
    }
    // 同一来源只刷新一次；刷新失败只保留读取错误，不改写删除回执。
    for (const origin of origins.values()) {
      try { await useNativeHistoryStore().load({ ...origin, force: true, retainOnFailure: true }) }
      catch { /* 来源读取错误留在历史 store。 */ }
      finally {
        try { await useUnifiedSessionsStore().refresh(origin.projectPath, { cli: origin.cli }) }
        catch { /* 目录刷新不能使已确认结果变成失败。 */ }
      }
    }
  }
  async function execute(batch: NativeDeleteBatch, permanent: boolean, subtree: boolean, owns: Owns) {
    if (batch.phase !== 'confirmation' || !permanent || !subtree || batch.cancelled) return
    if (batch.items.some(item => !owned(owns, item) || record(item)?.phase !== 'confirmation')) { cancel(batch); return }
    batch.phase = 'executing'
    try {
      for (const item of batch.items) {
        if (batch.cancelled || !owned(owns, item)) { cancel(batch); break }
        item.state = 'executing'
        try {
          await operations.execute(item.key, true, true, () => !batch.cancelled && owned(owns, item))
        } catch { /* 前置失权也不可跳过后继续发送。 */ }
        item.state = 'result'
        if (record(item)?.receipt?.state !== 'acknowledged') { cancel(batch); break }
      }
    } finally {
      batch.items.forEach(skip)
      batch.phase = 'result'
      await refreshAcknowledged(batch)
    }
  }
  function canInspect(item: NativeDeleteBatchItem) {
    return !!record(item)?.receipt && operations.records.get(item.key) === record(item)
  }
  async function inspect(batch: NativeDeleteBatch, item: NativeDeleteBatchItem) {
    if (batch.phase !== 'result' || !canInspect(item)) return
    await operations.inspect(item.key)
    await refreshAcknowledged(batch)
  }
  return { batches, create, prepare, cancel, beginConfirmation, execute, inspect, canInspect, record }
})
