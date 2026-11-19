<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import { diagnosticsCaptureControl } from '@/utils/diagnosticsCaptureControl'

const props = defineProps<{ active: boolean }>()
const { t } = useI18n()
const capture = diagnosticsCaptureControl.state
const snapshotPending = diagnosticsCaptureControl.snapshotPending
const enabled = computed(() => capture.value === 'on')
const preview = ref('')
const status = ref<'copied' | 'failed' | null>(null)
let owner = 0
function invalidate() { ++owner; preview.value = ''; status.value = null }
watch(() => props.active, current => { if (!current) invalidate() }, { flush: 'sync' })
onBeforeUnmount(invalidate)

function toggle() {
  if (!props.active) return
  if (capture.value === 'off') void diagnosticsCaptureControl.enable()
  else void diagnosticsCaptureControl.disable()
  invalidate()
}

async function showPreview() {
  if (!props.active) return
  const current = ++owner
  status.value = null
  const report = await diagnosticsCaptureControl.readReport()
  if (current === owner && props.active) preview.value = JSON.stringify(report, null, 2)
}

async function copyPreview() {
  if (!props.active || !preview.value) return
  const current = ++owner
  status.value = null
  try {
    await writeText(preview.value)
    if (current === owner) status.value = 'copied'
  } catch {
    if (current === owner) status.value = 'failed'
  }
}
</script>

<template>
  <div data-activity-diagnostics>
    <h3>{{ t('activityDiagnosticsTitle') }}</h3>
    <p>{{ t('activityDiagnosticsHint') }}</p>
    <AppButton :disabled="!active || capture === 'stopping'" :aria-pressed="enabled" @click="toggle">{{ t(capture !== 'off' ? 'activityDiagnosticsStop' : 'activityDiagnosticsStart') }}</AppButton>
    <p v-if="capture === 'starting'" role="status">{{ t('activityDiagnosticsStarting') }}</p>
    <p v-else-if="capture === 'stopping'" role="status">{{ t('activityDiagnosticsStopping') }}</p>
    <InlineNotice v-else-if="capture === 'unknown'" kind="warning" :message="t('activityDiagnosticsStateUnknown')" />
    <AppButton :disabled="!active || snapshotPending" variant="ghost" @click="showPreview">{{ t('activityDiagnosticsPreview') }}</AppButton>
    <template v-if="preview">
      <pre class="activity-preview" tabindex="0" :aria-label="t('activityDiagnosticsPreview')">{{ preview }}</pre>
      <AppButton :disabled="!active" @click="copyPreview">{{ t('activityDiagnosticsCopyPreview') }}</AppButton>
    </template>
    <InlineNotice v-if="status === 'failed'" kind="warning" :message="t('diagnosticsCopyFailed')" />
    <span v-else-if="status === 'copied'" role="status">{{ t('diagnosticsCopied') }}</span>
  </div>
</template>

<style scoped>
.activity-preview { max-height: 16rem; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
