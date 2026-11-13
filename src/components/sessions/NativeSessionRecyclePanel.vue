<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import { createNativeSessionRecycleClient } from '@/api/tauri'
import { recycleErrorCode, type RecycleRecord } from '@/api/nativeSessionRecycle'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { useNativeSessionRecoveryStore } from '@/stores/nativeSessionRecovery'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { normalizePath } from '@/utils/path'
import type { UnifiedSession } from '@/types/unifiedSession'
import type { SourceRef } from '@/types/nativeProjection'
const props = defineProps<{ sessions: UnifiedSession[]; active: boolean; projectPath?: string }>()
const { t } = useI18n(), history = useNativeHistoryStore(), recovery = useNativeSessionRecoveryStore()
const unavailableBySource = ref(new Map<string, { root: string; count: number }>())
const unavailable = computed(() => {
  const roots = new Map<string, number>()
  for (const item of unavailableBySource.value.values()) roots.set(item.root, Math.max(roots.get(item.root) ?? 0, item.count))
  return [...roots.values()].reduce((sum, count) => sum + count, 0)
})
const busy = ref(false), error = ref('')
const records = ref<{ source: SourceRef; record: RecycleRecord }[]>([])
let epoch = 0
const sourceIdentity = (source: SourceRef) => JSON.stringify([source.instanceId, source.sourceRootKey, source.profileId, source.profileRevision, source.target])
const knownSources = computed(() => {
  const byScope = new Map<string, SourceRef>()
  for (const entry of history.all()) {
    const source = entry.source
    if (source?.cli === 'claude' && source.basis === 'configured-profile' && source.target.kind === 'profile'
      && source.profileId === entry.context.profileId && source.profileRevision === entry.context.profileRevision && source.target.projectIds.includes(entry.context.projectId)
      && (!props.projectPath || normalizePath(props.projectPath) === normalizePath(entry.context.projectPath))) byScope.set(source.scopeId, source)
  }
  return [...byScope.values()]
})
const sources = computed(() => knownSources.value.filter(source => history.all().some(entry => entry.source?.scopeId === source.scopeId && entry.loaded && !entry.loading && !entry.error)))
const currentSource = (source: SourceRef) => sources.value.some(value => JSON.stringify(value) === JSON.stringify(source))
const readySignature = computed(() => JSON.stringify(sources.value))
const signature = computed(() => JSON.stringify([props.active, props.projectPath, knownSources.value.map(sourceIdentity)]))
const retained = computed(() => records.value.filter(item => item.record.state !== 'source'))
const pending = computed(() => [...recovery.pending.values()].filter(item => knownSources.value.some(source => source.sourceRootKey === item.root && source.instanceId === item.instance)
  && (!props.projectPath || normalizePath(props.projectPath) === normalizePath(item.projectPath))))
const sourceUnavailable = computed(() => retained.value.some(item => !currentSource(item.source)))
const visible = computed(() => props.active && (retained.value.length || pending.value.length || unavailable.value || error.value))
async function load() {
  if (!props.active || busy.value || !sources.value.length) return
  const readiness = readySignature.value
  const request = ++epoch, owner = signature.value, selectedSources = [...sources.value]
  const current = () => request === epoch && owner === signature.value && props.active
  busy.value = true; error.value = ''
  const next: typeof records.value = [...records.value], damaged = new Map(unavailableBySource.value)
  try {
    for (const source of selectedSources) {
      const result = await createNativeSessionRecycleClient().send({ source, action: 'list', operationId: null, sessionKey: null, acknowledgeRecoverable: false })
      if (!current()) return
      if (!currentSource(source)) continue
      for (let index = next.length - 1; index >= 0; index--) {
        if (sourceIdentity(next[index]!.source) === sourceIdentity(source)) next.splice(index, 1)
      }
      recovery.observe(source, result.records)
      damaged.set(sourceIdentity(source), { root: source.sourceRootKey, count: result.unavailableCount })
      next.push(...result.records.filter(record => !props.projectPath || normalizePath(record.projectPath) === normalizePath(props.projectPath)).map(record => ({ source, record })))
    }
    if (!current()) return
    unavailableBySource.value = damaged
    records.value = [...new Map(next.map(item => [`${item.source.sourceRootKey}/${item.record.operationId}`, item])).values()]
  } catch (failure) {
    if (current()) {
      const code = recycleErrorCode(failure)
      if (code !== 'RECYCLE_UNSUPPORTED') error.value = code
    }
  } finally {
    if (current()) {
      busy.value = false
      if (readiness !== readySignature.value) void load()
    }
  }
}
async function recheck() {
  if (!props.active || busy.value) return
  const owner = signature.value
  busy.value = true; error.value = ''
  try {
    for (const entry of history.all().filter(entry => entry.source && knownSources.value.some(source => sourceIdentity(source) === sourceIdentity(entry.source!)) && (!entry.loaded || entry.error) && !entry.loading)) {
      await history.load({ ...entry.context, force: true, retainOnFailure: true })
    }
  } catch (failure) { if (owner === signature.value) error.value = recycleErrorCode(failure) }
  finally { if (owner === signature.value) busy.value = false }
  if (owner === signature.value && !error.value) await load()
}
watch(readySignature, () => { if (props.active && !busy.value) void load() }, { flush: 'sync' })
let previousView = ''
watch(signature, () => {
  const view = JSON.stringify([props.active, props.projectPath]), identities = new Set(knownSources.value.map(sourceIdentity))
  const sameView = view === previousView
  previousView = view
  ++epoch
  records.value = sameView ? records.value.filter(item => identities.has(sourceIdentity(item.source))) : []
  unavailableBySource.value = sameView ? new Map([...unavailableBySource.value].filter(([identity]) => identities.has(identity))) : new Map()
  error.value = ''; busy.value = false
  if (props.active) void load()
}, { immediate: true, flush: 'sync' })
onBeforeUnmount(() => { ++epoch })
async function refresh(source: SourceRef) {
  for (const entry of history.all().filter(entry => entry.sourceRootKey === source.sourceRootKey)) {
    try { await history.load({ ...entry.context, force: true, retainOnFailure: true }) }
    finally { await useUnifiedSessionsStore().refresh(entry.context.projectPath, { cli: 'claude' }) }
  }
}
async function restore(item: (typeof records.value)[number]) {
  if (!props.active || busy.value || item.record.state !== 'stored' || item.record.originalOccupied || recovery.hasRestore(item.source, item.record.operationId)) return
  const source = sources.value.find(value => JSON.stringify(value) === JSON.stringify(item.source))
  if (!source || !records.value.includes(item)) return
  const owner = signature.value, request = ++epoch
  const current = () => props.active && owner === signature.value && request === epoch
  busy.value = true; error.value = ''
  try {
    recovery.beginRestore(source, item.record.operationId)
    try {
      const result = await createNativeSessionRecycleClient().send({ source, action: 'restore', operationId: item.record.operationId, sessionKey: null, acknowledgeRecoverable: true })
      recovery.settleRestore(source, item.record.operationId)
      recovery.observe(source, result.records)
    } finally { recovery.settleRestore(source, item.record.operationId) }
    await refresh(source)
    if (current()) { busy.value = false; await load() }
  } catch (failure) { if (current()) error.value = recycleErrorCode(failure) }
  finally { if (current()) busy.value = false }
}
</script>
<template>
  <div v-if="visible" data-recycle-legacy>
    <p>{{ t('recycleLegacyScope') }}</p>
    <p v-if="sourceUnavailable" data-recycle-source-unavailable role="status">{{ t('recycleSourceUnavailable') }}</p>
    <p v-if="error" data-recycle-error role="alert">{{ error }}</p>
    <p v-if="unavailable" role="alert">{{ t('recycleDamaged', { count: unavailable }) }}</p>
    <p v-for="item in pending" :key="item.operationId" data-recycle-pending>{{ item.nativeSessionId }} · {{ t('recyclePending') }}</p>
    <p v-if="busy" aria-live="polite">{{ t('deleteBusy') }}</p>
    <ul data-recycle-records><li v-for="item in retained" :key="`${item.source.sourceRootKey}/${item.record.operationId}`">
      <p>{{ item.record.nativeSessionId }} · {{ t(`recycleState_${item.record.state}`) }}</p>
      <p v-if="item.record.state === 'stored' && item.record.originalOccupied">{{ t('recycleConflict') }}</p>
      <AppButton v-if="item.record.state === 'stored'" data-recycle-restore :disabled="busy || !currentSource(item.source) || item.record.originalOccupied || recovery.hasRestore(item.source, item.record.operationId)" @click="restore(item)">{{ t('recycleRestore') }}</AppButton>
    </li></ul>
    <AppButton data-recycle-refresh :disabled="busy" @click="recheck">{{ t('recycleRefresh') }}</AppButton>
  </div>
</template>
<style scoped>
p { line-height: 1.6; overflow-wrap: anywhere; }
ul { padding-left: 20px; } li { margin-top: 12px; }
</style>
