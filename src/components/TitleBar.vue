<template>
  <header class="title-bar" data-tauri-drag-region @dblclick="!isMac && handleDblClick()">
    <!-- macOS 红绿灯占位（系统原生绘制，此处仅预留空间） -->
    <div v-if="isMac" class="traffic-light-spacer"></div>

    <!-- Windows 左侧图标和标题 -->
    <div class="win-title-left">
      <img v-if="!isMac" src="@/assets/icons/app-icon.png" alt="" class="win-app-icon" />
      <span class="win-app-title">{{ title }}</span>
    </div>

    <!-- Windows 窗口控制按钮 -->
    <div v-if="isWindows" class="window-controls">
      <button class="win-ctrl-btn" data-window-action="minimize" :aria-label="t('windowMinimize')" @click.stop="handleMinimize" @dblclick.stop>
        <svg width="10" height="1" viewBox="0 0 10 1">
          <rect width="10" height="1" fill="currentColor"/>
        </svg>
      </button>
      <button class="win-ctrl-btn" data-window-action="maximize" :aria-label="t(isMaximized ? 'windowRestore' : 'windowMaximize')" @click.stop="handleMaximize" @dblclick.stop>
        <svg v-if="!isMaximized" width="10" height="10" viewBox="0 0 10 10">
          <rect x="0.5" y="0.5" width="9" height="9" rx="1" fill="none" stroke="currentColor" stroke-width="1"/>
        </svg>
        <svg v-else width="10" height="10" viewBox="0 0 10 10">
          <rect x="2.5" y="0.5" width="7" height="7" rx="1" fill="none" stroke="currentColor" stroke-width="1"/>
          <rect x="0.5" y="2.5" width="7" height="7" rx="1" fill="var(--bg-secondary)" stroke="currentColor" stroke-width="1"/>
        </svg>
      </button>
      <button class="win-ctrl-btn win-close-btn" data-window-action="close" :aria-label="t('close')" @click.stop="handleClose" @dblclick.stop>
        <svg width="10" height="10" viewBox="0 0 10 10">
          <line x1="0.7" y1="0.7" x2="9.3" y2="9.3" stroke="currentColor" stroke-width="1"/>
          <line x1="9.3" y1="0.7" x2="0.7" y2="9.3" stroke="currentColor" stroke-width="1"/>
        </svg>
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isMac, isWindows } from '@/utils/platform'
withDefaults(defineProps<{ title?: string }>(), { title: 'CC Desk' })
const { t } = useI18n()
const win = getCurrentWindow()
const isMaximized = ref(false)
let mounted = true

async function handleMinimize() {
  await win.minimize()
}

async function handleMaximize() {
  await win.toggleMaximize()
}

async function handleClose() {
  await win.close()
}

async function handleDblClick() {
  await win.toggleMaximize()
}

let unlistenResize: (() => void) | null = null

onMounted(async () => {
  isMaximized.value = await win.isMaximized()

  if (!mounted) return
  const unlisten = await win.onResized(async () => {
    const maximized = await win.isMaximized()
    if (mounted) isMaximized.value = maximized
  })
  if (mounted) unlistenResize = unlisten
  else unlisten()
})

onUnmounted(() => {
  mounted = false
  unlistenResize?.()
})
</script>

<style scoped>
.title-bar {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  height: 32px;
  flex-shrink: 0;
  background: var(--bg-secondary);
  user-select: none;
  -webkit-user-select: none;
}

.win-title-left {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-right: auto;
  padding-left: 12px;
  min-width: 0;
  flex: 1;
}

.win-app-icon {
  width: 16px;
  height: 16px;
}

.win-app-title {
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
}

/* macOS 红绿灯占位（系统原生绘制，仅预留空间避免内容重叠） */
.traffic-light-spacer {
  width: 78px;
  flex-shrink: 0;
  margin-right: 0;
}

/* ===== Windows 窗口控制 =====
 * 规格: learn.microsoft.com/en-us/windows/apps/design/basics/titlebar-design
 */

.window-controls {
  display: flex;
  height: 100%;
  flex-shrink: 0;
}

.win-ctrl-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 46px;
  height: 100%;
  border: none;
  background: transparent;
  color: var(--text-primary);
  cursor: default;
  padding: 0;
  margin: 0;
}

.win-ctrl-btn:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }

.win-ctrl-btn:hover {
  background: rgba(0, 0, 0, 0.05);
}

.win-ctrl-btn:active {
  background: rgba(0, 0, 0, 0.08);
}

.win-close-btn:hover {
  background: #c42b1c;
  color: white;
}

.win-close-btn:active {
  background: #b42a1a;
  color: white;
}
</style>
