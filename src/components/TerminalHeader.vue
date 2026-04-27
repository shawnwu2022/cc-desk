<template>
  <header class="terminal-header">
    <span class="project-name">{{ projectName }}</span>
    <div class="header-right">
      <button class="header-btn" @click="snapLeft" title="Snap to left half">
        <img src="@/assets/icons/half-left.svg" alt="Snap left" />
      </button>
      <button class="header-btn" @click="snapRight" title="Snap to right half">
        <img src="@/assets/icons/half-right.svg" alt="Snap right" />
      </button>
      <button class="header-btn home-btn" @click="$emit('back')" title="Back to projects">
        <img src="@/assets/icons/home.svg" alt="Home" />
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
import { getCurrentWindow, currentMonitor } from '@tauri-apps/api/window'
import { PhysicalSize, PhysicalPosition } from '@tauri-apps/api/dpi'

defineProps<{
  projectName: string
}>()

defineEmits<{
  back: []
  sidebar: []
}>()

async function snapLeft() {
  try {
    const win = getCurrentWindow()
    const monitor = await currentMonitor()
    if (!monitor) return

    // 预留底部空间（任务栏 + 窗口边框）
    const bottomMargin = 100
    const halfWidth = Math.floor(monitor.size.width / 2)
    const height = monitor.size.height - bottomMargin

    await win.setPosition(new PhysicalPosition(monitor.position.x, monitor.position.y))
    await win.setSize(new PhysicalSize(halfWidth, height))
  } catch (err) {
    console.error('Failed to snap left:', err)
  }
}

async function snapRight() {
  try {
    const win = getCurrentWindow()
    const monitor = await currentMonitor()
    if (!monitor) return

    // 预留底部空间（任务栏 + 窗口边框）
    const bottomMargin = 100
    const halfWidth = Math.floor(monitor.size.width / 2)
    const height = monitor.size.height - bottomMargin

    await win.setPosition(new PhysicalPosition(monitor.position.x + halfWidth, monitor.position.y))
    await win.setSize(new PhysicalSize(halfWidth, height))
  } catch (err) {
    console.error('Failed to snap right:', err)
  }
}
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