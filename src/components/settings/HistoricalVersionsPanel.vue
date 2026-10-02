<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useVersionHistoryStore } from '@/stores/versionHistory'
import type { HistoryBlockReason, HistoryRelease } from '@/types/versionHistory'
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
const props = defineProps<{ active: boolean }>()
const { t, locale } = useI18n(), history = useVersionHistoryStore(), filter = ref('')
let owner = 0
watch(() => props.active, active => {
  if (active) { owner = history.activate(); filter.value = '' }
  else history.deactivate(owner)
}, { immediate: true, flush: 'sync' })
onBeforeUnmount(() => history.deactivate(owner))
const visibleRows = computed(() => history.rows.filter(row => row.version.includes(filter.value.trim())))
const disabled = computed(() => !props.active || history.busy || history.hasPreparation)
const reasons: Record<HistoryBlockReason, string> = {
  PLATFORM_UNSUPPORTED: 'historyPlatformUnsupported', PLATFORM_ASSET_MISSING: 'historyPlatformMissing',
  PACKAGE_FORMAT_UNSUPPORTED: 'historyFormatUnsupported', PACKAGING_BOUNDARY_UNKNOWN: 'historyPackagingUnknown',
  SIGNATURE_MISSING: 'historySignatureMissing', DIGEST_UNAVAILABLE: 'historyDigestMissing',
  ASSET_AMBIGUOUS: 'historyAssetAmbiguous', ASSET_METADATA_INVALID: 'historyAssetInvalid', RELEASE_NOT_HISTORICAL: 'historyNotHistorical',
}
const status = computed(() => ({ idle: 'historyAwaitingVerification', preparing: 'historyPreparing', verified: 'historyPublisherVerified',
  cancelling: 'historyCancelling', cancelled: 'historyCancelled', failed: 'historyPreparationFailed', 'cancel-failed': 'historyCancelUnconfirmed' })[history.phase])
function rowStatus(row: HistoryRelease) {
  if (row.blockedReason) return reasons[row.blockedReason]
  const selected = history.selected, prepared = history.prepared
  // Display only the current preparation of this exact observation/asset. The
  // catalogue stays unverified metadata, and this label grants no installation.
  return props.active && history.phase === 'verified' && selected?.releaseId === row.releaseId
    && selected.assetId === row.assetId && selected.version === row.version
    && prepared?.version === selected.version && prepared.verification === 'publisher-verified'
    ? 'historyPublisherVerifiedShort' : 'historyAwaitingVerification'
}
function date(value: string) { return new Date(value).toLocaleDateString(locale.value) }
</script>
<template>
  <section class="history-panel" data-history-panel :aria-label="t('historyTitle')">
    <div class="history-heading"><h3>{{ t('historyTitle') }}</h3>
      <AppButton data-history-refresh :disabled="disabled" :loading="history.loading" @click="history.list(owner)">{{ t('historyRefresh') }}</AppButton>
    </div>
    <p class="history-description">{{ t('historyDescription') }}</p>
    <InlineNotice v-if="history.error" data-history-error kind="warning" :message="t(history.error)" />
    <label v-if="history.rows.length" class="history-filter">{{ t('historyFilter') }}
      <input v-model="filter" data-history-filter type="search" :disabled="!active" :placeholder="t('historyFilterPlaceholder')" />
    </label>
    <p v-if="history.loaded && !history.rows.length" data-history-empty>{{ t('historyEmpty') }}</p>
    <p v-else-if="history.loaded && !visibleRows.length">{{ t('historyNoMatch') }}</p>
    <ul v-if="visibleRows.length" class="history-list" :aria-label="t('historyVersions')" :aria-busy="history.loading || history.selecting">
      <li v-for="row in visibleRows" :key="row.releaseId" data-history-row>
        <div class="history-release">
          <strong>v{{ row.version }}</strong><time :datetime="row.publishedAt">{{ date(row.publishedAt) }}</time>
          <span class="history-reason">{{ t(rowStatus(row)) }}</span>
        </div>
        <AppButton data-history-select :aria-label="t('historySelectVersion', { version: row.version })"
          :aria-pressed="history.selected?.releaseId === row.releaseId" :disabled="disabled || !row.selectAllowed"
          @click="history.select(owner, row)">{{ history.selected?.releaseId === row.releaseId ? t('historySelected') : t('historySelect') }}</AppButton>
      </li>
    </ul>
    <AppButton v-if="history.nextCursor" data-history-more :disabled="disabled" @click="history.list(owner, true)">{{ t('historyLoadMore') }}</AppButton>
    <InlineNotice v-if="history.truncated" data-history-truncated :message="t('historyTruncated')" />
    <section v-if="history.selected || history.hasPreparation || ['cancelled', 'failed'].includes(history.phase)" class="history-preparation" :aria-label="t('historyPreparation')">
      <strong v-if="history.selected" data-history-selected>{{ t('historySelectedVersion', { version: history.selected.version }) }}</strong>
      <p data-history-status role="status" aria-live="polite">{{ t(status) }}</p>
      <p>{{ t('historyFreshSettings') }}</p>
      <p>{{ t('historyKeepDataUnavailable') }}</p>
      <InlineNotice kind="warning" :message="t('historySharedDataWarning')" />
      <p class="history-description">{{ t('historyReturnPlan') }}</p>
      <div class="history-actions">
        <AppButton v-if="history.selected && !history.hasPreparation" data-history-prepare :disabled="!active || history.busy" @click="history.prepare(owner)">{{ t('historyPrepare') }}</AppButton>
        <AppButton v-if="history.hasPreparation" data-history-cancel :disabled="!active || history.phase === 'cancelling' || history.error === 'historyErrorDocument'" @click="history.cancel(owner)">{{ t(history.phase === 'cancel-failed' ? 'historyRetryCancel' : 'historyCancelPreparation') }}</AppButton>
      </div>
    </section>
    <InlineNotice data-history-install-unavailable :message="t('historyInstallUnavailable')" />
    <AppButton data-history-install disabled>{{ t('historyInstall') }}</AppButton>
  </section>
</template>
<style scoped>
.history-panel { display: flex; flex-direction: column; align-items: stretch; gap: 12px; padding-top: 20px; border-top: 1px solid var(--border-color); min-width: 0; }
.history-heading, .history-actions { display: flex; align-items: center; flex-wrap: wrap; gap: 12px; }
.history-heading { justify-content: space-between; } h3 { margin: 0; font-size: 16px; }
p { margin: 0; line-height: 1.6; font-size: 13px; overflow-wrap: anywhere; }
.history-description, .history-reason, time { color: var(--text-secondary); }
.history-filter { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; font-size: 13px; }
input { min-width: 0; max-width: 100%; padding: 8px 10px; color: var(--text-primary); background: var(--bg-primary); border: 1px solid var(--border-color); border-radius: 4px; }
input:focus-visible { outline: 2px solid var(--accent-color); outline-offset: 2px; }
.history-list { list-style: none; display: flex; flex-direction: column; padding: 0; margin: 0; }
li { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 12px 0; border-bottom: 1px solid var(--border-color); }
.history-release { min-width: 0; display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px 12px; font-size: 13px; }
.history-reason { flex-basis: 100%; overflow-wrap: anywhere; }
.history-preparation { display: flex; flex-direction: column; gap: 10px; padding: 14px; border: 1px solid var(--border-color); border-radius: 6px; min-width: 0; }
</style>
