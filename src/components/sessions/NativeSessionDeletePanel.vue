<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import AppDialog from '@/components/ui/AppDialog.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { sessionDeleteTargetKey, useNativeSessionDeletesStore } from '@/stores/nativeSessionDeletes'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { nativeDeleteBinding } from '@/utils/nativeSessionDelete'
import { createNativeUuid } from '@/utils/nativeId'
import type { UnifiedSession } from '@/types/unifiedSession'
import type { SessionDeletePreviewRequest } from '@/types/nativeSessionDelete'

const props = defineProps<{ target: UnifiedSession | null; active: boolean; dialogOnly?: boolean }>()
const { t } = useI18n(), history = useNativeHistoryStore(), operations = useNativeSessionDeletesStore()
const resultsOnly = ref(false)
const retainedOperations = computed(() => [...operations.records].filter(([, item]) => item.receipt || ['executing', 'checking'].includes(item.phase)))
const hasResults = computed(() => retainedOperations.value.length > 0)
const open = ref(false), selectedKey = ref(''), permanent = ref(false), subtree = ref(false)
const binding = computed(() => {
  const row = props.target
  return props.active && row ? nativeDeleteBinding(row, row.nativeOrigin ? history.get(row.nativeOrigin) : undefined) : null
})
const signature = computed(() => binding.value ? JSON.stringify([props.target?.id, binding.value.source, binding.value.sessionKey, binding.value.nativeSessionId]) : '')
const operation = computed(() => operations.records.get(selectedKey.value))
const sameTarget = computed(() => {
  const current = binding.value, record = operation.value
  return !!current && !!record && sessionDeleteTargetKey({ ...record.request, source: current.source, sessionKey: current.sessionKey }) === selectedKey.value
})
const ownsOperation = computed(() => {
  const current = binding.value, record = operation.value
  if (!current || !record) return false
  return sameTarget.value
    && JSON.stringify(current.source) === JSON.stringify(record.request.source) && record.preview?.nativeSessionId === current.nativeSessionId
})
const busy = computed(() => operation.value && ['previewing', 'executing', 'checking'].includes(operation.value.phase))
function close() {
  operations.cancel(selectedKey.value)
  open.value = false; permanent.value = false; subtree.value = false
}
watch(signature, () => {
  if (operation.value && ['previewing', 'review', 'confirmation'].includes(operation.value.phase)) close()
}, { flush: 'sync' })
watch(() => props.active, active => { if (!active) close() })
onBeforeUnmount(() => operations.cancel(selectedKey.value))
async function review() {
  const current = binding.value
  if (!current) return
  resultsOnly.value = false
  const request: SessionDeletePreviewRequest = { operationId: createNativeUuid(), source: current.source, sessionKey: current.sessionKey }
  selectedKey.value = sessionDeleteTargetKey(request)
  const requestKey = selectedKey.value
  open.value = true; permanent.value = false; subtree.value = false
  const originalSignature = signature.value
  await operations.prepare(request, { title: props.target!.title, projectPath: current.origin.projectPath, origin: current.origin })
  // A 的迟到预览不能关闭后来已切换到 B 的窗口或取消 B 的票据。
  if (selectedKey.value !== requestKey || operation.value?.request.operationId !== request.operationId) return
  if (signature.value !== originalSignature) { close(); return }
  // 后端实核目标与所选行不一致时，不允许用显示行替换真实授权目标。
  if (operation.value?.preview?.state === 'ready' && operation.value.preview.nativeSessionId !== current.nativeSessionId) close()
}
async function execute() {
  const original = signature.value, current = binding.value
  if (!current || operation.value?.phase !== 'confirmation' || !ownsOperation.value || !permanent.value || !subtree.value) return
  const requestKey = selectedKey.value
  try {
    await operations.execute(requestKey, permanent.value, subtree.value, () => selectedKey.value === requestKey && ownsOperation.value && signature.value === original)
    await refreshAcknowledged(requestKey)
  } catch { /* 后端结果保留在操作账本；前置失权不发送删除。 */ }
}
async function refreshAcknowledged(key: string) {
  const record = operations.records.get(key), origin = record?.display?.origin
  if (record?.receipt?.state !== 'acknowledged' || !origin) return
  try { await history.load({ ...origin, force: true, retainOnFailure: true }) }
  finally { await useUnifiedSessionsStore().refresh(origin.projectPath, { cli: origin.cli }) }
}
async function inspect() {
  const key = selectedKey.value, record = operations.records.get(key), previous = record?.receipt?.state
  try {
    await operations.inspect(key)
    if (operations.records.get(key) === record && previous !== 'acknowledged') await refreshAcknowledged(key)
  } catch { /* 重读失败保留缓存及错误，不改写删除回执。 */ }
}
function reopen(key: string, readonly = false) { if (readonly) operations.cancel(selectedKey.value); resultsOnly.value = readonly; selectedKey.value = key; open.value = true; permanent.value = false; subtree.value = false }
function showResults() {
  const latest = retainedOperations.value[retainedOperations.value.length - 1]
  if (props.active && latest) reopen(latest[0], true)
}
async function reviewAgain() {
  if (!sameTarget.value || !operations.canReviewAgain(selectedKey.value)) return
  operations.restartReview(selectedKey.value)
  await review()
}
defineExpose({ review, showResults, hasResults })
</script>

<template>
  <div class="native-delete-panel">
    <AppButton v-if="!dialogOnly" data-native-cleanup-delete variant="danger" :disabled="!binding" @click="review">{{ t('deleteReview') }}</AppButton>
    <p v-if="!dialogOnly && target?.cli === 'codex' && !binding" class="delete-hint">{{ t('deleteEligibility') }}</p>
    <div v-if="!dialogOnly && operations.records.size" class="delete-operations">
      <AppButton v-for="[key, item] of operations.records" :key="key" data-delete-existing @click="reopen(key)">{{ t('deleteViewOperation') }} · {{ item.request.source.profileId }}</AppButton>
    </div>
    <AppDialog :open="open" :title="t(resultsOnly ? 'deleteViewOperation' : operation?.phase === 'confirmation' ? 'deleteSecondConfirmation' : 'deleteReview')" data-delete-dialog @close="close">
      <div v-if="dialogOnly && retainedOperations.length > 1" class="delete-operations">
        <AppButton v-for="[key, item] of retainedOperations" :key="key" data-delete-existing @click="reopen(key, true)">{{ t('deleteViewOperation') }} · {{ item.display?.title || item.request.source.profileId }}</AppButton>
      </div>
      <template v-if="operation">
        <dl class="delete-identity">
          <dt>{{ t('cleanupTargetLabel') }}</dt><dd>{{ operation.display?.title || t('cleanupUnknown') }}</dd>
          <dt>{{ t('cleanupProjectLabel') }}</dt><dd>{{ operation.display?.projectPath || t('cleanupUnknown') }}</dd>
          <dt>{{ t('cleanupNativeIdLabel') }}</dt><dd>{{ operation.preview?.nativeSessionId || t('cleanupUnknown') }}</dd>
          <dt>{{ t('cleanupProfileLabel') }}</dt><dd>{{ operation.request.source.profileId }} / {{ operation.request.source.profileRevision }}</dd>
          <dt>{{ t('cleanupRootLabel') }}</dt><dd>{{ operation.request.source.sourceRootKey }}</dd>
        </dl>
        <p>{{ t('deleteNativeSubtreeScope') }}</p>
        <p>{{ t('deleteNativeMaintenance') }}</p>
        <InlineNotice kind="warning" :message="t('deleteIrreversible')" />
        <p v-if="operation.phase === 'previewing' || operation.phase === 'executing' || operation.phase === 'checking'">{{ t('deleteBusy') }}</p>
        <template v-if="operation.phase === 'confirmation'">
          <label class="delete-ack"><input v-model="permanent" data-delete-ack-permanent type="checkbox" />{{ t('deleteAcknowledgePermanent') }}</label>
          <label class="delete-ack"><input v-model="subtree" data-delete-ack-subtree type="checkbox" />{{ t('deleteAcknowledgeSubtree') }}</label>
        </template>
        <div v-if="operation.phase === 'result'" data-delete-result aria-live="polite">
          <p v-if="operation.receipt">{{ t(`deleteState_${operation.receipt.state}`) }}</p>
          <p v-if="!operation.receipt">{{ t('deleteState_blocked') }}</p>
          <p>{{ t('deleteDescendantsUnverified') }}</p>
          <p v-if="operation.receipt?.rootPresent === false">{{ t('deleteRootAbsent') }}</p>
          <p v-if="operation.receipt?.rootPresent === true">{{ t('deleteRootPresent') }}</p>
          <code v-if="operation.error || operation.receipt?.reason || operation.preview?.reason">{{ operation.error || operation.receipt?.reason || operation.preview?.reason }}</code>
          <AppButton v-if="operation.receipt" data-delete-inspect @click="inspect">{{ t('deleteInspect') }}</AppButton>
          <AppButton v-if="!resultsOnly && sameTarget && operations.canReviewAgain(selectedKey)" data-delete-review-again @click="reviewAgain">{{ t('deleteReviewAgain') }}</AppButton>
        </div>
      </template>
      <template #footer>
        <AppButton data-delete-cancel @click="close">{{ t(busy && operation?.phase !== 'previewing' ? 'close' : 'cancel') }}</AppButton>
        <AppButton v-if="!resultsOnly && operation?.phase === 'review'" data-delete-next-confirm @click="operations.beginConfirmation(selectedKey)">{{ t('deleteContinueConfirmation') }}</AppButton>
        <AppButton v-if="!resultsOnly && operation?.phase === 'confirmation'" data-delete-final variant="danger" :disabled="!permanent || !subtree || !ownsOperation" @click="execute">{{ t('deletePermanently') }}</AppButton>
      </template>
    </AppDialog>
  </div>
</template>

<style scoped>
.delete-identity { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 8px 12px; font-size: 12px; }
dd { margin: 0; overflow-wrap: anywhere; } dt, .delete-hint { color: var(--text-secondary); }
p { line-height: 1.6; } .delete-ack { display: flex; align-items: flex-start; gap: 8px; margin-top: 12px; line-height: 1.6; }
.delete-operations { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 8px; }
</style>
