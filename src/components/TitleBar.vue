<template>
  <header class="title-bar" data-tauri-drag-region @dblclick="handleDblClick">
    <!-- macOS 红绿灯 (SVG 来自 macos-traffic-lights 包) -->
    <div v-if="showMac" class="traffic-light-spacer">
      <div class="traffic-lights" @mouseenter="isHovered = true" @mouseleave="isHovered = false; closePressed = false; minPressed = false; maxPressed = false">
        <button class="mac-light" @click.stop="handleClose" @dblclick.stop
          @mousedown="closePressed = true" @mouseup="closePressed = false">
          <img :src="getLightSrc('close', closePressed)" />
        </button>
        <button class="mac-light" @click.stop="handleMinimize" @dblclick.stop
          @mousedown="minPressed = true" @mouseup="minPressed = false">
          <img :src="getLightSrc('minimize', minPressed)" />
        </button>
        <button class="mac-light" @click.stop="handleMaximize" @dblclick.stop
          @mousedown="maxPressed = true" @mouseup="maxPressed = false">
          <img :src="getLightSrc('maximize', maxPressed)" />
        </button>
      </div>
    </div>

    <!-- Windows 窗口控制按钮 -->
    <div v-if="showWin" class="window-controls">
      <button class="win-ctrl-btn" @click.stop="handleMinimize" @dblclick.stop>
        <svg width="10" height="1" viewBox="0 0 10 1">
          <rect width="10" height="1" fill="currentColor"/>
        </svg>
      </button>
      <button class="win-ctrl-btn" @click.stop="handleMaximize" @dblclick.stop>
        <svg v-if="!isMaximized" width="10" height="10" viewBox="0 0 10 10">
          <rect x="0.5" y="0.5" width="9" height="9" rx="1" fill="none" stroke="currentColor" stroke-width="1"/>
        </svg>
        <svg v-else width="10" height="10" viewBox="0 0 10 10">
          <rect x="2.5" y="0.5" width="7" height="7" rx="1" fill="none" stroke="currentColor" stroke-width="1"/>
          <rect x="0.5" y="2.5" width="7" height="7" rx="1" fill="var(--bg-secondary)" stroke="currentColor" stroke-width="1"/>
        </svg>
      </button>
      <button class="win-ctrl-btn win-close-btn" @click.stop="handleClose" @dblclick.stop>
        <svg width="10" height="10" viewBox="0 0 10 10">
          <line x1="0.7" y1="0.7" x2="9.3" y2="9.3" stroke="currentColor" stroke-width="1"/>
          <line x1="9.3" y1="0.7" x2="0.7" y2="9.3" stroke="currentColor" stroke-width="1"/>
        </svg>
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isMac, isWindows } from '@/utils/platform'

import unfocusedSrc from '@/assets/macos-lights/unfocused.svg'
import closeDefaultSrc from '@/assets/macos-lights/close/default.svg'
import closeHoverSrc from '@/assets/macos-lights/close/hover.svg'
import closeActiveSrc from '@/assets/macos-lights/close/active.svg'
import minimizeDefaultSrc from '@/assets/macos-lights/minimize/default.svg'
import minimizeHoverSrc from '@/assets/macos-lights/minimize/hover.svg'
import minimizeActiveSrc from '@/assets/macos-lights/minimize/active.svg'
import maximizeDefaultSrc from '@/assets/macos-lights/maximize/default.svg'
import maximizeHoverSrc from '@/assets/macos-lights/maximize/hover.svg'
import maximizeActiveSrc from '@/assets/macos-lights/maximize/active.svg'

const isDev = import.meta.env.DEV
const win = getCurrentWindow()
const isMaximized = ref(false)
const isFocused = ref(true)
const isHovered = ref(false)
const closePressed = ref(false)
const minPressed = ref(false)
const maxPressed = ref(false)

const showMac = computed(() => isDev || isMac)
const showWin = computed(() => isDev || isWindows)

const lights = {
  close: { default: closeDefaultSrc, hover: closeHoverSrc, active: closeActiveSrc },
  minimize: { default: minimizeDefaultSrc, hover: minimizeHoverSrc, active: minimizeActiveSrc },
  maximize: { default: maximizeDefaultSrc, hover: maximizeHoverSrc, active: maximizeActiveSrc },
}

function getLightSrc(name: keyof typeof lights, pressed: boolean): string {
  const light = lights[name]
  if (pressed) return light.active
  if (isHovered.value) return light.hover
  if (isFocused.value) return light.default
  return unfocusedSrc
}

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
let unlistenFocus: (() => void) | null = null

onMounted(async () => {
  isMaximized.value = await win.isMaximized()
  isFocused.value = await win.isFocused()

  unlistenResize = await win.onResized(async () => {
    isMaximized.value = await win.isMaximized()
  })

  unlistenFocus = await win.onFocusChanged(({ payload: focused }) => {
    isFocused.value = focused
  })
})

onUnmounted(() => {
  unlistenResize?.()
  unlistenFocus?.()
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

/* ===== macOS 红绿灯 =====
 * SVG 来源: macos-traffic-lights npm 包 (github.com/aw3r1se/macOS-traffic-lights)
 * 每张 SVG 包含完整圆形按钮 (86×86 viewBox → 12×12 渲染)
 * 四态: unfocused(灰) → focused(彩色无图标) → hover(彩色+图标) → active(按下)
 */

.traffic-light-spacer {
  display: flex;
  align-items: center;
  padding-left: 12px;
  flex-shrink: 0;
  margin-right: auto;
}

.traffic-lights {
  display: flex;
  align-items: center;
  gap: 8px;
}

.mac-light {
  padding: 0;
  border: none;
  outline: none;
  cursor: default;
  background: transparent;
  width: 12px;
  height: 12px;
}

.mac-light img {
  width: 12px;
  height: 12px;
  pointer-events: none;
  display: block;
}

/* ===== Windows 窗口控制 =====
 * 规格: learn.microsoft.com/en-us/windows/apps/design/basics/titlebar-design
 */

.window-controls {
  display: flex;
  height: 100%;
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
  outline: none;
}

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
