import { defineStore } from 'pinia'
import { reactive } from 'vue'
import type { SourceRef } from '@/types/nativeProjection'
import type { RecycleRecord } from '@/api/nativeSessionRecycle'
interface Pending { operationId: string; nativeSessionId: string; root: string; instance: string; projectPath: string }
const key = (source: SourceRef, nativeId: string) => JSON.stringify([source.instanceId, source.sourceRootKey, nativeId.toLowerCase()])
/** 未决意图跨组件关闭/重开保留；不同后端 instance 不能继承其执行身份。 */
export const useNativeSessionRecoveryStore = defineStore('native-session-recovery', () => {
  const pending = reactive(new Map<string, Pending>())
  function has(source: SourceRef, nativeId: string) { return pending.has(key(source, nativeId)) }
  function begin(source: SourceRef, nativeId: string, operationId: string, projectPath: string) {
    if (has(source, nativeId)) throw Object.assign(new Error('RECYCLE_OUTCOME_UNKNOWN'), { code: 'RECYCLE_OUTCOME_UNKNOWN' })
    pending.set(key(source, nativeId), { operationId, nativeSessionId: nativeId, root: source.sourceRootKey, instance: source.instanceId, projectPath })
  }
  function observe(source: SourceRef, records: RecycleRecord[]) {
    for (const record of records) {
      const id = key(source, record.nativeSessionId), item = pending.get(id)
      if (item?.operationId === record.operationId && ['stored', 'source'].includes(record.state)) pending.delete(id)
    }
  }
  function failed(source: SourceRef, nativeId: string, code: string) {
    // 这些固定前置错误证明该调用未移动原件；传输/失权/未知结果继续等待只读核实。
    if (['SOURCE_BUSY', 'RECYCLE_INVALID_REQUEST', 'RECYCLE_UNSUPPORTED', 'RECYCLE_UNAVAILABLE', 'RECYCLE_CAPACITY', 'RECYCLE_OPERATION_CONFLICT', 'SOURCE_CHANGED', 'SCOPE_STALE', 'SCOPE_UNKNOWN', 'FORBIDDEN'].includes(code)) pending.delete(key(source, nativeId))
  }
  return { pending, has, begin, observe, failed }
})
