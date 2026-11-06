<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import type { UnifiedSession } from '@/types/unifiedSession'

const props = defineProps<{ sessions: UnifiedSession[]; active: boolean }>()
const { t } = useI18n(), selectedId = ref('')
const archived = computed(() => props.sessions.filter(row => row.archived))
const target = computed(() => archived.value.find(row => row.id === selectedId.value))
const options = computed(() => [{ value: '', label: t('cleanupSelectTarget') }, ...archived.value.map(row => ({ value: row.id, label: `${row.cli} · ${row.title}` }))])
watch(() => [props.active, target.value] as const, ([active, current]) => { if (!active || !current) selectedId.value = '' }, { flush: 'sync' })
const protectedTarget = computed(() => target.value && (target.value.opened === true || target.value.resumePending
  || !!target.value.preparationState || target.value.processState === 'running' || target.value.processState === 'starting' || target.value.processState === 'unknown'))
const reason = computed(() => target.value?.cli === 'claude' ? 'cleanupClaudeUnsupported'
  : target.value?.cli === 'opencode' ? 'cleanupOpencodeUnsafe' : 'cleanupCodexUnsafe')
// 当前原生接口缺少预览集合与活动状态的原子条件。此组件仅解释能力，不发出 mutation。
</script>

<template>
  <details class="cleanup-availability" data-cleanup-availability>
    <summary>{{ t('cleanupAvailabilityTitle') }}</summary>
    <p>{{ t('cleanupDeskDistinction') }}</p>
    <AppSelect v-model="selectedId" :options="options" :label="t('cleanupTargetLabel')" :disabled="!active || !archived.length" />
    <div v-if="target && active" data-cleanup-target class="cleanup-target">
      <dl>
        <dt>{{ t('cleanupTargetLabel') }}</dt><dd>{{ target.title }} · {{ target.cli }}</dd>
        <dt>{{ t('cleanupProjectLabel') }}</dt><dd>{{ target.projectPath }}</dd>
        <dt>{{ t('cleanupNativeIdLabel') }}</dt><dd>{{ target.nativeSessionId || t('cleanupUnknown') }}</dd>
        <dt>{{ t('cleanupProfileLabel') }}</dt><dd>{{ target.nativeOrigin?.profileId || target.launchConfigId || t('cleanupUnknown') }} / {{ target.nativeOrigin?.profileRevision || t('cleanupUnknown') }}</dd>
        <dt>{{ t('cleanupRootLabel') }}</dt><dd>{{ target.nativeRootKey || t('cleanupUnknown') }}</dd>
      </dl>
      <p>{{ t('cleanupIncompletePreview') }}</p>
      <InlineNotice v-if="protectedTarget" kind="warning" :message="t('cleanupProtected')" />
      <InlineNotice kind="warning" :message="t(reason)" />
      <p>{{ t('cleanupRecoveryUnknown') }}</p>
      <div class="cleanup-actions">
        <AppButton data-native-cleanup-archive disabled>{{ t('cleanupNativeArchive') }}</AppButton>
        <AppButton data-native-cleanup-delete variant="danger" disabled>{{ t('cleanupNativeDelete') }}</AppButton>
      </div>
    </div>
  </details>
</template>

<style scoped>
.cleanup-availability { margin-top: 16px; padding-top: 12px; border-top: 1px solid var(--border-color); font-size: 12px; color: var(--text-secondary); }
summary { cursor: pointer; color: var(--text-primary); font-weight: 600; }
p { margin: 10px 0; line-height: 1.6; }
.cleanup-target { margin-top: 12px; } dl { display: grid; grid-template-columns: minmax(80px, auto) minmax(0, 1fr); gap: 6px 12px; line-height: 1.5; }
dt { color: var(--text-secondary); } dd { margin: 0; overflow-wrap: anywhere; color: var(--text-primary); }
.cleanup-actions { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 12px; }
</style>
