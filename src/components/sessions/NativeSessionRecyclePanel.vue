<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import AppDialog from '@/components/ui/AppDialog.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import { createNativeSessionRecycleClient } from '@/api/tauri'
import { recycleErrorCode, type RecycleRecord } from '@/api/nativeSessionRecycle'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { useNativeSessionRecoveryStore } from '@/stores/nativeSessionRecovery'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { nativeRecycleBinding } from '@/utils/nativeSessionRecycle'
import { createNativeUuid } from '@/utils/nativeId'
import { normalizePath } from '@/utils/path'
import type { UnifiedSession } from '@/types/unifiedSession'
import type { SourceRef } from '@/types/nativeProjection'
const props = defineProps<{ sessions: UnifiedSession[]; active: boolean; projectPath?: string }>()
const { t } = useI18n(), history = useNativeHistoryStore(), recovery = useNativeSessionRecoveryStore()
const unavailable = ref(0)
const open = ref(false), busy = ref(false), error = ref(''), selected = ref(''), acknowledged = ref(false)
const records = ref<{ source: SourceRef; record: RecycleRecord }[]>([])
const eligible = computed(() => props.sessions.map(row => nativeRecycleBinding(row, row.nativeOrigin ? history.get(row.nativeOrigin) : undefined)).filter(row => row !== null).filter(row => !recovery.has(row.source, row.nativeSessionId)))
const choices = computed(() => [{ value: '', label: t('recycleChoose') }, ...eligible.value.map(row => ({ value: row.rowId, label: `${row.title} · ${row.nativeSessionId}` }))])
const target = computed(() => eligible.value.find(row => row.rowId === selected.value))
const signature = computed(() => JSON.stringify(target.value ?? null))
const sources = computed(() => {
  const byScope = new Map<string, SourceRef>()
  for (const entry of history.all()) if (entry.loaded && !entry.loading && !entry.error && entry.source?.cli === 'claude' && entry.source.basis === 'configured-profile' && entry.source.target.kind === 'profile'
    && (!props.projectPath || normalizePath(props.projectPath) === normalizePath(entry.context.projectPath))) byScope.set(entry.source.scopeId, entry.source)
  return [...byScope.values()]
})
watch(signature, () => { acknowledged.value = false })
watch(() => props.active, active => { if (!active) { open.value = false; selected.value = ''; acknowledged.value = false } })
const pending = computed(() => [...recovery.pending.values()].filter(item => sources.value.some(source => source.sourceRootKey === item.root && source.instanceId === item.instance) && (!props.projectPath || normalizePath(props.projectPath) === normalizePath(item.projectPath))))
async function load() {
  const next: typeof records.value = []
  const damaged = new Map<string, number>()
  for (const source of sources.value) {
    const result = await createNativeSessionRecycleClient().send({ source, action: 'list', operationId: null, sessionKey: null, acknowledgeRecoverable: false })
    recovery.observe(source, result.records)
    damaged.set(source.sourceRootKey, Math.max(damaged.get(source.sourceRootKey) ?? 0, result.unavailableCount))
    next.push(...result.records.filter(record => !props.projectPath || normalizePath(record.projectPath) === normalizePath(props.projectPath)).map(record => ({ source, record })))
  }
  unavailable.value = [...damaged.values()].reduce((sum, count) => sum + count, 0)
  records.value = [...new Map(next.map(item => [`${item.source.sourceRootKey}/${item.record.operationId}`, item])).values()]
}
async function show() {
  if (!props.active || busy.value) return
  open.value = true; selected.value = ''; acknowledged.value = false; error.value = ''; busy.value = true
  try { await load() } catch (failure) { error.value = recycleErrorCode(failure) } finally { busy.value = false }
}
async function refresh(source: SourceRef) {
  const entries = history.all().filter(entry => entry.sourceRootKey === source.sourceRootKey)
  for (const entry of entries) {
    try { await history.load({ ...entry.context, force: true, retainOnFailure: true }) }
    finally { await useUnifiedSessionsStore().refresh(entry.context.projectPath, { cli: 'claude' }) }
  }
}
async function remove() {
  const binding = target.value
  if (!props.active || !open.value || busy.value || !acknowledged.value || !binding) return
  busy.value = true; error.value = ''
  const source = binding.source
  selected.value = ''; acknowledged.value = false
  const operationId = createNativeUuid()
  try {
    recovery.begin(source, binding.nativeSessionId, operationId, binding.origin.projectPath)
    const response = await createNativeSessionRecycleClient().send({ source, action: 'remove', operationId, sessionKey: binding.sessionKey, acknowledgeRecoverable: true })
    if (response.records[0]?.nativeSessionId !== binding.nativeSessionId) throw { code: 'RECYCLE_OUTCOME_UNKNOWN' }
    recovery.observe(source, response.records)
    await refresh(source); await load()
  } catch (failure) {
    error.value = recycleErrorCode(failure)
    recovery.failed(source, binding.nativeSessionId, error.value)
    // 不重发移动；后端日志允许用户明确刷新或重新打开恢复区。
  } finally { busy.value = false }
}
async function restore(item: (typeof records.value)[number]) {
  if (!props.active || !open.value || busy.value || item.record.state !== 'stored' || item.record.originalOccupied) return
  const source = sources.value.find(value => value.scopeId === item.source.scopeId) ?? history.all().find(entry => entry.sourceRootKey === item.source.sourceRootKey && normalizePath(entry.context.projectPath) === normalizePath(item.record.projectPath) && !entry.loading && !entry.error)?.source
  if (!source) { error.value = 'SCOPE_STALE'; return }
  busy.value = true; error.value = ''
  try {
    await createNativeSessionRecycleClient().send({ source, action: 'restore', operationId: item.record.operationId, sessionKey: null, acknowledgeRecoverable: true })
    await refresh(source); await load()
  } catch (failure) { error.value = recycleErrorCode(failure) } finally { busy.value = false }
}
</script>
<template>
  <AppButton data-recycle-open :disabled="!active || busy" @click="show">{{ t('recycleOpen') }}</AppButton>
  <AppDialog :open="open" :title="t('recycleOpen')" data-recycle-dialog @close="open = false">
    <p>{{ t('recycleScope') }}</p>
    <InlineNotice kind="warning" :message="t('recycleLimitations')" />
    <p v-if="error" data-recycle-error role="alert">{{ error }}</p>
    <p v-if="unavailable" role="alert">{{ t('recycleDamaged', { count: unavailable }) }}</p>
    <p v-for="item in pending" :key="item.operationId" data-recycle-pending>{{ item.nativeSessionId }} · {{ t('recyclePending') }}</p>
    <p v-if="busy" aria-live="polite">{{ t('deleteBusy') }}</p>
    <AppSelect v-model="selected" data-recycle-select :label="t('recycleChoose')" :options="choices" :disabled="busy" />
    <template v-if="target">
      <p>{{ target.nativeSessionId }}</p>
      <label><input v-model="acknowledged" data-recycle-ack type="checkbox" :disabled="busy">{{ t('recycleAcknowledge') }}</label>
      <AppButton data-recycle-remove :disabled="busy || !acknowledged || !active" @click="remove">{{ t('recycleRemove') }}</AppButton>
    </template>
    <ul data-recycle-records><li v-for="item in records" :key="`${item.source.sourceRootKey}/${item.record.operationId}`">
      <p>{{ item.record.nativeSessionId }} · {{ t(`recycleState_${item.record.state}`) }}</p>
      <p v-if="item.record.state === 'stored' && item.record.originalOccupied">{{ t('recycleConflict') }}</p>
      <AppButton v-if="item.record.state === 'stored'" data-recycle-restore :disabled="busy || item.record.originalOccupied || !active" @click="restore(item)">{{ t('recycleRestore') }}</AppButton>
    </li></ul>
    <template #footer><AppButton data-recycle-refresh :disabled="busy" @click="show">{{ t('recycleRefresh') }}</AppButton><AppButton @click="open = false">{{ t('close') }}</AppButton></template>
  </AppDialog>
</template>
<style scoped>
p { line-height: 1.6; overflow-wrap: anywhere; }
label { display: flex; align-items: flex-start; gap: 8px; margin: 12px 0; }
ul { padding-left: 20px; } li { margin-top: 12px; }
</style>
