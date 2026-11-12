import { defineStore } from 'pinia'
import { reactive } from 'vue'
import type { OpencodeArchiveBinding } from '@/utils/opencodeArchive'
import type { NativeHistoryEntry } from './nativeHistory'
import { validateProjectionSource } from '@/api/nativeProjection'
import { normalizePath } from '@/utils/path'
interface Pending { operationId: string; binding: OpencodeArchiveBinding; archived: boolean; sending: boolean }
const key = (binding: OpencodeArchiveBinding) => JSON.stringify([binding.source.sourceRootKey, binding.nativeSessionId])
/** 未知意图跨组件重开保留；换 scope、operation 或状态不能绕过同一目标。 */
export const useOpencodeArchiveStore = defineStore('opencode-archive', () => {
  const pending = reactive(new Map<string, Pending>())
  function has(binding: OpencodeArchiveBinding) { return pending.has(key(binding)) }
  function begin(binding: OpencodeArchiveBinding, operationId: string, archived: boolean) {
    if (has(binding)) throw Object.assign(new Error('ARCHIVE_OUTCOME_UNKNOWN'), { code: 'ARCHIVE_OUTCOME_UNKNOWN' })
    pending.set(key(binding), { operationId, binding, archived, sending: true })
  }
  function settled(binding: OpencodeArchiveBinding) { const item = pending.get(key(binding)); if (item) item.sending = false }
  function failed(binding: OpencodeArchiveBinding, code: string) {
    // 仅后端保证尚未发出 mutation 的固定错误允许用户重新选择确认。
    if (['OPENCODE_ARCHIVE_UNSUPPORTED', 'ARCHIVE_SCHEMA_UNSUPPORTED', 'ARCHIVE_SOURCE_UNAVAILABLE',
      'NODE_RUNTIME_UNAVAILABLE', 'SDK_HELPER_UNAVAILABLE', 'SOURCE_BUSY'].includes(code)) pending.delete(key(binding))
  }
  function observe(entry: NativeHistoryEntry) {
    if (!entry.loaded || entry.loading || entry.error || !entry.source) return
    let source
    try { source = validateProjectionSource(entry.source) } catch { return }
    if (source.cli !== 'opencode' || source.target.kind !== 'profile') return
    for (const [id, item] of pending) {
      const { binding } = item, original = binding.source, origin = binding.origin
      if (item.sending || source.instanceId !== original.instanceId || source.sourceRootKey !== original.sourceRootKey
        || source.profileId !== original.profileId || source.profileRevision !== original.profileRevision
        || !source.target.projectIds.includes(origin.projectId) || entry.context.projectId !== origin.projectId
        || entry.context.cli !== origin.cli || entry.context.profileId !== origin.profileId || entry.context.profileRevision !== origin.profileRevision
        || normalizePath(entry.context.projectPath) !== normalizePath(origin.projectPath)) continue
      const matches = entry.sessions.filter(row => row.sessionKey === binding.sessionKey && row.nativeSessionId === binding.nativeSessionId)
      // 缺席、旧状态及读失败均不是完成证据，只认同一目标的正向状态。
      if (matches.length === 1 && !matches[0]!.readFailure && matches[0]!.projectId === origin.projectId && matches[0]!.archived === item.archived) pending.delete(id)
    }
  }
  return { pending, has, begin, settled, failed, observe }
})
