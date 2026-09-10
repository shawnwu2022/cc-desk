<template>
"
        "  <div class="section-content">
"
        "    <h2 class="section-heading">{{ t('ccDeskUpdate') }}</h2>
"
        "    <div class="update-card">
"
        "      <div class="version-row">
"
        "        <div class="version-info">
"
        "          <span class="version-label">CC Desk</span>
"
        "          <span class="version-value">v{{ currentVersion }}</span>
"
        "        </div>
"
        "        <button class="check-btn" :disabled="checking" @click="handleCheckUpdate">
"
        "          {{ checking ? t('checking') : t('checkForUpdates') }}
"
        "        </button>
"
        "      </div>

"
        "      <div v-if="errorMessage" class="update-message error">
"
        "        {{ t('checkFailed', { error: errorMessage }) }}
"
        "      </div>
"
        "      <div v-else-if="updateStore.updateInfo && !updateStore.updateInfo.hasUpdate" class="update-message success">
"
        "        {{ t('upToDate') }}
"
        "      </div>

"
        "      <div v-if="updateStore.updateInfo?.hasUpdate" class="update-available">
"
        "        <div class="update-banner">
"
        "          <strong>{{ t('versionAvailable', { version: updateStore.updateInfo.version }) }}</strong>
"
        "          <span>{{ t('yourVersion') }} v{{ updateStore.updateInfo.currentVersion }}</span>
"
        "        </div>
"
        "        <div v-if="updateStore.updateInfo.releaseNotes" class="release-notes">
"
        "          <h4>{{ t('whatsNew') }}</h4>
"
        "          <div class="notes-content" v-html="renderedNotes"></div>
"
        "        </div>
"
        "        <div v-if="updateStore.downloadState === 'downloading'" class="progress-section">
"
        "          <div class="progress-bar"><div class="progress-fill" :style="{ width: updateStore.downloadProgress.percent + '%' }"></div></div>
"
        "          <span>{{ updateStore.downloadProgress.percent.toFixed(0) }}%</span>
"
        "        </div>
"
        "        <p v-else-if="updateStore.downloadState === 'installing'" class="update-message">{{ t('installingUpdate') }}</p>
"
        "        <p v-else-if="updateStore.downloadState === 'error'" class="update-message error">{{ updateStore.downloadError }}</p>
"
        "        <div class="action-row">
"
        "          <button v-if="updateStore.downloadState === 'idle'" class="action-btn primary" @click="handleDownloadAndInstall">{{ t('downloadAndInstall') }}</button>
"
        "          <button v-if="updateStore.downloadState === 'error'" class="action-btn primary" @click="handleRetry">{{ t('retry') }}</button>
"
        "          <button class="action-btn secondary" @click="openReleases">{{ t('manualDownload') }}</button>
"
        "        </div>
"
        "      </div>
"
        "    </div>

"
        "    <div v-if="showConfirm" class="confirm-overlay" @click.self="showConfirm = false">
"
        "      <div class="confirm-dialog">
"
        "        <p>{{ t('updateConfirmActivePtys') }}</p>
"
        "        <div class="action-row">
"
        "          <button class="action-btn secondary" @click="showConfirm = false">{{ t('cancel') }}</button>
"
        "          <button class="action-btn primary" @click="confirmUpdate">{{ t('downloadAndInstall') }}</button>
"
        "        </div>
"
        "      </div>
"
        "    </div>
"
        "  </div>
"
        "</template>

"
        "<script setup lang="ts">
"
        "import { computed, ref } from 'vue'
"
        "import { useI18n } from 'vue-i18n'
"
        "import { open } from '@tauri-apps/plugin-shell'
"
        "import { check, checkForUpdates, relaunch } from '@/api/tauri'
"
        "import { useSessionStore } from '@/stores/session'
"
        "import { useSidebarStore } from '@/stores/sidebar'
"
        "import { useUpdateStore } from '@/stores/update'

"
        "const { t } = useI18n()
"
        "const sessionStore = useSessionStore()
"
        "const sidebarStore = useSidebarStore()
"
        "const updateStore = useUpdateStore()
"
        "const currentVersion = __APP_VERSION__
"
        "const checking = ref(false)
"
        "const errorMessage = ref('')
"
        "const showConfirm = ref(false)

"
        "const renderedNotes = computed(() => (updateStore.updateInfo?.releaseNotes ?? '')
"
        "  .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
"
        "  .replace(/\n/g, '<br>'))

"
        "async function handleCheckUpdate() {
"
        "  checking.value = true
"
        "  errorMessage.value = ''
"
        "  try {
"
        "    const info = await checkForUpdates()
"
        "    updateStore.setUpdateInfo(info)
"
        "    sidebarStore.setUpdateInfo(info)
"
        "  } catch (error) {
"
        "    errorMessage.value = String(error)
"
        "  } finally {
"
        "    checking.value = false
"
        "  }
"
        "}

"
        "function openReleases() {
"
        "  open('https://github.com/shawnwu2022/cc-desk/releases')
"
        "}

"
        "function handleDownloadAndInstall() {
"
        "  if (sessionStore.runningTabIds.length > 0) {
"
        "    showConfirm.value = true
"
        "    return
"
        "  }
"
        "  void startDownload()
"
        "}

"
        "async function confirmUpdate() {
"
        "  showConfirm.value = false
"
        "  await startDownload()
"
        "}

"
        "async function startDownload() {
"
        "  updateStore.setDownloadState('downloading')
"
        "  updateStore.clearError()
"
        "  updateStore.setDownloadProgress({ downloaded: 0, total: 0, percent: 0 })
"
        "  try {
"
        "    const update = await check()
"
        "    if (!update) throw new Error(t('noUpdateAvailable'))
"
        "    let downloaded = 0
"
        "    let total = 0
"
        "    await update.downloadAndInstall((event) => {
"
        "      if (event.event === 'Started') {
"
        "        total = event.data.contentLength ?? 0
"
        "      } else if (event.event === 'Progress') {
"
        "        downloaded += event.data.chunkLength
"
        "      } else if (event.event === 'Finished') {
"
        "        updateStore.setDownloadState('installing')
"
        "      }
"
        "      updateStore.setDownloadProgress({
"
        "        downloaded, total, percent: total > 0 ? downloaded / total * 100 : 0,
"
        "      })
"
        "    })
"
        "    await relaunch()
"
        "  } catch (error) {
"
        "    updateStore.setDownloadError(t('updateFailed', { error: String(error) }))
"
        "    updateStore.setDownloadState('error')
"
        "  }
"
        "}

"
        "async function handleRetry() {
"
        "  updateStore.resetDownload()
"
        "  await startDownload()
"
        "}
"
        "</script>

"
        "<style scoped>
"
        ".section-content { max-width: 760px; }
"
        ".section-heading { margin: 0 0 20px; color: var(--text-primary); }
"
        ".update-card { padding: 20px; border: 1px solid var(--border-color); border-radius: 10px; background: var(--bg-secondary); }
"
        ".version-row, .version-info, .action-row { display: flex; align-items: center; gap: 12px; }
"
        ".version-row { justify-content: space-between; }
"
        ".version-info { flex-direction: column; align-items: flex-start; gap: 2px; }
"
        ".version-label, .version-value { color: var(--text-primary); }
"
        ".version-value { font-size: 12px; color: var(--text-secondary); }
"
        ".check-btn, .action-btn { border: 1px solid var(--border-color); border-radius: 6px; padding: 7px 12px; cursor: pointer; }
"
        ".check-btn:disabled { cursor: default; opacity: .6; }
"
        ".action-btn.primary { background: var(--accent-color); color: white; border-color: var(--accent-color); }
"
        ".action-btn.secondary, .check-btn { background: var(--bg-primary); color: var(--text-primary); }
"
        ".update-message { margin-top: 16px; color: var(--text-secondary); }
"
        ".update-message.error { color: var(--status-error); }
"
        ".update-message.success { color: var(--status-success); }
"
        ".update-available { margin-top: 18px; display: grid; gap: 16px; }
"
        ".update-banner { display: flex; flex-direction: column; gap: 4px; color: var(--text-primary); }
"
        ".release-notes { color: var(--text-secondary); }
"
        ".release-notes h4 { color: var(--text-primary); margin: 0 0 8px; }
"
        ".notes-content { line-height: 1.6; }
"
        ".progress-bar { height: 6px; overflow: hidden; border-radius: 3px; background: var(--bg-primary); }
"
        ".progress-fill { height: 100%; background: var(--accent-color); }
"
        ".confirm-overlay { position: fixed; inset: 0; z-index: 200; display: grid; place-items: center; background: rgba(0,0,0,.45); }
"
        ".confirm-dialog { width: min(420px, 90vw); padding: 20px; border-radius: 10px; background: var(--bg-primary); color: var(--text-primary); }
"
        "</style>
