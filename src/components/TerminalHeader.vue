<template>
  <header class="terminal-header">
    <div class="header-left"></div>
    <span class="project-name">{{ projectName }}</span>
    <div class="header-right">
      <button class="header-btn home-btn" @click="$emit('back')" title="Back to projects">
        <img src="@/assets/icons/home.svg" alt="Home" />
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
defineProps<{
  projectName: string
}>()

defineEmits<{
  back: []
  sidebar: []
}>()
</script>

<style scoped>
.terminal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 38px;
  padding: 0 12px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
  -webkit-app-region: drag;
}

.header-left {
  display: flex;
  align-items: center;
  gap: 4px;
  -webkit-app-region: no-drag;
}

.header-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 6px;
  transition: all 0.15s ease;
  -webkit-app-region: no-drag;
}

.header-btn img {
  width: 18px;
  height: 18px;
}

.header-btn:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.project-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 300px;
}

.header-right {
  width: 32px;
}
</style>