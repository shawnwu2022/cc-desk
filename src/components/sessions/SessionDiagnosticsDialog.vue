<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import AppDialog from '@/components/ui/AppDialog.vue'
import ErrorDetails from '@/components/ui/ErrorDetails.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import type { SessionDiagnostics } from '@/utils/sessionDiagnostics'

defineProps<{ diagnostics: SessionDiagnostics | null }>()
const emit = defineEmits<{ close: [] }>()
const { t } = useI18n()
</script>

<template>
  <AppDialog :open="!!diagnostics" :title="t('sessionActionDiagnostics')" data-session-diagnostics @close="emit('close')">
    <template v-if="diagnostics">
      <dl class="session-diagnostics-summary">
        <dt>{{ t('sessionDiagnosticsTool') }}</dt><dd>{{ diagnostics.cli === 'claude' ? 'Claude Code' : 'Codex CLI' }}</dd>
        <dt>{{ t('sessionDiagnosticsState') }}</dt><dd>{{ t(diagnostics.stateKey) }}</dd>
        <dt>{{ t('sessionDiagnosticsLocation') }}</dt><dd>{{ t(diagnostics.preparing ? 'sessionDiagnosticsPreparing' : diagnostics.open ? 'sessionDiagnosticsOpen' : 'sessionDiagnosticsHistory') }}</dd>
      </dl>
      <details>
        <summary>{{ t('toggleDetails') }}</summary>
        <dl class="session-diagnostics-summary">
          <dt>{{ t('sessionDiagnosticsRuntime') }}</dt><dd>{{ diagnostics.runtime }}</dd>
          <template v-if="diagnostics.generation !== null">
            <dt>{{ t('sessionDiagnosticsGeneration') }}</dt><dd>{{ diagnostics.generation }}</dd>
          </template>
        </dl>
      </details>
      <InlineNotice
        v-if="diagnostics.errorCode === 'NATIVE_INPUT_PAUSED'"
        kind="warning"
        :message="t('errorNativeInputPaused')"
      />
      <ErrorDetails v-if="diagnostics.errorCode" :code="diagnostics.errorCode" context="session" />
    </template>
  </AppDialog>
</template>

<style scoped>
.session-diagnostics-summary { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 2fr); gap: 8px 16px; }
dt { color: var(--text-secondary); }
dd { margin: 0; overflow-wrap: anywhere; }
summary { cursor: pointer; }
</style>
