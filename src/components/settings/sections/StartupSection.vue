<template>
  <div class="section-content">
    <h2 class="section-heading">Startup Defaults</h2>
    <p class="section-desc">Default arguments applied when starting a new Claude session.</p>

    <div class="setting-group">
      <label class="setting-card">
        <div class="card-left">
          <input type="checkbox" v-model="skipPermissions" />
          <div class="card-info">
            <span class="setting-label">Skip permissions</span>
            <span class="setting-hint">Auto-accept all tool permissions</span>
          </div>
        </div>
        <code class="flag-badge warning">--allow-dangerously-skip-permissions</code>
      </label>

      <div class="setting-card text-card">
        <div class="card-info">
          <span class="setting-label">Custom arguments</span>
          <span class="setting-hint">Additional CLI flags appended to every new session</span>
        </div>
        <input
          type="text"
          v-model="customArgs"
          placeholder="e.g. --model sonnet"
          class="text-input"
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { useAppStore } from '@/stores/app'
import { saveDefaultClaudeOptions } from '@/api/tauri'

const appStore = useAppStore()
const skipPermissions = ref(appStore.claudeOptions.skipPermissions)
const customArgs = ref(appStore.claudeOptions.customArgs)

watch([skipPermissions, customArgs], () => {
  appStore.setClaudeOptions({
    skipPermissions: skipPermissions.value,
    customArgs: customArgs.value
  })
  saveDefaultClaudeOptions({
    skipPermissions: skipPermissions.value,
    customArgs: customArgs.value
  })
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
  margin-bottom: 8px;
}

.section-desc {
  font-size: 13px;
  color: var(--text-secondary);
  margin-bottom: 24px;
}

.setting-group {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.setting-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 16px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  cursor: pointer;
  transition: border-color 0.15s ease;
}

.setting-card:hover {
  border-color: var(--accent-color);
}

.card-left {
  display: flex;
  align-items: center;
  gap: 12px;
}

.card-left input[type="checkbox"] {
  width: 16px;
  height: 16px;
  accent-color: var(--accent-color);
  cursor: pointer;
}

.card-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.setting-label {
  font-size: 14px;
  font-weight: 500;
  color: var(--text-primary);
}

.setting-hint {
  font-size: 12px;
  color: var(--text-tertiary);
}

.flag-badge {
  font-size: 11px;
  padding: 2px 8px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-family: var(--font-mono);
  color: var(--text-secondary);
  white-space: nowrap;
}

.flag-badge.warning {
  color: var(--error-color);
  border-color: rgba(239, 68, 68, 0.3);
}

.text-card {
  flex-direction: column;
  align-items: flex-start;
  cursor: default;
}

.text-input {
  width: 100%;
  margin-top: 8px;
  padding: 8px 12px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 13px;
  font-family: var(--font-mono);
  color: var(--text-primary);
  transition: border-color 0.15s ease;
}

.text-input:focus {
  outline: none;
  border-color: var(--accent-color);
}

.text-input::placeholder {
  color: var(--text-tertiary);
}
</style>
