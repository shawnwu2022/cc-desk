<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { open } from '@tauri-apps/plugin-shell'
import { useI18n } from 'vue-i18n'
import { checkForUpdates } from '@/api/tauri'
import { useSidebarStore } from '@/stores/sidebar'
import { useUpdateStore } from '@/stores/update'
import { useOwnedSessionCounts } from '@/composables/useOwnedSessionCounts'
import AppButton from '@/components/ui/AppButton.vue'
import AppDialog from '@/components/ui/AppDialog.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import HistoricalVersionsPanel from '@/components/settings/HistoricalVersionsPanel.vue'
const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })
const { t } = useI18n(), sidebar = useSidebarStore(), updates = useUpdateStore()
const counts = useOwnedSessionCounts(), checking = ref(false), error = ref<string | null>(null), reviewing = ref(false)
const currentVersion = __APP_VERSION__
const channel = computed(() => ['candidate', 'test-only'].includes(updates.updateInfo?.channel ?? '') ? updates.updateInfo!.channel! : 'unverified')
const channelLabels = { stable: 'updateStable', candidate: 'updateCandidate', 'test-only': 'updateTestOnly', unverified: 'updateUnverified' }
let owner = 0
function invalidate() { ++owner; checking.value = false; reviewing.value = false; error.value = null }
watch(() => props.active, active => { if (!active) invalidate() }, { flush: 'sync' }); onBeforeUnmount(invalidate)
async function checkUpdate() {
  if (!props.active || checking.value) return
  const version = ++owner; checking.value = true; error.value = null
  try {
    const info = await checkForUpdates()
    if (owner !== version || !props.active) return
    updates.setUpdateInfo(info); sidebar.setUpdateInfo(info)
  } catch { if (owner === version && props.active) error.value = 'updateCheckSafeFailed' }
  finally { if (owner === version) checking.value = false }
}
function reviewInstall() { if (props.active && updates.updateInfo?.hasUpdate) reviewing.value = true }
async function manualDownload() {
  if (!props.active) return
  const version = owner
  try { await open('https://github.com/shawnwu2022/cc-desk/releases') }
  catch { if (version === owner && props.active) error.value = 'settingsExternalLinkFailed' }
}
</script>
<template>
  <section class="remaining-settings" data-settings-update>
    <h2>{{ t('ccDeskUpdate') }}</h2>
    <div class="version-row"><span>CC Desk v{{ currentVersion }}</span><AppButton data-update-check :disabled="checking || !active" @click="checkUpdate">{{ checking ? t('checking') : t('checkForUpdates') }}</AppButton></div>
    <div class="channel-legend"><span>{{ t('updateStable') }}</span><span>{{ t('updateCandidate') }}</span><span>{{ t('updateTestOnly') }}</span></div>
    <InlineNotice data-update-policy kind="info" :message="t('updatePublicationBlocked')" />
    <InlineNotice v-if="error" data-update-error kind="warning" :message="t(error)" />
    <p v-if="updates.updateInfo && !updates.updateInfo.hasUpdate">{{ t('upToDate') }}</p>
    <div v-if="updates.updateInfo?.hasUpdate" class="package-summary" data-update-package>
      <strong>{{ t('updateObservedPackage', { version: updates.updateInfo.version }) }}</strong>
      <span data-update-channel>{{ t(channelLabels[channel]) }}</span>
      <p v-if="updates.updateInfo.releaseNotes" class="release-notes">{{ updates.updateInfo.releaseNotes }}</p>
      <AppButton data-update-review :disabled="!active" @click="reviewInstall">{{ t('updateReviewInstallation') }}</AppButton>
    </div>
    <div class="actions"><AppButton data-update-install disabled>{{ t('updateRestartInstall') }}</AppButton><AppButton variant="ghost" @click="manualDownload">{{ t('manualDownload') }}</AppButton></div>
    <HistoricalVersionsPanel :active="active" />
    <AppDialog :open="reviewing && active" data-update-confirmation :title="t('updateReviewInstallation')" @close="reviewing = false">
      <p data-update-running-count>{{ t('updateRunningCount', { count: counts.running }) }}</p>
      <p data-update-starting-count>{{ t('updateStartingCount', { count: counts.starting }) }}</p>
      <p data-update-unknown-count>{{ t('updateUnknownCount', { count: counts.unknown }) }}</p>
      <InlineNotice kind="warning" :message="t('updateInstallImpact')" />
      <InlineNotice :message="t('updatePublicationBlocked')" />
      <template #footer><AppButton @click="reviewing = false">{{ t('cancel') }}</AppButton><AppButton data-danger="true" disabled>{{ t('updateRestartInstall') }}</AppButton></template>
    </AppDialog>
  </section>
</template>
<style scoped>
.remaining-settings { display: flex; flex-direction: column; gap: 16px; min-width: 0; max-width: 760px; color: var(--text-primary); }
h2 { font-size: 20px; } .version-row, .actions, .channel-legend { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
.channel-legend { color: var(--text-secondary); font-size: 12px; } .channel-legend span { padding: 4px 8px; border: 1px solid var(--border-color); border-radius: 4px; }
.package-summary { display: flex; flex-direction: column; align-items: flex-start; gap: 10px; min-width: 0; }
.release-notes { white-space: pre-wrap; overflow-wrap: anywhere; color: var(--text-secondary); font-size: 13px; line-height: 1.6; }
</style>
