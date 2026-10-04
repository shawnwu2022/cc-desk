<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { workspaceSourceLabels, type WorkspaceSourceWarning } from '@/utils/workspaceSourceWarnings'
defineProps<{ warnings: readonly WorkspaceSourceWarning[]; truncated: boolean }>()
const { t } = useI18n()
</script>

<template>
  <details v-if="warnings.length" class="ui-error-details source-warning-details" data-workspace-source-details>
    <summary>{{ t('toggleDetails') }}</summary>
    <ul>
      <li v-for="warning in warnings" :key="`${warning.source}:${warning.stage}:${warning.code}`">
        {{ t(workspaceSourceLabels[warning.source]) }}: <code>{{ warning.code }}<template v-if="warning.stage"> / {{ warning.stage }}</template></code>
      </li>
    </ul>
    <p v-if="truncated">{{ t('sourceWarningMore') }}</p>
  </details>
</template>

<style scoped>
.source-warning-details { flex-basis: 100%; min-width: 0; margin-top: 0; }
.source-warning-details ul { margin: 8px 0 0; padding-left: 20px; }
.source-warning-details li { overflow-wrap: anywhere; }
.source-warning-details code { display: inline; padding: 0; }
</style>
