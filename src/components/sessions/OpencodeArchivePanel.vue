<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import AppDialog from '@/components/ui/AppDialog.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import { createNativeOpencodeArchiveClient } from '@/api/tauri'
import { opencodeArchiveErrorCode } from '@/api/opencodeArchive'
import { useNativeHistoryStore, nativeHistoryContextKey, type NativeHistoryContext } from '@/stores/nativeHistory'
import { useOpencodeArchiveStore } from '@/stores/opencodeArchive'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { opencodeArchiveBinding } from '@/utils/opencodeArchive'
import { createNativeUuid } from '@/utils/nativeId'
import { normalizePath } from '@/utils/path'
import type { UnifiedSession } from '@/types/unifiedSession'

const props = defineProps<{ sessions: UnifiedSession[]; active: boolean; projectPath?: string }>()
const { t } = useI18n(), history = useNativeHistoryStore(), operations = useOpencodeArchiveStore()
const open = ref(false), busy = ref(false), error = ref(''), selected = ref(''), acknowledged = ref(false), verified = ref(false)
const inProject = (path: string) => !props.projectPath || normalizePath(path) === normalizePath(props.projectPath)
const eligible = computed(() => props.sessions.filter(row => inProject(row.projectPath))
  .map(row => opencodeArchiveBinding(row, row.nativeOrigin ? history.get(row.nativeOrigin) : undefined))
  .filter(row => row !== null).filter(row => !operations.has(row)))
const choices = computed(() => [{ value: '', label: t('opencodeArchiveChoose') }, ...eligible.value.map(row => ({ value: row.rowId, label: `${row.title} · ${row.nativeSessionId}` }))])
const target = computed(() => eligible.value.find(row => row.rowId === selected.value))
const signature = computed(() => JSON.stringify(target.value ?? null))
const pending = computed(() => [...operations.pending.values()].filter(item => inProject(item.binding.origin.projectPath)))
watch(signature, () => { acknowledged.value = false }, { flush: 'sync' })
watch(() => props.active, active => { if (!active) { open.value = false; selected.value = ''; acknowledged.value = false } })
function show() {
  if (!props.active || busy.value) return
  open.value = true; selected.value = ''; acknowledged.value = false; error.value = ''; verified.value = false
}
function showFor(rowId: string) {
  if (!props.active || busy.value) return
  show()
  selected.value = rowId
}
defineExpose({ showFor })
async function refreshContexts(contexts: NativeHistoryContext[]) {
  for (const context of new Map(contexts.map(value => [nativeHistoryContextKey(value), value])).values()) {
    try {
      const entry = await history.load({ ...context, force: true, retainOnFailure: true })
      operations.observe(entry)
    } finally { await useUnifiedSessionsStore().refresh(context.projectPath, { cli: 'opencode' }) }
  }
}
async function refresh() {
  if (!props.active || !open.value || busy.value) return
  busy.value = true; error.value = ''; selected.value = ''; acknowledged.value = false
  try {
    await refreshContexts([...history.all().filter(entry => entry.context.cli === 'opencode' && inProject(entry.context.projectPath)).map(entry => entry.context),
      ...pending.value.map(item => item.binding.origin)])
  } catch (failure) { error.value = opencodeArchiveErrorCode(failure) } finally { busy.value = false }
}
async function submit() {
  const binding = target.value
  if (!props.active || !open.value || busy.value || !acknowledged.value || !binding) return
  busy.value = true; error.value = ''; verified.value = false; selected.value = ''; acknowledged.value = false
  const operationId = createNativeUuid(), archived = !binding.nativeArchived
  try {
    operations.begin(binding, operationId, archived)
    try {
      await createNativeOpencodeArchiveClient().set({ operationId, source: binding.source, sessionKey: binding.sessionKey, archived, acknowledgeRecoverable: true })
    } catch (failure) {
      operations.failed(binding, opencodeArchiveErrorCode(failure))
      throw failure
    } finally { operations.settled(binding) }
    await refreshContexts([binding.origin])
    verified.value = !operations.has(binding)
  } catch (failure) { error.value = opencodeArchiveErrorCode(failure) } finally { busy.value = false }
}
</script>
<template>
  <AppButton data-opencode-archive-open :disabled="!active || busy" @click="show">{{ t('opencodeArchiveOpen') }}</AppButton>
  <AppDialog :open="open" :title="t('opencodeArchiveOpen')" data-opencode-archive-dialog @close="open = false">
    <p>{{ t('opencodeArchiveScope') }}</p>
    <InlineNotice kind="warning" :message="t('opencodeArchiveLimitations')" />
    <p v-if="error" data-opencode-archive-error role="alert">{{ error }}</p>
    <p v-if="verified" aria-live="polite">{{ t('opencodeArchiveVerified') }}</p>
    <p v-for="item in pending" :key="item.operationId" data-opencode-archive-pending>{{ item.binding.nativeSessionId }} · {{ t(item.archived ? 'opencodeArchiveSet' : 'opencodeArchiveRestore') }} · {{ t('opencodeArchivePending') }}</p>
    <p v-if="busy" aria-live="polite">{{ t('deleteBusy') }}</p>
    <AppSelect v-model="selected" data-opencode-archive-select :label="t('opencodeArchiveChoose')" :options="choices" :disabled="busy" />
    <template v-if="target">
      <p>{{ target.nativeSessionId }} · {{ t(target.nativeArchived ? 'opencodeArchiveArchived' : 'opencodeArchiveUnarchived') }}</p>
      <label><input v-model="acknowledged" data-opencode-archive-ack type="checkbox" :disabled="busy">{{ t('opencodeArchiveAcknowledge') }}</label>
      <AppButton data-opencode-archive-submit :disabled="busy || !acknowledged || !active" @click="submit">{{ t(target.nativeArchived ? 'opencodeArchiveRestore' : 'opencodeArchiveSet') }}</AppButton>
    </template>
    <template #footer>
      <AppButton data-opencode-archive-refresh :disabled="busy || !active" @click="refresh">{{ t('opencodeArchiveRefresh') }}</AppButton>
      <AppButton @click="open = false">{{ t('close') }}</AppButton>
    </template>
  </AppDialog>
</template>
<style scoped>
p { line-height: 1.6; overflow-wrap: anywhere; }
label { display: flex; align-items: flex-start; gap: 8px; margin: 12px 0; }
</style>
