import { defineStore } from 'pinia'
import { reactive } from 'vue'
import type { SessionDeleteClient, SessionDeletePreview, SessionDeletePreviewRequest, SessionDeleteReceipt } from '@/types/nativeSessionDelete'
import { createNativeSessionDeleteClient } from '@/api/tauri'
import { sessionDeleteErrorCode } from '@/api/nativeSessionDelete'
import type { NativeHistoryContext } from './nativeHistory'

export interface SessionDeleteOperation {
  request: SessionDeletePreviewRequest
  phase: 'previewing' | 'review' | 'confirmation' | 'executing' | 'checking' | 'result'
  preview?: SessionDeletePreview
  receipt?: SessionDeleteReceipt
  error?: string
  inspected?: boolean
  /** 仅用于复核界面展示；不进入后端请求或提供任何授权。 */
  display?: { title: string; projectPath: string; origin?: NativeHistoryContext }
}
export function sessionDeleteTargetKey(input: SessionDeletePreviewRequest): string { return JSON.stringify([input.source.instanceId, input.source.cli, input.source.profileId, input.source.profileRevision, input.source.sourceRootKey, input.sessionKey]) }
export const useNativeSessionDeletesStore = defineStore('native-session-deletes', () => {
  const records = reactive(new Map<string, SessionDeleteOperation>())
  const clients = new Map<string, SessionDeleteClient>()
  const executions = new Map<string, Promise<void>>()
  function unavailable(): never { throw new Error('DELETE_CONFIRMATION_REQUIRED') }
  async function prepare(request: SessionDeletePreviewRequest, display?: SessionDeleteOperation['display']): Promise<void> {
    const key = sessionDeleteTargetKey(request), old = records.get(key)
    // 已发出的操作保留原来源与客户端，关闭抽屉或新 scope 不会清掉未知归属。
    if (old && (['executing', 'checking'].includes(old.phase) || old.phase === 'result' && old.receipt)) return
    if (old?.phase === 'previewing') return
    const frozen = JSON.parse(JSON.stringify(request)) as SessionDeletePreviewRequest
    const record = reactive<SessionDeleteOperation>({ request: frozen, phase: 'previewing', ...(display ? { display: { ...display, ...(display.origin ? { origin: { ...display.origin } } : {}) } } : {}) })
    records.set(key, record)
    try {
      const client = createNativeSessionDeleteClient()
      clients.set(key, client)
      const preview = await client.preview(frozen)
      if (records.get(key) !== record) return
      record.preview = preview
      record.phase = preview.state === 'ready' ? 'review' : 'result'
    } catch (failure) {
      if (records.get(key) !== record) return
      record.error = sessionDeleteErrorCode(failure); record.phase = 'result'
    }
  }
  function beginConfirmation(key: string): void {
    const record = records.get(key)
    if (!record || record.phase !== 'review' || record.preview?.state !== 'ready') return unavailable()
    record.phase = 'confirmation'
  }
  function cancel(key: string): void {
    const record = records.get(key)
    if (!record || !['previewing', 'review', 'confirmation'].includes(record.phase)) return
    records.delete(key); clients.delete(key)
  }
  async function execute(key: string, permanent: boolean, subtree: boolean, owns: () => boolean): Promise<void> {
    const flight = executions.get(key)
    if (flight) return flight
    const record = records.get(key), client = clients.get(key)
    if (!record || !client || record.phase !== 'confirmation' || !record.preview?.previewId || !permanent || !subtree) return unavailable()
    let current = false
    try { current = owns() } catch { /* 来源已失效。 */ }
    if (!current) { cancel(key); return unavailable() }
    record.phase = 'executing'
    const previewId = record.preview.previewId
    const task = Promise.resolve().then(async () => {
      try {
        let stillOwns = false
        try { stillOwns = owns() } catch { /* 排队期间来源已撤销。 */ }
        if (!stillOwns) {
          record.receipt = { operationId: record.request.operationId, cli: record.request.source.cli, state: 'blocked', rootPresent: null, descendants: 'unverified', reason: 'DELETE_PREVIEW_CHANGED' }
          return
        }
        record.receipt = await client.confirm({ operationId: record.request.operationId, previewId, acknowledgePermanentDelete: true, acknowledgeNativeSubtree: true })
      } catch {
        // 请求已进入发送路径；异常或文档替换均不能证明删除未发生。
        record.receipt = { operationId: record.request.operationId, cli: record.request.source.cli, state: 'unknown', rootPresent: null, descendants: 'unverified', reason: 'DELETE_OUTCOME_UNKNOWN' }
      } finally { record.phase = 'result'; executions.delete(key) }
    })
    executions.set(key, task)
    return task
  }
  async function inspect(key: string): Promise<void> {
    const record = records.get(key), client = clients.get(key)
    if (!record?.receipt || !client || record.phase !== 'result') return
    record.phase = 'checking'; record.error = undefined
    try { record.receipt = await client.inspect({ operationId: record.request.operationId }); record.inspected = true }
    catch (failure) { record.error = sessionDeleteErrorCode(failure) }
    finally { record.phase = 'result' }
  }
  function canReviewAgain(key: string): boolean {
    const record = records.get(key)
    return !!record && record.phase === 'result' && (!record.receipt || ['blocked', 'unsupported'].includes(record.receipt.state)
      || record.receipt.state === 'partial' && record.receipt.rootPresent === true && record.inspected === true)
  }
  function restartReview(key: string): void {
    if (!canReviewAgain(key)) return unavailable()
    records.delete(key); clients.delete(key)
  }
  return { records, prepare, beginConfirmation, cancel, execute, inspect, canReviewAgain, restartReview }
})
