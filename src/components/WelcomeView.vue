<template>
  <div class="welcome-view">
    <div class="welcome-content">
      <div class="logo">
        <svg width="80" height="80" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M12 2L2 7l10 5 10-5-10-5z"/>
          <path d="M2 17l10 5 10-5"/>
          <path d="M2 12l10 5 10-5"/>
        </svg>
      </div>
      <h1>CC Desk</h1>
      <p class="subtitle">{{ t('nativeWorkspaceSubtitle') }}</p>

      <div class="welcome-actions">
        <button class="select-btn" @click="emit('openNative')">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <rect x="3" y="4" width="18" height="16" rx="2"/>
            <path d="m7 9 3 3-3 3"/>
            <line x1="13" y1="15" x2="17" y2="15"/>
          </svg>
          {{ t('openNativeCliWorkspace') }}
        </button>
        <p class="hint">{{ t('nativeWorkspaceHint') }}</p>

        <button class="legacy-btn" @click="handleSelectProject">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
          </svg>
          {{ t('openLegacyClaudeWorkspace') }}
        </button>
        <p class="legacy-hint">{{ t('legacyClaudeWorkspaceHint') }}</p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const emit = defineEmits<{
  selectProject: []
  openNative: []
}>()

function handleSelectProject() {
  emit('selectProject')
}
</script>

<style scoped>
.welcome-view {
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-primary);
}

.welcome-content {
  text-align: center;
  max-width: 460px;
  padding: 24px;
}

.logo {
  margin-bottom: 24px;
  color: var(--accent-primary);
}

.logo svg {
  filter: drop-shadow(0 2px 8px rgba(30, 58, 95, 0.15));
}

h1 {
  font-size: 32px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 8px;
  letter-spacing: -0.5px;
}

.subtitle {
  font-size: 14px;
  color: var(--text-secondary);
  margin-bottom: 32px;
}

.welcome-actions {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 10px;
}

.select-btn,
.legacy-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 12px 24px;
  border-radius: var(--radius-lg);
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: background-color 0.15s ease, color 0.15s ease, border-color 0.15s ease, opacity 0.15s ease, transform 0.15s ease, box-shadow 0.15s ease;
}

.select-btn {
  background: var(--accent-primary);
  color: var(--text-inverse);
  border: none;
  box-shadow: var(--shadow-md);
}

.select-btn:hover {
  background: var(--accent-secondary);
  transform: translateY(-1px);
  box-shadow: var(--shadow-lg);
}

.select-btn:active,
.legacy-btn:active {
  transform: translateY(0);
}

.legacy-btn {
  margin-top: 8px;
  background: transparent;
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
}

.legacy-btn:hover {
  color: var(--text-primary);
  border-color: var(--accent-primary);
  background: var(--hover-bg);
}

.hint,
.legacy-hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-tertiary);
}

.legacy-hint {
  font-size: 11px;
}
</style>