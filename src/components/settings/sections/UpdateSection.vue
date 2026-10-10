<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-shell'
import { useI18n } from 'vue-i18n'
import { checkForUpdates, getUpdaterSettings, saveUpdaterSettings, testUpdaterProxy, installDesktopUpdate } from '@/api/tauri'
import { isOrdinaryUpdateEligible } from '@/utils/updatePolicy'
import { updateFailure } from '@/utils/updateErrors'
import { useSidebarStore } from '@/stores/sidebar'
import { useUpdateStore } from '@/stores/update'
import { useOwnedSessionCounts } from '@/composables/useOwnedSessionCounts'
import AppButton from '@/components/ui/AppButton.vue'
import AppInput from '@/components/ui/AppInput.vue'
import AppDialog from '@/components/ui/AppDialog.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import HistoricalVersionsPanel from '@/components/settings/HistoricalVersionsPanel.vue'
const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })
const { t } = useI18n(), sidebar = useSidebarStore(), updates = useUpdateStore()
const counts = useOwnedSessionCounts(), checking = ref(false), error = ref<ReturnType<typeof updateFailure> | null>(null), reviewing = ref(false)
const proxy = ref(''), savedProxy = ref(''), savingProxy = ref(false), loadingProxy = ref(true)
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
const installing = computed(() => ['downloading', 'installing'].includes(updates.downloadState))
const eligible = computed(() => isOrdinaryUpdateEligible(updates.updateInfo))
const sessionsBusy = computed(() => counts.value.running + counts.value.starting + counts.value.unknown > 0)
const channel = computed(() => eligible.value ? 'stable' : ['candidate', 'test-only'].includes(updates.updateInfo?.channel ?? '') ? updates.updateInfo!.channel! : 'unverified')
const channelLabels = { stable: 'updateStable', candidate: 'updateCandidate', 'test-only': 'updateTestOnly', unverified: 'updateUnverified' }
let owner = 0, proxyOwner = 0, disposed = false, unlisten: UnlistenFn | undefined
function invalidateProxy() { ++proxyOwner; proxyResult.value = null }
function invalidate() { ++owner; checking.value = false; reviewing.value = false; error.value = null; invalidateProxy() }
watch([proxy, proxyUsername, proxyPassword, originalUserinfo], invalidateProxy, { flush: 'sync' })
watch(() => props.active, active => { if (!active) invalidate() }, { flush: 'sync' })
onBeforeUnmount(() => { disposed = true; unlisten?.(); invalidate() })
onMounted(async () => {
  const version = owner
  try {
    const settings = await getUpdaterSettings()
    if (!disposed) { setProxyAddress(settings.proxy ?? ''); savedProxy.value = proxyValue.value }
  } catch (cause) { if (!disposed && version === owner) error.value = updateFailure(cause) }
  finally { loadingProxy.value = false }
  try {
    const stop = await listen<{ admissionId: string; phase: string; downloaded: number; total: number | null }>('desktop-update-progress', ({ payload }) => {
      if (disposed || !installing.value || payload.admissionId !== updates.updateInfo?.admissionId) return
      if (payload.phase === 'installing') updates.setDownloadState('installing')
      if (Number.isSafeInteger(payload.downloaded) && Number.isSafeInteger(payload.total) && payload.total! > 0 && payload.downloaded >= 0 && payload.downloaded <= payload.total!) {
        updates.setDownloadProgress({ downloaded: payload.downloaded, total: payload.total!, percent: Math.floor(100 * payload.downloaded / payload.total!) })
      }
    })
    if (disposed) stop(); else unlisten = stop
  } catch { /* Progress display is optional; installation remains owned by the host. */ }
})
async function saveProxy() {
  if (!props.active || loadingProxy.value || checking.value || savingProxy.value || installing.value || testingProxy.value) return
  const version = owner, value = proxyValue.value
  savingProxy.value = true; error.value = null
  updates.setUpdateInfo(null); sidebar.setUpdateInfo(null)
  try {
    await saveUpdaterSettings(value || null)
    if (!disposed && owner === version) savedProxy.value = value
  } catch (cause) { if (!disposed && owner === version && props.active) error.value = updateFailure(cause) }
  finally { savingProxy.value = false }
}
async function testProxy() {
  if (!props.active || loadingProxy.value || checking.value || savingProxy.value || installing.value || testingProxy.value) return
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
  if (!props.active || loadingProxy.value || checking.value || savingProxy.value || installing.value || testingProxy.value) return
  const version = ++owner; checking.value = true; error.value = null
  updates.setUpdateInfo(null)
  sidebar.setUpdateInfo(null)
  try {
    const info = await checkForUpdates()
    if (owner !== version || !props.active) return
    updates.setUpdateInfo(info); sidebar.setUpdateInfo(info)
  } catch (cause) { if (owner === version && props.active) error.value = updateFailure(cause) }
  finally { if (owner === version) checking.value = false }
}
function reviewInstall() { if (props.active && !installing.value && updates.updateInfo?.hasUpdate) reviewing.value = true }
async function installUpdate() {
  const info = updates.updateInfo, version = owner
  if (!props.active || checking.value || savingProxy.value || installing.value || !isOrdinaryUpdateEligible(info) || !info?.admissionId || sessionsBusy.value) return
  updates.resetDownload(); updates.setDownloadState('downloading'); error.value = null
  try { await installDesktopUpdate(info.admissionId); updates.setDownloadState('installing') }
  catch (cause) {
    updates.setDownloadState('error')
    if (updates.updateInfo?.admissionId === info.admissionId) {
      const revoked = { ...info, admissionId: null, installEligible: false }
      updates.setUpdateInfo(revoked); sidebar.setUpdateInfo(revoked)
    }
    if (!disposed && owner === version && props.active) error.value = updateFailure(cause)
  }
}
async function manualDownload() {
  if (!props.active) return
  const version = owner
  try { await open('https://github.com/shawnwu2022/cc-desk/releases') }
  catch { if (version === owner && props.active) error.value = { code: 'UPDATER_FAILED', stage: 'unknown', key: 'settingsExternalLinkFailed' } }
}
</script>
<template>
  <section class="remaining-settings" data-settings-update>
    <h2>{{ t('ccDeskUpdate') }}</h2>
    <div class="version-row"><span>CC Desk v{{ currentVersion }}</span><AppButton data-update-check :disabled="loadingProxy || checking || savingProxy || installing || testingProxy || !active" @click="checkUpdate">{{ checking ? t('checking') : t('checkForUpdates') }}</AppButton></div>
    <div class="proxy-settings">
      <AppInput v-model="proxyAddress" data-update-proxy type="text" autocomplete="off" :label="t('updateProxyLabel')" :disabled="loadingProxy || checking || savingProxy || installing || !active" />
      <div class="proxy-credentials">
        <AppInput v-model="proxyUsername" data-update-proxy-username type="password" autocomplete="off" :label="t('updateProxyUsername')" :disabled="loadingProxy || checking || savingProxy || installing || !active" />
        <AppInput v-model="proxyPassword" data-update-proxy-password type="password" autocomplete="new-password" :label="t('updateProxyPassword')" :disabled="loadingProxy || checking || savingProxy || installing || !active" />
      </div>
      <p>{{ t('updateProxyHint') }}</p>
      <div class="actions">
        <AppButton data-update-proxy-save :disabled="loadingProxy || checking || savingProxy || installing || testingProxy || !active || proxyValue === savedProxy" @click="saveProxy">{{ t('updateProxySave') }}</AppButton>
        <AppButton data-update-proxy-test :disabled="loadingProxy || checking || savingProxy || installing || testingProxy || !active" @click="testProxy">{{ testingProxy ? t('updateProxyTesting') : t('updateProxyTest') }}</AppButton>
      </div>
      <p v-if="proxyResult" data-update-proxy-result role="status">
        <template v-if="proxyResult.state === 'testing'">{{ t('updateProxyTesting') }}</template>
        <template v-else-if="proxyResult.state === 'success'">{{ t('updateProxyTestSuccess', { elapsed: proxyResult.elapsedMs }) }}</template>
        <template v-else>{{ t('updateProxyTestFailure', { elapsed: proxyResult.elapsedMs }) }} {{ t(proxyResult.failure!.key) }} ({{ proxyResult.failure!.code }})</template>
      </p>
    </div>
    <div class="channel-legend"><span>{{ t('updateStable') }}</span><span>{{ t('updateCandidate') }}</span><span>{{ t('updateTestOnly') }}</span></div>
    <InlineNotice data-update-policy kind="info" :message="t('updatePublicationBlocked')" />
    <InlineNotice v-if="error" data-update-error kind="warning" :message="t(error.key) + ' (' + error.code + ' · ' + error.stage + ')'" />
    <p v-if="updates.updateInfo && !updates.updateInfo.hasUpdate">{{ t('upToDate') }}</p>
    <div v-if="updates.updateInfo?.hasUpdate" class="package-summary" data-update-package>
      <strong>{{ t('updateObservedPackage', { version: updates.updateInfo.version }) }}</strong>
      <span data-update-channel>{{ t(channelLabels[channel]) }}</span>
      <InlineNotice v-if="!eligible" kind="warning" :message="t('updateSourceUnverified')" />
      <p v-if="updates.updateInfo.releaseNotes" class="release-notes">{{ updates.updateInfo.releaseNotes }}</p>
      <AppButton data-update-review :disabled="!active" @click="reviewInstall">{{ t('updateReviewInstallation') }}</AppButton>
    </div>
    <p v-if="installing" role="status">{{ updates.downloadState === 'installing' ? t('updateInstalling') : t('updateDownloading', { percent: updates.downloadProgress.percent }) }}</p>
    <div class="actions"><AppButton data-update-install :disabled="!eligible || installing || !active" @click="reviewInstall">{{ t('updateRestartInstall') }}</AppButton><AppButton variant="ghost" @click="manualDownload">{{ t('manualDownload') }}</AppButton></div>
    <HistoricalVersionsPanel :active="active" />
    <AppDialog :open="reviewing && active" data-update-confirmation :title="t('updateReviewInstallation')" @close="reviewing = false">
      <p data-update-running-count>{{ t('updateRunningCount', { count: counts.running }) }}</p>
      <p data-update-starting-count>{{ t('updateStartingCount', { count: counts.starting }) }}</p>
      <p data-update-unknown-count>{{ t('updateUnknownCount', { count: counts.unknown }) }}</p>
      <InlineNotice kind="warning" :message="t('updateInstallImpact')" />
      <InlineNotice v-if="sessionsBusy" :message="t('updateSessionsBusy')" />
      <InlineNotice v-if="!eligible" :message="t('updateSourceUnverified')" />
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
</style>
