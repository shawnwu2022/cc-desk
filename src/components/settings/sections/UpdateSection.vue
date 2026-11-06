<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-shell'
import { useI18n } from 'vue-i18n'
import { getUpdaterSettings, saveUpdaterSettings, saveUpdaterChannel, cancelDesktopUpdate, testUpdaterProxy, installDesktopUpdate } from '@/api/tauri'
import { isOrdinaryUpdateEligible, isReleaseChannel } from '@/utils/updatePolicy'
import { updateFailure } from '@/utils/updateErrors'
import { releaseNotesForLocale } from '@/utils/releaseNotes'
import { useUpdateStore } from '@/stores/update'
import { useOwnedSessionCounts } from '@/composables/useOwnedSessionCounts'
import AppButton from '@/components/ui/AppButton.vue'
import AppInput from '@/components/ui/AppInput.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppDialog from '@/components/ui/AppDialog.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import HistoricalVersionsPanel from '@/components/settings/HistoricalVersionsPanel.vue'
const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })
const { t, locale } = useI18n(), updates = useUpdateStore()
const releaseNotes = computed(() => releaseNotesForLocale(updates.updateInfo, locale.value))
const counts = useOwnedSessionCounts(), checking = computed(() => updates.checking), error = ref<ReturnType<typeof updateFailure> | null>(null), reviewing = ref(false), recoveryGuide = ref(false)
const proxy = ref(''), savedProxy = ref(''), savingProxy = ref(false), loadingProxy = ref(true)
const settingsUncertain = computed(() => updates.settingsUncertain)
const savingChannel = ref(false), cancelling = ref(false), cancellation = ref<'requested' | 'too-late' | null>(null)
const channelOptions = computed(() => [
  { value: 'stable', label: t('updateChannelStable') },
  { value: 'test', label: t('updateChannelTest') },
  { value: 'preview', label: t('updateChannelPreview') },
])
const proxyUsername = ref(''), proxyPassword = ref(''), testingProxy = ref(false)
const originalUserinfo = ref('')
const proxyResult = ref<{ state: 'testing' | 'success' | 'failure'; elapsedMs?: number; failure?: ReturnType<typeof updateFailure> } | null>(null)
function decodeCredential(value: string) { try { return decodeURIComponent(value) } catch { return value } }
function setProxyAddress(value: string) {
  // Split userinfo immediately, including pasted URLs. Only the endpoint is a text input.
  value = value.trim()
  const match = value.match(/^([a-z][a-z\d+.-]*:\/\/)?([^?#]+)@([^/?#]*)(.*)$/i)
  if (match) {
    originalUserinfo.value = match[2]
    const separator = match[2].indexOf(':')
    proxyUsername.value = decodeCredential(separator < 0 ? match[2] : match[2].slice(0, separator))
    proxyPassword.value = decodeCredential(separator < 0 ? '' : match[2].slice(separator + 1))
    proxy.value = (match[1] ?? '') + match[3] + match[4]
  } else {
    proxy.value = value
    if (!value.trim()) { proxyUsername.value = ''; proxyPassword.value = ''; originalUserinfo.value = '' }
  }
}
const proxyAddress = computed({ get: () => proxy.value, set: setProxyAddress })
const proxyValue = computed(() => {
  const address = proxy.value.trim()
  if (!proxyUsername.value && !proxyPassword.value) return address
  const separator = originalUserinfo.value.indexOf(':')
  const originalUsername = separator < 0 ? originalUserinfo.value : originalUserinfo.value.slice(0, separator)
  const originalPassword = separator < 0 ? '' : originalUserinfo.value.slice(separator + 1)
  // Preserve each unchanged credential's encoded bytes, including non-UTF8 userinfo.
  const username = proxyUsername.value === decodeCredential(originalUsername) ? originalUsername : encodeURIComponent(proxyUsername.value)
  const password = proxyPassword.value === decodeCredential(originalPassword) ? originalPassword : encodeURIComponent(proxyPassword.value)
  const credentials = username + (separator >= 0 || password ? ':' + password : '') + '@'
  return /^[a-z][a-z\d+.-]*:\/\//i.test(address)
    ? address.replace(/^([a-z][a-z\d+.-]*:\/\/)/i, prefix => prefix + credentials) : credentials + address
})
const currentVersion = __APP_VERSION__
const installing = computed(() => updates.busy)
const settingsBlocked = computed(() => settingsUncertain.value || loadingProxy.value || checking.value || savingProxy.value || savingChannel.value || installing.value || testingProxy.value)
const visibleFailure = computed(() => error.value ?? (['error', 'cancelled'].includes(updates.downloadState) ? updates.lastFailure : null))
const canCancel = computed(() => ['downloading', 'verifying'].includes(updates.downloadState) && cancellation.value !== 'requested' && cancellation.value !== 'too-late')
const eligible = computed(() => !settingsUncertain.value && !updates.isCandidateIgnored && isOrdinaryUpdateEligible(updates.updateInfo))
const eligibilityHint = computed(() => updates.availableVersion !== null && updates.updateInfo?.version === updates.availableVersion
  ? updates.isCandidateIgnored ? 'updateCandidateIgnoredHint' : 'updateCheckBeforeInstall' : 'updateSourceUnverified')
const sessionsBusy = computed(() => counts.value.running + counts.value.starting + counts.value.unknown > 0)
const channel = computed(() => updates.updateInfo?.channel ?? 'unverified')
const channelLabels = { stable: 'updateStable', beta: 'updateBeta', rc: 'updateRc', preview: 'updatePreview', candidate: 'updateCandidate', 'test-only': 'updateTestOnly', unverified: 'updateUnverified' }
const progressKey = computed(() => ({ idle: 'updateChecking', downloading: 'updateDownloading', verifying: 'updateVerifying', installing: 'updateInstalling', restarting: 'updateRestarting', cancelled: 'updateCancelled', error: 'updateCheckSafeFailed' })[updates.downloadState])
let owner = 0, proxyOwner = 0, cancelOwner = 0, disposed = false, unlisten: UnlistenFn | undefined
function invalidateProxy() { ++proxyOwner; proxyResult.value = null }
function invalidate() { ++owner; reviewing.value = false; recoveryGuide.value = false; error.value = null; invalidateProxy() }
watch([proxy, proxyUsername, proxyPassword, originalUserinfo], invalidateProxy, { flush: 'sync' })
watch(() => props.active, active => { if (!active) invalidate() }, { flush: 'sync' })
onBeforeUnmount(() => { disposed = true; unlisten?.(); invalidate() })
async function reloadSettings() {
  if (disposed) return
  if (installing.value || savingProxy.value || savingChannel.value || testingProxy.value) { loadingProxy.value = false; return }
  const version = owner
  loadingProxy.value = true
  try {
    const settings = await getUpdaterSettings()
    if (!disposed) {
      setProxyAddress(settings.proxy ?? ''); savedProxy.value = proxyValue.value
      updates.setSelectedChannel(isReleaseChannel(settings.channel) ? settings.channel : 'stable')
      updates.setSettingsUncertain(false)
      error.value = null
    }
  } catch (cause) {
    if (!disposed) { updates.setSettingsUncertain(true); updates.recordFailure(cause) }
    if (!disposed && version === owner) error.value = updateFailure(cause)
  }
  finally { loadingProxy.value = false }
}
watch(installing, (busy, previous) => { if (previous && !busy && !disposed) void reloadSettings() })
onMounted(async () => {
  await reloadSettings()
  try {
    const stop = await listen<{ admissionId: string; phase: string; downloaded: number; total: number | null }>('desktop-update-progress', ({ payload }) => {
      if (disposed || !installing.value || payload.admissionId !== updates.updateInfo?.admissionId) return
      const phases = ['downloading', 'verifying', 'installing', 'restarting'] as const
      const phase = phases.find(value => value === payload.phase)
      if (phase && phases.indexOf(phase) >= phases.findIndex(value => value === updates.downloadState)) updates.setDownloadState(phase)
      if (Number.isSafeInteger(payload.downloaded) && Number.isSafeInteger(payload.total) && payload.total! > 0 && payload.downloaded >= 0 && payload.downloaded <= payload.total!) {
        updates.setDownloadProgress({ downloaded: payload.downloaded, total: payload.total!, percent: Math.floor(100 * payload.downloaded / payload.total!) })
      }
    })
    if (disposed) stop(); else unlisten = stop
  } catch { /* Progress display is optional; installation remains owned by the host. */ }
})
async function saveProxy() {
  if (!props.active || settingsBlocked.value) return
  const version = owner, value = proxyValue.value
  savingProxy.value = true; error.value = null
  const resumeChecks = updates.suspendChecks()
  updates.setUpdateInfo(null)
  try {
    await saveUpdaterSettings(value || null)
    if (!disposed && owner === version) savedProxy.value = value
  } catch (cause) { updates.setSettingsUncertain(true); updates.recordFailure(cause); if (!disposed && owner === version && props.active) error.value = updateFailure(cause) }
  finally { savingProxy.value = false; resumeChecks() }
}
async function changeChannel(value: string) {
  if (!isReleaseChannel(value) || value === updates.selectedChannel || !props.active || settingsBlocked.value) return
  const version = owner
  savingChannel.value = true; error.value = null; reviewing.value = false
  const resumeChecks = updates.suspendChecks()
  updates.setUpdateInfo(null)
  try {
    await saveUpdaterChannel(value)
    updates.setSelectedChannel(value)
  } catch (cause) {
    updates.setSettingsUncertain(true)
    updates.recordFailure(cause)
    if (!disposed && owner === version && props.active) error.value = updateFailure(cause)
  } finally { savingChannel.value = false; resumeChecks() }
}
async function testProxy() {
  if (!props.active || settingsBlocked.value) return
  const version = ++proxyOwner, value = proxyValue.value, started = performance.now()
  testingProxy.value = true; proxyResult.value = { state: 'testing' }
  try {
    const result = await testUpdaterProxy(value || null)
    if (!disposed && props.active && version === proxyOwner) {
      proxyResult.value = { state: 'success', elapsedMs: Number.isSafeInteger(result.elapsedMs) && result.elapsedMs >= 0 ? result.elapsedMs : Math.round(performance.now() - started) }
    }
  } catch (cause) {
    if (!disposed && props.active && version === proxyOwner) proxyResult.value = { state: 'failure', elapsedMs: Math.round(performance.now() - started), failure: updateFailure(cause) }
  } finally { testingProxy.value = false }
}
async function checkUpdate() {
  if (!props.active || settingsBlocked.value) return
  const version = ++owner; error.value = null
  try {
    await updates.checkForUpdate(() => !disposed && owner === version && props.active)
  } catch (cause) { if (owner === version && props.active) error.value = updateFailure(cause) }
}
function reviewInstall() { if (props.active && !installing.value && !updates.isCandidateIgnored && updates.updateInfo?.hasUpdate) reviewing.value = true }
async function installUpdate() {
  const info = updates.updateInfo, version = owner
  if (!props.active || settingsUncertain.value || updates.isCandidateIgnored || checking.value || savingProxy.value || savingChannel.value || installing.value || !isOrdinaryUpdateEligible(info) || !info?.admissionId || sessionsBusy.value) return
  reviewing.value = false
  ++cancelOwner; cancelling.value = false; cancellation.value = null
  updates.resetDownload(); updates.setDownloadState('downloading'); error.value = null
  try { await installDesktopUpdate(info.admissionId); if (updates.downloadState !== 'restarting') updates.setDownloadState('installing') }
  catch (cause) {
    const failure = updateFailure(cause)
    updates.recordFailure(cause)
    updates.setDownloadState(failure.code === 'UPDATER_CANCELLED' ? 'cancelled' : 'error')
    if (updates.updateInfo?.admissionId === info.admissionId) {
      updates.revokeInstallAdmission()
    }
    if (!disposed && owner === version && props.active) error.value = updateFailure(cause)
  }
}
async function cancelUpdate() {
  const id = updates.updateInfo?.admissionId
  if (!id || !props.active || !canCancel.value || cancelling.value) return
  const version = ++cancelOwner
  const current = () => !disposed && version === cancelOwner && updates.updateInfo?.admissionId === id && installing.value
  cancelling.value = true
  try { const receipt = await cancelDesktopUpdate(id); if (current()) cancellation.value = receipt.status }
  catch (cause) { if (current()) { updates.recordFailure(cause); if (props.active) error.value = updateFailure(cause) } }
  finally { if (version === cancelOwner) cancelling.value = false }
}
async function manualDownload() {
  if (!props.active) return
  const version = owner
  try { await open('https://github.com/shawnwu2022/cc-desk/releases') }
  catch { if (version === owner && props.active) error.value = { code: 'UPDATER_FAILED', stage: 'unknown', key: 'settingsExternalLinkFailed' } }
}
function ignoreCandidate() { if (props.active && !settingsBlocked.value && updates.ignoreCurrentCandidate()) reviewing.value = false }
function clearIgnoredCandidate() { if (props.active && !settingsBlocked.value) updates.clearIgnoredCandidate() }
function historyTime(at: number) { return new Date(at).toLocaleString(locale.value === 'zh' ? 'zh-CN' : 'en-US') }
</script>
<template>
  <section class="remaining-settings" data-settings-update>
    <h2>{{ t('ccDeskUpdate') }}</h2>
    <div class="version-row"><span>CC Desk v{{ currentVersion }}</span><AppButton data-update-check :disabled="settingsBlocked || !active" @click="checkUpdate">{{ checking ? t('checking') : t('checkForUpdates') }}</AppButton></div>
    <AppSelect data-update-channel-select :model-value="updates.selectedChannel" :options="channelOptions" :label="t('updateChannelLabel')" :disabled="settingsBlocked || !active" @update:model-value="changeChannel" />
    <AppButton v-if="settingsUncertain" data-update-reload-settings :disabled="loadingProxy || installing || !active" @click="reloadSettings">{{ t('updateReloadSettings') }}</AppButton>
    <InlineNotice data-update-channel-risk kind="info" :message="t(updates.selectedChannel === 'stable' ? 'updateChannelStableHint' : updates.selectedChannel === 'test' ? 'updateChannelTestHint' : 'updateChannelPreviewHint')" />
    <div class="proxy-settings">
      <AppInput v-model="proxyAddress" data-update-proxy type="text" autocomplete="off" :label="t('updateProxyLabel')" :disabled="loadingProxy || checking || savingProxy || installing || !active" />
      <div class="proxy-credentials">
        <AppInput v-model="proxyUsername" data-update-proxy-username type="password" autocomplete="off" :label="t('updateProxyUsername')" :disabled="loadingProxy || checking || savingProxy || installing || !active" />
        <AppInput v-model="proxyPassword" data-update-proxy-password type="password" autocomplete="new-password" :label="t('updateProxyPassword')" :disabled="loadingProxy || checking || savingProxy || installing || !active" />
      </div>
      <p>{{ t('updateProxyHint') }}</p>
      <div class="actions">
        <AppButton data-update-proxy-save :disabled="settingsBlocked || !active || proxyValue === savedProxy" @click="saveProxy">{{ t('updateProxySave') }}</AppButton>
        <AppButton data-update-proxy-test :disabled="settingsBlocked || !active" @click="testProxy">{{ testingProxy ? t('updateProxyTesting') : t('updateProxyTest') }}</AppButton>
      </div>
      <p v-if="proxyResult" data-update-proxy-result role="status">
        <template v-if="proxyResult.state === 'testing'">{{ t('updateProxyTesting') }}</template>
        <template v-else-if="proxyResult.state === 'success'">{{ t('updateProxyTestSuccess', { elapsed: proxyResult.elapsedMs }) }}</template>
        <template v-else>{{ t('updateProxyTestFailure', { elapsed: proxyResult.elapsedMs }) }} {{ t(proxyResult.failure!.key) }} ({{ proxyResult.failure!.code }})</template>
      </p>
    </div>
    <InlineNotice data-update-policy kind="info" :message="t('updatePublicationBlocked')" />
    <InlineNotice v-if="visibleFailure" data-update-error kind="warning" :message="t(visibleFailure.key) + ' (' + visibleFailure.code + ' · ' + visibleFailure.stage + ')'" />
    <AppButton v-if="visibleFailure" data-update-retry :disabled="settingsBlocked || !active" @click="checkUpdate">{{ t('updateRetryCheck') }}</AppButton>
    <InlineNotice v-if="!updates.preferencesPersistent" kind="warning" :message="t('updatePreferencesMemoryOnly')" />
    <div v-if="updates.ignoredCandidate" data-update-ignored class="actions">
      <p>{{ t('updateIgnoredCandidate', { version: updates.ignoredCandidate.version }) }}</p>
      <AppButton data-update-clear-ignore variant="ghost" :disabled="settingsBlocked || !active" @click="clearIgnoredCandidate">{{ t('updateClearIgnore') }}</AppButton>
    </div>
    <p v-if="updates.updateInfo && !updates.updateInfo.hasUpdate">{{ t('upToDate') }}</p>
    <div v-if="updates.updateInfo?.hasUpdate" class="package-summary" data-update-package>
      <strong>{{ t('updateObservedPackage', { version: updates.updateInfo.version }) }}</strong>
      <span data-update-channel>{{ t(channelLabels[channel]) }}</span>
      <InlineNotice v-if="!eligible" :kind="eligibilityHint === 'updateSourceUnverified' ? 'warning' : 'info'" :message="t(eligibilityHint)" />
      <p v-if="releaseNotes" class="release-notes">{{ releaseNotes }}</p>
      <AppButton data-update-review :disabled="!active || updates.isCandidateIgnored" @click="reviewInstall">{{ t('updateReviewInstallation') }}</AppButton>
      <AppButton v-if="!updates.isCandidateIgnored" data-update-ignore variant="ghost" :disabled="!eligible || settingsBlocked || !active" @click="ignoreCandidate">{{ t('updateIgnoreCandidate') }}</AppButton>
    </div>
    <p v-if="installing || updates.downloadState === 'cancelled'" data-update-progress role="status">{{ t(progressKey, { percent: updates.downloadProgress.percent }) }}</p>
    <AppButton v-if="canCancel" data-update-cancel :disabled="cancelling || !active" @click="cancelUpdate">{{ t('updateCancelDownload') }}</AppButton>
    <p v-if="installing && cancellation" data-update-cancel-status role="status">{{ t(cancellation === 'requested' ? 'updateCancelling' : 'updateCancelTooLate') }}</p>
    <div class="actions"><AppButton data-update-install :disabled="!eligible || installing || !active" @click="reviewInstall">{{ t('updateRestartInstall') }}</AppButton><AppButton variant="ghost" @click="manualDownload">{{ t('manualDownload') }}</AppButton></div>
    <AppButton data-update-recovery-guide variant="ghost" :disabled="!active" @click="recoveryGuide = true">{{ t('updateRecoveryGuide') }}</AppButton>
    <details data-update-history class="update-history">
      <summary>{{ t('updateHistoryTitle') }}</summary>
      <p>{{ t('updateHistoryHint') }}</p>
      <ul v-if="updates.history.length">
        <li v-for="(entry, index) in updates.history" :key="index">
          <time>{{ historyTime(entry.at) }}</time> · {{ t(`updateHistoryEvent_${entry.event}`) }}
          <span v-if="entry.version"> · v{{ entry.version }}</span> · {{ t(entry.channel === 'stable' ? 'updateChannelStable' : entry.channel === 'test' ? 'updateChannelTest' : 'updateChannelPreview') }}
          <span v-if="entry.failure"> · {{ entry.failure.code }} / {{ entry.failure.stage }}</span>
        </li>
      </ul>
      <p v-else>{{ t('updateHistoryEmpty') }}</p>
      <AppButton v-if="updates.history.length" data-update-clear-history variant="ghost" :disabled="!active" @click="updates.clearHistory">{{ t('updateHistoryClear') }}</AppButton>
    </details>
    <HistoricalVersionsPanel :active="active" />
    <AppDialog :open="recoveryGuide && active" data-update-recovery-dialog :title="t('updateRecoveryGuide')" @close="recoveryGuide = false">
      <p>{{ t('updateRecoveryBeforeInstall') }}</p>
      <p>{{ t('updateRecoveryManual') }}</p>
      <InlineNotice kind="warning" :message="t('updateRecoverySharedData')" />
      <p>{{ t('updateRecoveryBackup') }}</p>
      <template #footer><AppButton autofocus @click="recoveryGuide = false">{{ t('close') }}</AppButton><AppButton data-update-recovery-releases @click="manualDownload">{{ t('manualDownload') }}</AppButton></template>
    </AppDialog>
    <AppDialog :open="reviewing && active" data-update-confirmation :title="t('updateReviewInstallation')" @close="reviewing = false">
      <p data-update-running-count>{{ t('updateRunningCount', { count: counts.running }) }}</p>
      <p data-update-starting-count>{{ t('updateStartingCount', { count: counts.starting }) }}</p>
      <p data-update-unknown-count>{{ t('updateUnknownCount', { count: counts.unknown }) }}</p>
      <InlineNotice kind="warning" :message="t('updateInstallImpact')" />
      <InlineNotice v-if="sessionsBusy" :message="t('updateSessionsBusy')" />
      <InlineNotice v-if="!eligible" :message="t(eligibilityHint)" />
      <template #footer><AppButton :disabled="installing" @click="reviewing = false">{{ t('cancel') }}</AppButton><AppButton data-update-confirm-install data-danger="true" :disabled="!eligible || sessionsBusy || installing || !active" @click="installUpdate">{{ t('updateRestartInstall') }}</AppButton></template>
    </AppDialog>
  </section>
</template>
<style scoped>
.remaining-settings { display: flex; flex-direction: column; gap: 16px; min-width: 0; max-width: 760px; color: var(--text-primary); }
h2 { font-size: 20px; } .version-row, .actions, .channel-legend { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
.channel-legend { color: var(--text-secondary); font-size: 12px; } .channel-legend span { padding: 4px 8px; border: 1px solid var(--border-color); border-radius: 4px; }
.package-summary { display: flex; flex-direction: column; align-items: flex-start; gap: 10px; min-width: 0; }
.release-notes { white-space: pre-wrap; overflow-wrap: anywhere; color: var(--text-secondary); font-size: 13px; line-height: 1.6; }
.proxy-settings { display: flex; flex-direction: column; align-items: flex-start; gap: 8px; }
.proxy-credentials { display: flex; flex-wrap: wrap; gap: 12px; }
.proxy-settings p { color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
.update-history { font-size: 12px; color: var(--text-secondary); line-height: 1.6; }
.update-history summary { cursor: pointer; color: var(--text-primary); font-weight: 600; }
.update-history ul { padding-left: 20px; overflow-wrap: anywhere; } .update-history p { margin: 8px 0; }
</style>
