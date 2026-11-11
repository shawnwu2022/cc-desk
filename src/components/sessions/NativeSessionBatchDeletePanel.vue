<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import AppInput from '@/components/ui/AppInput.vue'
import AppDialog from '@/components/ui/AppDialog.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { useNativeSessionDeletesStore, sessionDeleteTargetKey } from '@/stores/nativeSessionDeletes'
import { useNativeSessionDeleteBatchesStore, type NativeDeleteBatchItem } from '@/stores/nativeSessionDeleteBatches'
import { nativeDeleteBinding, type NativeDeleteBinding } from '@/utils/nativeSessionDelete'
import type { UnifiedSession } from '@/types/unifiedSession'

const props = defineProps<{ sessions: UnifiedSession[]; active: boolean }>()
const { t } = useI18n(), history = useNativeHistoryStore(), batches = useNativeSessionDeleteBatchesStore(), operations = useNativeSessionDeletesStore()
const selecting = ref(false), selected = ref<string[]>([]), search = ref('')
const open = ref(false), batchId = ref(''), permanent = ref(false), subtree = ref(false)
let mounted = true
const batch = computed(() => batches.batches.get(batchId.value))
const candidates = computed(() => {
  const seen = new Set<string>()
  return props.sessions.filter(row => row.archived).map(row => {
    let binding = nativeDeleteBinding(row, row.nativeOrigin ? history.get(row.nativeOrigin) : undefined)
    if (binding) {
      const key = sessionDeleteTargetKey({ operationId: '', source: binding.source, sessionKey: binding.sessionKey })
      const old = operations.records.get(key)
      if (seen.has(binding.physicalKey) || old && (['executing', 'checking'].includes(old.phase)
        || old.phase === 'result' && old.receipt && !operations.canReviewAgain(key))) binding = null
      else seen.add(binding.physicalKey)
    }
    return { row, binding }
  })
})
const visible = computed(() => {
  const query = search.value.trim().toLocaleLowerCase()
  return candidates.value.filter(({ row }) => !query || [row.title, row.projectPath, row.cli].some(value => value.toLocaleLowerCase().includes(query)))
})
const bindings = computed(() => candidates.value.filter(item => item.binding && selected.value.includes(item.row.id)).map(item => item.binding!))
const locked = computed(() => [...batches.batches.values()].some(item => item.phase !== 'result'))
const summary = computed(() => {
  const items = batch.value?.items ?? []
  return {
    total: items.length,
    acknowledged: items.filter(item => batches.record(item)?.receipt?.state === 'acknowledged').length,
    skipped: items.filter(item => item.state === 'skipped').length,
    other: items.filter(item => item.state === 'result' && batches.record(item)?.receipt?.state !== 'acknowledged').length,
  }
})
function currentBinding(id: string): NativeDeleteBinding | null {
  const row = props.sessions.find(item => item.id === id)
  return row ? nativeDeleteBinding(row, row.nativeOrigin ? history.get(row.nativeOrigin) : undefined) : null
}
function owns(item: NativeDeleteBatchItem) {
  return mounted && props.active && currentBinding(item.binding.rowId)?.signature === item.binding.signature
}
watch(() => candidates.value.map(item => item.binding?.signature ?? '').join('\n'), () => {
  selected.value = selected.value.filter(id => candidates.value.some(item => item.row.id === id && item.binding))
  const current = batch.value
  if (current && ['previewing', 'review', 'confirmation'].includes(current.phase) && current.items.some(item => !owns(item))) close()
  else if (current?.phase === 'executing' && current.items.some(item => ['pending', 'ready'].includes(item.state) && !owns(item))) batches.cancel(current)
}, { flush: 'sync' })
watch(() => props.active, active => { if (!active) { close(); selecting.value = false; selected.value = [] } })
onBeforeUnmount(() => { mounted = false; if (batch.value) batches.cancel(batch.value) })
function close() {
  if (batch.value && batch.value.phase !== 'result') batches.cancel(batch.value)
  open.value = false; permanent.value = false; subtree.value = false
}
function start() { selected.value = []; search.value = ''; selecting.value = true }
function selectAll() { selected.value = [...new Set([...selected.value, ...visible.value.filter(item => item.binding).map(item => item.row.id)])] }
async function review() {
  if (!props.active || locked.value || !bindings.value.length) return
  const created = batches.create(bindings.value)
  batchId.value = created.id; open.value = true; permanent.value = false; subtree.value = false
  await batches.prepare(created, owns)
}
function reopen(id: string) { batchId.value = id; open.value = true; permanent.value = false; subtree.value = false }
function inspect(item: NativeDeleteBatchItem) { if (batch.value) void batches.inspect(batch.value, item) }
</script>

<template>
  <section class="batch-delete-panel" :aria-label="t('deleteBatchTitle')">
    <div class="batch-toolbar">
      <AppButton v-if="!selecting" data-batch-start variant="danger" :disabled="!active || locked || !sessions.some(row => row.archived)" @click="start">{{ t('deleteBatchTitle') }}</AppButton>
      <template v-else>
        <AppButton data-batch-cancel-selection :disabled="locked" @click="selecting = false; selected = []">{{ t('deleteBatchCancelSelection') }}</AppButton>
        <AppButton data-batch-select-all :disabled="locked || !visible.some(item => item.binding)" @click="selectAll">{{ t('deleteBatchSelectVisible') }}</AppButton>
        <AppButton :disabled="locked || !selected.length" @click="selected = []">{{ t('deleteBatchClear') }}</AppButton>
      </template>
    </div>
    <template v-if="selecting">
      <p class="batch-hint">{{ t('deleteBatchEligibility') }}</p>
      <AppInput v-model="search" class="batch-search" :label="t('search')" type="search" :disabled="locked" />
      <div class="batch-selection">
        <label v-for="{ row, binding } in visible" :key="row.id" class="batch-choice">
          <input v-model="selected" data-batch-choice type="checkbox" :value="row.id" :disabled="!active || !binding || locked" />
          <span><strong>{{ row.title }}</strong><small>{{ row.cli }} · {{ row.projectPath }}</small><small v-if="!binding">{{ t('deleteBatchUnavailable') }}</small></span>
        </label>
      </div>
      <p aria-live="polite">{{ t('deleteBatchSelected', { count: bindings.length }) }}</p>
      <AppButton data-batch-review variant="danger" :disabled="!active || locked || !bindings.length" @click="review">{{ t('deleteBatchReview', { count: bindings.length }) }}</AppButton>
    </template>
    <div class="batch-ledger">
      <AppButton v-for="[id, entry] in batches.batches" :key="id" data-batch-existing :disabled="locked && entry.phase === 'result'" @click="reopen(id)">{{ t('deleteBatchView', { count: entry.items.length }) }} · {{ entry.items[0]?.binding.title }}</AppButton>
    </div>
    <AppDialog :open="open" :title="t(batch?.phase === 'confirmation' ? 'deleteSecondConfirmation' : 'deleteBatchTitle')" data-batch-dialog @close="close">
      <template v-if="batch">
        <p>{{ t('deleteBatchSelected', { count: batch.items.length }) }}</p>
        <p>{{ t('deleteBatchScope') }}</p>
        <p>{{ t('deleteNativeSubtreeScope') }}</p>
        <p>{{ t('deleteNativeMaintenance') }}</p>
        <InlineNotice kind="warning" :message="t('deleteIrreversible')" />
        <p v-if="batch.phase === 'previewing' || batch.phase === 'executing'" aria-live="polite">{{ t('deleteBusy') }}</p>
        <div v-if="batch.phase === 'result'" data-batch-result aria-live="polite">{{ t('deleteBatchSummary', summary) }}</div>
        <ol class="batch-targets">
          <li v-for="item in batch.items" :key="item.request.operationId">
            <strong>{{ item.binding.title }}</strong>
            <dl>
              <dt>{{ t('cleanupProjectLabel') }}</dt><dd>{{ item.binding.origin.projectPath }}</dd>
              <dt>{{ t('cleanupNativeIdLabel') }}</dt><dd>{{ item.binding.nativeSessionId }}</dd>
              <dt>{{ t('cleanupProfileLabel') }}</dt><dd>{{ item.request.source.profileId }} / {{ item.request.source.profileRevision }}</dd>
              <dt>{{ t('cleanupRootLabel') }}</dt><dd>{{ item.request.source.sourceRootKey }}</dd>
            </dl>
            <p v-if="item.state === 'skipped'">{{ t('deleteBatchNotSent') }}</p>
            <template v-else-if="item.state === 'result'">
              <p>{{ t(`deleteState_${batches.record(item)?.receipt?.state || 'blocked'}`) }}</p>
              <p>{{ t('deleteDescendantsUnverified') }}</p>
              <p v-if="batches.record(item)?.receipt?.rootPresent === false">{{ t('deleteRootAbsent') }}</p>
              <p v-if="batches.record(item)?.receipt?.rootPresent === true">{{ t('deleteRootPresent') }}</p>
              <code>{{ batches.record(item)?.error || batches.record(item)?.receipt?.reason || batches.record(item)?.preview?.reason }}</code>
              <AppButton v-if="batches.canInspect(item)" data-batch-inspect :disabled="batch.phase !== 'result' || batches.record(item)?.phase === 'checking'" @click="inspect(item)">{{ t('deleteInspect') }}</AppButton>
            </template>
          </li>
        </ol>
        <template v-if="batch.phase === 'confirmation'">
          <label class="batch-choice"><input v-model="permanent" data-batch-ack-permanent type="checkbox" />{{ t('deleteAcknowledgePermanent') }}</label>
          <label class="batch-choice"><input v-model="subtree" data-batch-ack-subtree type="checkbox" />{{ t('deleteBatchAcknowledgeScope', { count: batch.items.length }) }}</label>
        </template>
      </template>
      <template #footer>
        <AppButton data-batch-close @click="close">{{ t(batch?.phase === 'executing' ? 'deleteBatchStop' : 'close') }}</AppButton>
        <AppButton v-if="batch?.phase === 'review'" data-batch-next @click="batches.beginConfirmation(batch, owns)">{{ t('deleteContinueConfirmation') }}</AppButton>
        <AppButton v-if="batch?.phase === 'confirmation'" data-batch-final variant="danger" :disabled="!permanent || !subtree || batch.items.some(item => !owns(item))" @click="batches.execute(batch, permanent, subtree, owns)">{{ t('deleteBatchFinal', { count: batch.items.length }) }}</AppButton>
      </template>
    </AppDialog>
  </section>
</template>

<style scoped>
.batch-delete-panel { margin: 12px 0; font-size: 12px; }
.batch-toolbar, .batch-ledger { display: flex; flex-wrap: wrap; gap: 8px; }
.batch-ledger:empty { display: none; } .batch-ledger { margin-top: 8px; }
.batch-hint, small, dt { color: var(--text-secondary); }
.batch-search { margin: 12px 0; }
.batch-selection { max-height: 280px; overflow: auto; }
.batch-choice { display: flex; align-items: flex-start; gap: 8px; margin: 10px 0; line-height: 1.5; }
.batch-choice input { flex: none; margin-top: 3px; } .batch-choice span { min-width: 0; overflow-wrap: anywhere; }
small { display: block; } p { line-height: 1.6; }
.batch-targets { padding-left: 24px; }
.batch-targets li { margin: 14px 0; overflow-wrap: anywhere; }
dl { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 4px 10px; font-size: 12px; }
dd { margin: 0; overflow-wrap: anywhere; }
</style>
