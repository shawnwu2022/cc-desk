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

          <button
            class="download-btn"
            @click="openExternal(updateInfo.downloadUrl)"
          >
            Download on GitHub
          </button>
        </div>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { open } from '@tauri-apps/plugin-shell'
import { checkForUpdates } from '@/api/tauri'
import type { UpdateInfo } from '@/types'

const currentVersion = __APP_VERSION__
const checking = ref(false)
const error = ref(false)
const updateInfo = ref<UpdateInfo | null>(null)

const renderedNotes = computed(() => {
  if (!updateInfo.value?.releaseNotes) return ''
  return updateInfo.value.releaseNotes
    .replace(/\n/g, '<br>')
    .replace(/#{1,3}\s(.+)/g, '<strong>$1</strong>')
})

function openExternal(url: string) {
  open(url)
}

async function handleCheckUpdate() {
  checking.value = true
  error.value = false
  try {
    updateInfo.value = await checkForUpdates()
  } catch {
    error.value = true
  } finally {
    checking.value = false
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

.download-btn {
  display: inline-block;
  padding: 10px 20px;
  background: var(--accent-color);
  color: white;
  text-decoration: none;
  border-radius: 6px;
  font-size: 14px;
  font-weight: 500;
  transition: opacity 0.15s ease;
}

.download-btn:hover {
  opacity: 0.9;
}
</style>
