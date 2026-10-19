<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import { historyRecoveryHint, workspaceSourceLabels, workspaceWarningKey, type WorkspaceHistoryIssue, type WorkspaceSourceConfiguration, type WorkspaceSourceWarning } from '@/utils/workspaceSourceWarnings'
withDefaults(defineProps<{ warnings: readonly WorkspaceSourceWarning[]; truncated: boolean; configurations?: readonly WorkspaceSourceConfiguration[]; issues?: readonly WorkspaceHistoryIssue[]; busy?: boolean; compact?: boolean; partial?: boolean }>(), { configurations: () => [], issues: () => [], busy: false, compact: false, partial: false })
const emit = defineEmits<{ configure: [profileId: string]; retry: [key: string]; openProject: [key: string] }>()
const { t } = useI18n()
</script>

<template>
  <details v-if="warnings.length" class="ui-error-details source-warning-details" data-workspace-source-details>
    <summary>{{ t(partial ? 'historyReadDiagnostics' : compact ? 'sourceWarningDiagnostics' : 'toggleDetails') }}</summary>
    <p v-if="partial">{{ t('historyReadFailureHint') }}</p>
    <ul>
      <li v-for="warning in warnings" :key="`${warning.source}:${warning.stage}:${warning.code}`">
        {{ t(workspaceSourceLabels[warning.source]) }}: <code>{{ warning.code }}<template v-if="warning.stage"> / {{ warning.stage }}</template></code>
        <p class="source-warning-hint">{{ t(historyRecoveryHint(warning.code)) }}</p>
        <p v-if="warning.source === 'claude-history' || warning.source === 'codex-history' || warning.source === 'opencode-history'">{{ t('historyIssueUnknownScope') }}</p>
        <div v-for="issue in issues.filter(row => row.warningKey === workspaceWarningKey(warning))" :key="issue.key" class="source-issue" data-history-issue :data-session-id="issue.sessionId">
          <dl>
            <dt>{{ t('sourceWarningConfigurations') }}</dt><dd>{{ issue.configurationName }}</dd>
            <dt>{{ t('historyIssueProject') }}</dt><dd>{{ issue.projectPath }}</dd>
            <template v-if="issue.sessionId">
              <dt>{{ t('historyIssueSession') }}</dt><dd><strong>{{ issue.sessionTitle }}</strong> <code>{{ issue.sessionId }}</code></dd>
            </template>
          </dl>
          <p>{{ t(issue.sessionId ? 'historyIssueKnownScope' : 'historyIssueUnknownScope') }}</p>
          <AppButton data-history-issue-retry variant="ghost" size="compact" :disabled="busy" @click="emit('retry', issue.key)">{{ t('historyIssueRescan') }}</AppButton>
          <AppButton data-history-issue-open-project variant="ghost" size="compact" :disabled="busy" @click="emit('openProject', issue.key)">{{ t('historyIssueOpenProject') }}</AppButton>
        </div>
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
.source-issue { border-top: 1px solid var(--border-color); margin-top: 8px; padding-top: 8px; }
.source-issue dl { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 4px 8px; }
.source-issue dd { margin: 0; overflow-wrap: anywhere; }
</style>
