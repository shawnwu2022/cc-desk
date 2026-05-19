<template>
  <div class="section-content">
    <h2 class="section-heading">{{ t('ccBoxUpdate') }}</h2>

    <div class="update-card">
      <div class="version-row">
        <div class="version-info">
          <span class="version-label">CC-Box</span>
          <span class="version-value">v{{ currentVersion }}</span>
        </div>
        <button
          class="check-btn"
          :disabled="checking"
          @click="handleCheckUpdate"
        >
          <img v-if="checking" src="@/assets/icons/refresh.svg" class="spinning" alt="" />
          <span>{{ checking ? t('checking') : t('checkForUpdates') }}</span>
        </button>
      </div>

      <div v-if="error" class="update-message error">
        <span>{{ t('checkFailed', { error: errorMessage }) }}</span>
      </div>

      <div v-if="updateStore.updateInfo && !updateStore.updateInfo.hasUpdate" class="update-message success">
        <span>{{ t('upToDate') }}</span>
      </div>

      <template v-if="updateStore.updateInfo && updateStore.updateInfo.hasUpdate">
        <div class="update-available">
          <div class="update-banner">
            <span class="update-icon">🆕</span>
            <div>
              <span class="update-version">{{ t('versionAvailable', { version: updateStore.updateInfo.version }) }}</span>
              <span class="update-hint">{{ t('yourVersion') }} v{{ updateStore.updateInfo.currentVersion }}</span>
            </div>
          </div>

          <div v-if="updateStore.updateInfo.releaseNotes" class="release-notes">
            <h4>{{ t('whatsNew') }}</h4>
            <div class="notes-content" v-html="renderedNotes"></div>
          </div>

          <!-- 下载/安装状态 -->
          <div class="update-actions">
            <!-- 初始状态：显示下载并安装按钮 -->
            <template v-if="updateStore.downloadState === 'idle'">
              <div class="action-row">
                <button class="action-btn primary" @click="handleDownloadAndInstall">
                  {{ t('downloadAndInstall') }}
                </button>
                <button class="action-btn secondary" @click="openReleases">
                  {{ t('manualDownload') }}
                </button>
              </div>
            </template>

            <!-- 下载中 -->
            <template v-if="updateStore.downloadState === 'downloading'">
              <div class="progress-section">
                <div class="progress-header">
                  <span>{{ t('downloadingUpdate') }}</span>
                </div>
                <div class="progress-bar">
                  <div class="progress-fill" :style="{ width: updateStore.downloadProgress.percent + '%' }"></div>
                </div>
                <div class="progress-info">
                  <span>{{ updateStore.downloadProgress.percent.toFixed(0) }}%</span>
                  <span class="progress-size">
                    {{ formatSize(updateStore.downloadProgress.downloaded) }} / {{ formatSize(updateStore.downloadProgress.total) }}
                  </span>
                </div>
              </div>
            </template>

            <!-- 安装中 -->
            <template v-if="updateStore.downloadState === 'installing'">
              <div class="installing-message">
                <span class="spinning-text">{{ t('installingUpdate') }}</span>
                <span class="installing-hint">{{ t('willRestartAuto') }}</span>
              </div>
            </template>

            <!-- 错误 -->
            <div v-if="updateStore.downloadState === 'error'" class="update-message error">
              <span>{{ updateStore.downloadError }}</span>
              <div class="error-actions">
                <button class="retry-link" @click="handleRetry">{{ t('retry') }}</button>
              </div>
            </div>
          </div>
        </div>
      </template>
    </div>

    <!-- Claude CLI Update Card -->
    <div class="update-card claude-cli-card">
      <div class="version-row">
        <div class="version-info">
          <span class="version-label">Claude CLI</span>
          <span class="version-value" v-if="!updateStore.claudeCliUpdateInfo?.notInstalled">
            v{{ updateStore.claudeCliUpdateInfo?.installedVersion || '...' }}
          </span>
          <span class="version-value not-installed" v-else>
            {{ t('notInstalled') }}
          </span>
        </div>
        <button
          class="check-btn"
          :disabled="cliChecking"
          @click="handleCheckClaudeCliUpdate"
        >
          <img v-if="cliChecking" src="@/assets/icons/refresh.svg" class="spinning" alt="" />
          <span>{{ cliChecking ? t('checking') : t('checkForUpdates') }}</span>
        </button>
      </div>

      <div v-if="cliError" class="update-message error">
        <span>{{ t('checkFailed', { error: cliErrorMessage }) }}</span>
      </div>

      <div v-if="updateStore.claudeCliUpdateInfo && !updateStore.claudeCliUpdateInfo.hasUpdate
                  && !updateStore.claudeCliUpdateInfo.notInstalled && !cliChecking" class="update-message success">
        <span>{{ t('upToDate') }}</span>
      </div>

      <template v-if="updateStore.claudeCliUpdateInfo?.hasUpdate">
        <div class="update-available">
          <div class="update-banner">
            <span class="update-icon">🆕</span>
            <div>
              <span class="update-version">{{ t('versionAvailable', { version: updateStore.claudeCliUpdateInfo.latestVersion }) }}</span>
              <span class="update-hint">{{ t('yourVersion') }} v{{ updateStore.claudeCliUpdateInfo.installedVersion }}</span>
            </div>
          </div>

          <div class="update-actions">
            <template v-if="updateStore.claudeCliDownloadState === 'idle'">
              <button class="action-btn primary" @click="handleInstallClaudeCli">
                {{ t('downloadAndInstall') }}
              </button>
            </template>

            <template v-if="updateStore.claudeCliDownloadState === 'downloading'">
              <div class="progress-section">
                <div class="progress-header">
                  <span>{{ updateStore.claudeCliDownloadMessage }}</span>
                </div>
                <div class="progress-bar">
                  <div class="progress-fill" :style="{ width: updateStore.claudeCliDownloadProgress + '%' }"></div>
                </div>
                <div class="progress-info">
                  <span>{{ updateStore.claudeCliDownloadProgress }}%</span>
                </div>
              </div>
            </template>

            <div v-if="updateStore.claudeCliDownloadState === 'done'" class="update-message success">
              <span>{{ t('claudeCliInstalled') }}</span>
            </div>

            <div v-if="updateStore.claudeCliDownloadState === 'error'" class="update-message error">
              <span>{{ updateStore.claudeCliDownloadError }}</span>
              <div class="error-actions">
                <button class="retry-link" @click="handleInstallClaudeCli">{{ t('retry') }}</button>
              </div>
            </div>
          </div>
        </div>
      </template>

      <template v-if="updateStore.claudeCliUpdateInfo?.notInstalled && updateStore.claudeCliDownloadState === 'idle'">
        <div class="update-actions" style="margin-top: 12px;">
          <button class="action-btn primary" @click="handleInstallClaudeCli">
            {{ t('installClaudeCli') }}
          </button>
        </div>
      </template>
    </div>

    <!-- CC-Box 更新确认对话框 -->
    <div v-if="showConfirm" class="confirm-overlay" @click.self="showConfirm = false">
      <div class="confirm-dialog">
        <p class="confirm-text">{{ t('updateConfirmActivePtys') }}</p>
        <div class="confirm-actions">
          <button class="btn-cancel" @click="showConfirm = false">{{ t('cancel') }}</button>
          <button class="btn-primary" @click="confirmUpdate">{{ t('downloadAndInstall') }}</button>
        </div>
      </div>
    </div>

    <!-- Claude CLI 更新确认对话框 -->
    <div v-if="showCliConfirm" class="confirm-overlay" @click.self="showCliConfirm = false">
      <div class="confirm-dialog">
        <p class="confirm-text">{{ t('updateConfirmClaudeRunning') }}</p>
        <div class="confirm-actions">
          <button class="btn-cancel" @click="showCliConfirm = false">{{ t('cancel') }}</button>
          <button class="btn-primary" @click="confirmCliUpdate">{{ t('downloadAndInstall') }}</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { check, relaunch } from '@/api/tauri'
import { checkForUpdates, checkClaudeCliUpdate, checkClaudeRunning, killClaudeProcesses, downloadAndInstallClaude, onInstallProgress, ptyKillAll } from '@/api/tauri'
import { open } from '@tauri-apps/plugin-shell'
import { useSidebarStore } from '@/stores/sidebar'
import { useSessionStore } from '@/stores/session'
import { useUpdateStore } from '@/stores/update'

const { t } = useI18n()
const sidebarStore = useSidebarStore()
const sessionStore = useSessionStore()
const updateStore = useUpdateStore()
const currentVersion = __APP_VERSION__
const checking = ref(false)
const error = ref(false)
const errorMessage = ref('')
const showConfirm = ref(false)

// Claude CLI 更新状态
const cliChecking = ref(false)
const cliError = ref(false)
const cliErrorMessage = ref('')
const showCliConfirm = ref(false)
let unlistenCliProgress: (() => void) | null = null

const renderedNotes = computed(() => {
  if (!updateStore.updateInfo?.releaseNotes) return ''
  return updateStore.updateInfo.releaseNotes
    .replace(/\n/g, '<br>')
    .replace(/#{1,3}\s(.+)/g, '<strong>$1</strong>')
})

function formatSize(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(1024))
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`
}

async function handleCheckUpdate() {
  checking.value = true
  error.value = false
  errorMessage.value = ''
  try {
    const info = await checkForUpdates()
    if (info) {
      updateStore.setUpdateInfo(info)
      sidebarStore.setUpdateInfo(info)
    }
  } catch (err) {
    error.value = true
    errorMessage.value = String(err)
  } finally {
    checking.value = false
  }
}

function hasActivePtys(): boolean {
  return sessionStore.runningTabIds.length > 0
}

function openReleases() {
  open('https://github.com/orczh-hj/cc-box/releases')
}

async function handleDownloadAndInstall() {
  if (hasActivePtys()) {
    showConfirm.value = true
    return
  }
  startDownload()
}

async function confirmUpdate() {
  showConfirm.value = false
  await startDownload()
}

async function startDownload() {
  updateStore.setDownloadState('downloading')
  updateStore.clearError()
  updateStore.setDownloadProgress({ downloaded: 0, total: 0, percent: 0 })

  try {
    const update = await check()
    if (!update) {
      updateStore.setDownloadError(t('noUpdateAvailable'))
      updateStore.setDownloadState('error')
      return
    }

    let downloaded = 0
    let contentLength = 0

    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case 'Started':
          contentLength = event.data.contentLength ?? 0
          updateStore.setDownloadProgress({ downloaded: 0, total: contentLength, percent: 0 })
          break
        case 'Progress':
          downloaded += event.data.chunkLength
          const percent = contentLength > 0 ? (downloaded / contentLength) * 100 : 0
          updateStore.setDownloadProgress({ downloaded, total: contentLength, percent })
          break
        case 'Finished':
          updateStore.setDownloadState('installing')
          break
      }
    })

    await relaunch()
  } catch (err) {
    updateStore.setDownloadError(t('updateFailed', { error: String(err) }))
    updateStore.setDownloadState('error')
  }
}

async function handleRetry() {
  updateStore.resetDownload()
  await startDownload()
}

// ============ Claude CLI 更新 ============

async function handleCheckClaudeCliUpdate() {
  cliChecking.value = true
  cliError.value = false
  cliErrorMessage.value = ''
  try {
    const info = await checkClaudeCliUpdate()
    updateStore.setClaudeCliUpdateInfo(info)
    sidebarStore.setClaudeCliUpdateInfo(info)
  } catch (err) {
    cliError.value = true
    cliErrorMessage.value = String(err)
  } finally {
    cliChecking.value = false
  }
}

async function handleInstallClaudeCli() {
  // 检测是否有 claude 进程在运行
  try {
    const running = await checkClaudeRunning()
    if (running) {
      showCliConfirm.value = true
      return
    }
  } catch {}

  startCliDownload()
}

async function confirmCliUpdate() {
  showCliConfirm.value = false
  // 先停止所有 PTY Tab，避免杀死 claude 后终端残留不可用
  await ptyKillAll()
  // 再杀死全局所有 claude 进程（包括非 CC-Box 启动的）
  await killClaudeProcesses()
  startCliDownload()
}

async function startCliDownload() {
  updateStore.resetClaudeCliDownload()
  updateStore.setClaudeCliDownloadState('downloading')
  updateStore.setClaudeCliDownloadProgress(0, t('checking'))

  // 监听安装进度
  unlistenCliProgress = await onInstallProgress((progress) => {
    if (progress.item === 'claude') {
      if (progress.stage === 'done') {
        updateStore.setClaudeCliDownloadState('done')
      } else if (progress.stage === 'error') {
        updateStore.setClaudeCliDownloadError(progress.message)
        updateStore.setClaudeCliDownloadState('error')
      } else {
        updateStore.setClaudeCliDownloadProgress(progress.progress, progress.message)
      }
    }
  })

  try {
    await downloadAndInstallClaude()
    updateStore.setClaudeCliDownloadState('done')
    // 重新检查版本
    const info = await checkClaudeCliUpdate()
    updateStore.setClaudeCliUpdateInfo(info)
    sidebarStore.setClaudeCliUpdateInfo(info)
  } catch (err) {
    updateStore.setClaudeCliDownloadError(t('updateFailed', { error: String(err) }))
    updateStore.setClaudeCliDownloadState('error')
  } finally {
    unlistenCliProgress?.()
    unlistenCliProgress = null
  }
}

onUnmounted(() => {
  unlistenCliProgress?.()
})
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

.claude-cli-card {
  margin-top: 20px;
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

.not-installed {
  color: var(--text-tertiary);
  font-style: italic;
  font-weight: 400;
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

.action-row {
  display: flex;
  gap: 12px;
  align-items: center;
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
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
}

.action-btn.secondary:hover {
  border-color: var(--accent-color);
  color: var(--accent-color);
}

.progress-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.progress-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 13px;
  color: var(--text-secondary);
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

.error-actions {
  display: flex;
  gap: 12px;
  margin-top: 8px;
}

.retry-link {
  background: none;
  border: none;
  color: var(--accent-color);
  cursor: pointer;
  font-size: 13px;
  text-decoration: underline;
  padding: 0;
}

.confirm-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.3);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 300;
}

.confirm-dialog {
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  padding: 24px;
  min-width: 320px;
  box-shadow: var(--shadow-xl);
}

.confirm-text {
  font-size: 14px;
  font-weight: 500;
  color: var(--text-primary);
  margin: 0 0 20px 0;
}

.confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

.confirm-actions .btn-cancel {
  padding: 7px 18px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: 13px;
  cursor: pointer;
  font-family: var(--font-sans);
}

.confirm-actions .btn-cancel:hover {
  background: var(--hover-bg);
}

.confirm-actions .btn-primary {
  padding: 7px 18px;
  background: var(--accent-color);
  color: #fff;
  border: none;
  border-radius: var(--radius-md);
  font-size: 13px;
  cursor: pointer;
  font-family: var(--font-sans);
}

.confirm-actions .btn-primary:hover {
  opacity: 0.9;
}
</style>
