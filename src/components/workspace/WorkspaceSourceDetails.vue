<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import { workspaceSourceLabels, workspaceWarningKey, type WorkspaceSourceConfiguration, type WorkspaceSourceWarning } from '@/utils/workspaceSourceWarnings'
withDefaults(defineProps<{ warnings: readonly WorkspaceSourceWarning[]; truncated: boolean; configurations?: readonly WorkspaceSourceConfiguration[]; compact?: boolean; partial?: boolean }>(), { configurations: () => [], compact: false, partial: false })
const emit = defineEmits<{ configure: [profileId: string] }>()
const { t } = useI18n()
</script>

<template>
  <details v-if="warnings.length" class="ui-error-details source-warning-details" data-workspace-source-details>
    <summary>{{ t(partial ? 'historyReadDiagnostics' : compact ? 'sourceWarningDiagnostics' : 'toggleDetails') }}</summary>
    <p v-if="partial">{{ t('historyReadFailureHint') }}</p>
    <ul>
      <li v-for="warning in warnings" :key="`${warning.source}:${warning.stage}:${warning.code}`">
        {{ t(workspaceSourceLabels[warning.source]) }}: <code>{{ warning.code }}<template v-if="warning.stage"> / {{ warning.stage }}</template></code>
        <p v-if="warning.code === 'SOURCE_BUDGET_EXCEEDED'" class="source-warning-hint">{{ t('sourceWarningBudget') }}</p>
        <p v-if="warning.code === 'SCOPE_UNKNOWN' && warning.stage === 'scope-profile-validation'" class="source-warning-hint">{{ t('sourceWarningScopeUnknown') }}</p>
        <div v-for="configuration in configurations.filter(row => row.warningKey === workspaceWarningKey(warning))" :key="configuration.profileId" class="source-warning-configuration">
          <span>{{ t('sourceWarningConfiguration', { name: configuration.name }) }}</span>
          <AppButton data-source-warning-configuration variant="ghost" size="compact" @click="emit('configure', configuration.profileId)">{{ t('launchConfigEditAction') }}</AppButton>
        </div>
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
.source-warning-hint { margin: 6px 0; }
.source-warning-configuration { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
.source-warning-configuration span { overflow-wrap: anywhere; min-width: 0; }
</style>
