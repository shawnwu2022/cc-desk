<template>
  <div class="section-content">
    <h2 class="section-heading">Software Update</h2>

    <div class="update-card">
      <div class="version-row">
        <div class="version-info">
          <span class="version-label">Current Version</span>
          <span class="version-value">v{{ currentVersion }}</span>
        </div>
        <button
          class="check-btn"
          :disabled="checking"
          @click="handleCheckUpdate"
        >
          <img v-if="checking" src="@/assets/icons/refresh.svg" class="spinning" alt="" />
          <span>{{ checking ? 'Checking...' : 'Check for Updates' }}</span>
        </button>
      </div>

      <div v-if="error" class="update-message error">
        <span>Failed to check for updates. Please try again later.</span>
      </div>

      <div v-if="updateInfo && !updateInfo.hasUpdate" class="update-message success">
        <span>You're up to date!</span>
      </div>

      <template v-if="updateInfo && updateInfo.hasUpdate">
        <div class="update-available">
          <div class="update-banner">
            <span class="update-icon">🆕</span>
            <div>
              <span class="update-version">v{{ updateInfo.version }} is available</span>
              <span class="update-hint">Your version: v{{ updateInfo.currentVersion }}</span>
            </div>
          </div>

          <div v-if="updateInfo.releaseNotes" class="release-notes">
            <h4>What's New</h4>
            <div class="notes-content" v-html="renderedNotes"></div>
          </div>

          <!-- 下载/安装状态 -->
          <div class="update-actions">
            <!-- 初始状态：显示下载按钮 -->
            <template v-if="downloadState === 'idle'">
              <button
                v-if="updateInfo.platformAsset"
                class="action-btn primary"
                @click="handleDownload"
              >
                Download & Install
              </button>
              <a
                class="action-btn secondary"
                :href="updateInfo.downloadUrl"
                target="_blank"
                rel="noopener"
                @click.prevent="openExternal(updateInfo.downloadUrl)"
              >
                {{ updateInfo.platformAsset ? 'Download on GitHub' : 'Download on GitHub' }}
              </a>
              <span v-if="updateInfo.platformAsset" class="file-size">
                {{ formatSize(updateInfo.platformAsset.size) }}
              </span>
            </template>

            <!-- 下载中 -->
            <template v-if="downloadState === 'downloading'">
              <div class="progress-section">
                <div class="progress-bar">
                  <div class="progress-fill" :style="{ width: downloadProgress.percent + '%' }"></div>
                </div>
                <div class="progress-info">
                  <span>{{ downloadProgress.percent.toFixed(0) }}%</span>
                  <span class="progress-size">
                    {{ formatSize(downloadProgress.downloaded) }} / {{ formatSize(downloadProgress.total) }}
                  </span>
                </div>
              </div>
            </template>

            <!-- 下载完成 -->
            <template v-if="downloadState === 'downloaded'">
              <button class="action-btn primary" @click="handleInstall">
                Install & Restart
              </button>
              <span class="file-size">Ready to install</span>
            </template>

            <!-- 安装中 -->
            <template v-if="downloadState === 'installing'">
              <div class="installing-message">
                <span class="spinning-text">Installing update...</span>
                <span class="installing-hint">The application will restart automatically.</span>
              </div>
            </template>

            <!-- 错误 -->
            <div v-if="downloadError" class="update-message error">
              <span>{{ downloadError }}</span>
              <button class="retry-link" @click="downloadState = 'idle'; downloadError = ''">Retry</button>
            </div>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { open } from '@tauri-apps/plugin-shell'
import { checkForUpdates, downloadUpdate, installUpdate, onUpdateDownloadProgress } from '@/api/tauri'
import { useSidebarStore } from '@/stores/sidebar'
import type { UpdateInfo, DownloadProgress } from '@/types'

const sidebarStore = useSidebarStore()
const currentVersion = __APP_VERSION__
const checking = ref(false)
const error = ref(false)
const updateInfo = ref<UpdateInfo | null>(sidebarStore.updateInfo)

type DownloadState = 'idle' | 'downloading' | 'downloaded' | 'installing'
const downloadState = ref<DownloadState>('idle')
const downloadProgress = ref<DownloadProgress>({ downloaded: 0, total: 0, percent: 0 })
const downloadError = ref('')
const downloadedFilePath = ref('')

let unlistenProgress: (() => void) | null = null

onMounted(async () => {
  unlistenProgress = await onUpdateDownloadProgress((progress) => {
    downloadProgress.value = progress
    if (progress.percent >= 100) {
      downloadState.value = 'downloaded'
    }
  })
})

onUnmounted(() => {
  unlistenProgress?.()
})

const renderedNotes = computed(() => {
  if (!updateInfo.value?.releaseNotes) return ''
  return updateInfo.value.releaseNotes
    .replace(/\n/g, '<br>')
    .replace(/#{1,3}\s(.+)/g, '<strong>$1</strong>')
})

function openExternal(url: string) {
  open(url)
}

function formatSize(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(1024))
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`
}

async function handleCheckUpdate() {
  checking.value = true
  error.value = false
  try {
    const info = await checkForUpdates()
    updateInfo.value = info
    sidebarStore.setUpdateInfo(info)
  } catch {
    error.value = true
  } finally {
    checking.value = false
  }
}

async function handleDownload() {
  if (!updateInfo.value?.platformAsset) return

  downloadState.value = 'downloading'
  downloadError.value = ''
  downloadProgress.value = { downloaded: 0, total: 0, percent: 0 }

  try {
    const asset = updateInfo.value.platformAsset
    downloadedFilePath.value = await downloadUpdate(asset.url, asset.name)
    downloadState.value = 'downloaded'
  } catch (err) {
    downloadError.value = `Download failed: ${err}`
    downloadState.value = 'idle'
  }
}

async function handleInstall() {
  if (!downloadedFilePath.value) return

  downloadState.value = 'installing'
  try {
    await installUpdate(downloadedFilePath.value)
  } catch (err) {
    downloadError.value = `Install failed: ${err}`
    downloadState.value = 'downloaded'
  }
}
</script>

<style scoped>
.section-content {
  padding: 8px 0;
}

.section-heading {
  font-size: 20px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 24px;
}

.update-card {
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  padding: 20px;
}

.version-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.version-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.version-label {
  font-size: 12px;
  color: var(--text-tertiary);
}

.version-value {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
  font-variant-numeric: tabular-nums;
}

.check-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  background: var(--accent-color);
  color: white;
  border: none;
  border-radius: 6px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: opacity 0.15s ease;
}

.check-btn:hover:not(:disabled) {
  opacity: 0.9;
}

.check-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.check-btn img {
  width: 14px;
  height: 14px;
}

.spinning {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.update-message {
  margin-top: 16px;
  padding: 10px 14px;
  border-radius: 6px;
  font-size: 13px;
}

.update-message.error {
  background: rgba(239, 68, 68, 0.1);
  color: var(--error-color);
}

.update-message.success {
  background: rgba(34, 197, 94, 0.1);
  color: var(--success-color);
}

.update-available {
  margin-top: 16px;
}

.update-banner {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  background: var(--accent-light);
  border-radius: 6px;
  margin-bottom: 16px;
}

.update-icon {
  font-size: 20px;
}

.update-version {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  display: block;
}

.update-hint {
  font-size: 12px;
  color: var(--text-secondary);
}

.release-notes {
  margin-bottom: 16px;
}

.release-notes h4 {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 8px;
}

.notes-content {
  font-size: 13px;
  color: var(--text-primary);
  line-height: 1.6;
  max-height: 200px;
  overflow-y: auto;
  padding: 12px;
  background: var(--bg-primary);
  border-radius: 6px;
  border: 1px solid var(--border-color);
}

.update-actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.action-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 10px 20px;
  border-radius: 6px;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: opacity 0.15s ease;
  text-decoration: none;
  border: none;
}

.action-btn:hover {
  opacity: 0.9;
}

.action-btn.primary {
  background: var(--accent-color);
  color: white;
}

.action-btn.secondary {
  background: transparent;
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
}

.action-btn.secondary:hover {
  border-color: var(--accent-color);
  color: var(--accent-color);
}

.file-size {
  font-size: 12px;
  color: var(--text-tertiary);
}

.progress-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.progress-bar {
  height: 8px;
  background: var(--bg-primary);
  border-radius: 4px;
  overflow: hidden;
  border: 1px solid var(--border-color);
}

.progress-fill {
  height: 100%;
  background: var(--accent-color);
  border-radius: 4px;
  transition: width 0.3s ease;
}

.progress-info {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  color: var(--text-secondary);
}

.progress-size {
  color: var(--text-tertiary);
}

.installing-message {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 20px;
  background: var(--accent-light);
  border-radius: 6px;
}

.spinning-text {
  font-size: 14px;
  font-weight: 500;
  color: var(--text-primary);
  animation: pulse-text 1.5s ease-in-out infinite;
}

.installing-hint {
  font-size: 12px;
  color: var(--text-secondary);
}

@keyframes pulse-text {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}

.retry-link {
  background: none;
  border: none;
  color: var(--accent-color);
  cursor: pointer;
  font-size: 13px;
  text-decoration: underline;
  padding: 0;
  margin-left: 8px;
}
</style>
