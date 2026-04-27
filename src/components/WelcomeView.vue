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
      <h1>Claude Code</h1>
      <p class="subtitle">AI-powered coding assistant</p>
      <button class="select-btn" @click="handleSelectProject">
        <img src="@/assets/icons/folder-plus.svg" alt="Select" />
        Select Project Directory
      </button>
      <p class="hint">Choose a directory to start a new session</p>
    </div>
  </div>
</template>

<script setup lang="ts">
const emit = defineEmits<{
  selectProject: []
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
  height: 100vh;
  background: var(--bg-primary);
}

.welcome-content {
  text-align: center;
  max-width: 400px;
}

.logo {
  margin-bottom: 24px;
  color: var(--accent-color);
}

h1 {
  font-size: 32px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 8px;
}

.subtitle {
  font-size: 14px;
  color: var(--text-secondary);
  margin-bottom: 32px;
}

.select-btn {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 12px 24px;
  background: var(--accent-color);
  color: #fff;
  border: none;
  border-radius: 8px;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s ease;
}

.select-btn img {
  width: 20px;
  height: 20px;
}

.select-btn:hover {
  background: #5b4cdb;
  transform: translateY(-1px);
}

.select-btn:active {
  transform: translateY(0);
}

.hint {
  margin-top: 16px;
  font-size: 12px;
  color: var(--text-secondary);
}
</style>