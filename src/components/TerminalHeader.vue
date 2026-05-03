<template>
  <header class="terminal-header">
    <span v-if="!isMac" class="project-name">{{ projectName }}</span>
    <div class="header-right">
      <button class="header-btn" @click="snapWindow('left')" title="Snap to left half (Ctrl+Shift+←)">
        <img src="@/assets/icons/half-left.svg" alt="Snap left" />
      </button>
      <button class="header-btn" @click="snapWindow('right')" title="Snap to right half (Ctrl+Shift+→)">
        <img src="@/assets/icons/half-right.svg" alt="Snap right" />
      </button>
      <button class="header-btn home-btn" @click="$emit('back')" title="Back to projects (Ctrl+Shift+H)">
        <img src="@/assets/icons/home.svg" alt="Home" />
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
import { snapWindow } from '@/composables/useAppShortcuts'
import { isMac } from '@/utils/platform'

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
  justify-content: center;
  height: 38px;
  padding: 0 12px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
  position: relative;
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
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 300px;
}

.header-right {
  position: absolute;
  right: 12px;
  display: flex;
  align-items: center;
  gap: 2px;
}
</style>