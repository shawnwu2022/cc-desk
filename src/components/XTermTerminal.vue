<template>
  <div ref="containerRef" class="xterm-container">
    <!-- 动态渲染每个 Tab 的终端容器 -->
    <div
      v-for="[tabId] in terminalInstances"
      :key="tabId"
      :ref="(el: Element | ComponentPublicInstance | null) => setTerminalEl(tabId, el as HTMLElement | null)"
      :data-tab="tabId"
      class="terminal-wrapper"
      :class="{ active: tabId === currentDisplayTabId }"
    ></div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, watch, onMounted, onUnmounted, onActivated, nextTick, type ComponentPublicInstance } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import {
  ptySpawn,
  ptyInput,
  ptyResize,
  ptyKill,
  onPtyOutput,
  onPtyExit,
  onTerminalKeydown,
} from '@/api/tauri'
import { registerTerminalCommand } from '@/composables/useTerminalCommand'
import { readText } from '@tauri-apps/plugin-clipboard-manager'

const props = defineProps<{
  fontSize?: number
}>()

const emit = defineEmits<{
  ptyStarted: [tabId: string, ptyId: string]
}>()

const appStore = useAppStore()
const sessionStore = useSessionStore()
const containerRef = ref<HTMLElement>()

// 等待 DOM 元素可用
async function waitForElement(tabId: string, timeout = 10000): Promise<HTMLElement | null> {
  const start = Date.now()
  while (Date.now() - start < timeout) {
    let el = terminalEls.get(tabId)
    if (el) return el

    el = containerRef.value?.querySelector(
      `.terminal-wrapper[data-tab="${tabId}"]`
    ) as HTMLElement
    if (el) {
      terminalEls.set(tabId, el)
      return el
    }

    await new Promise(r => requestAnimationFrame(r))
  }
  return null
}

// 每个 Tab 独立的 Terminal 实例（key 为 tabId）
const terminalInstances = reactive(new Map<string, {
  term: Terminal
  fitAddon: FitAddon
  ptyId: string
}>())

// Terminal DOM 元素引用
const terminalEls = reactive(new Map<string, HTMLElement | null>())

// 当前显示的 Tab ID
const currentDisplayTabId = ref<string | null>(null)

// 是否正在启动 PTY（防止并发）
const isPtyStarting = ref<boolean>(false)

// Unlisten functions
let unlistenPtyOutput: (() => void) | null = null
let unlistenPtyExit: (() => void) | null = null
let unlistenKeydown: (() => void) | null = null

// ResizeObserver
let resizeObserver: ResizeObserver | null = null

// 浅色终端主题
const lightTheme = {
  background: '#f8f9fa',
  foreground: '#1a1a2e',
  cursor: '#6c5ce7',
  cursorAccent: '#f8f9fa',
  selectionBackground: '#ede9fe',
  selectionForeground: '#1a1a2e',
  black: '#1a1a2e',
  red: '#e74c3c',
  green: '#27ae60',
  yellow: '#f39c12',
  blue: '#3498db',
  magenta: '#9b59b6',
  cyan: '#1abc9c',
  white: '#ecf0f1',
  brightBlack: '#2c3e50',
  brightRed: '#e74c3c',
  brightGreen: '#27ae60',
  brightYellow: '#f39c12',
  brightBlue: '#3498db',
  brightMagenta: '#9b59b6',
  brightCyan: '#1abc9c',
  brightWhite: '#ffffff'
}

// 设置 Terminal DOM 元素引用
function setTerminalEl(tabId: string, el: HTMLElement | null) {
  if (el) {
    terminalEls.set(tabId, el)
    const instance = terminalInstances.get(tabId)
    if (instance && !instance.term.element) {
      instance.term.open(el)
      if (tabId === currentDisplayTabId.value) {
        requestAnimationFrame(() => instance.fitAddon.fit())
      }
    }
  }
}

// Fit 当前显示的终端
function fitCurrentTerminal() {
  if (!currentDisplayTabId.value) return
  const instance = terminalInstances.get(currentDisplayTabId.value)
  if (instance) {
    requestAnimationFrame(() => instance.fitAddon.fit())
  }
}

// 创建新的 Terminal 实例
function createTerminal(tabId: string): Terminal {
  const term = new Terminal({
    fontFamily: 'Cascadia Code, Fira Code, Consolas, monospace',
    fontSize: props.fontSize ?? 14,
    lineHeight: 1.2,
    cursorBlink: true,
    cursorStyle: 'bar',
    theme: lightTheme,
    allowProposedApi: true,
    macOptionIsMeta: true,
  })

  const fitAddon = new FitAddon()
  term.loadAddon(fitAddon)

  // 用户输入 → 发送到对应的 PTY
  term.onData(data => {
    const instance = terminalInstances.get(tabId)
    if (instance) {
      ptyInput(instance.ptyId, data)
    }
  })

  // 终端尺寸变化 → resize 对应的 PTY
  term.onResize(({ cols, rows }) => {
    const instance = terminalInstances.get(tabId)
    if (instance) {
      ptyResize(instance.ptyId, cols, rows)
    }
  })

  // Ctrl+V 粘贴
  term.attachCustomKeyEventHandler((event: KeyboardEvent) => {
    if (event.type === 'keydown' && event.ctrlKey && event.key === 'v') {
      event.preventDefault()
      readText().then(text => {
        if (text) term.paste(text)
      }).catch(() => {})
      return false
    }
    return true
  })

  return term
}

onMounted(async () => {
  await setupEventListeners()

  if (containerRef.value) {
    resizeObserver = new ResizeObserver(() => fitCurrentTerminal())
    resizeObserver.observe(containerRef.value)
  }

  registerTerminalCommand(sendText)
})

// 设置事件监听器
async function setupEventListeners() {
  // PTY 输出 → 写入对应 Tab 的 Terminal + 触发会话匹配
  unlistenPtyOutput = await onPtyOutput(({ id, data }) => {
    for (const [tabId, instance] of terminalInstances) {
      if (instance.ptyId === id) {
        instance.term.write(data)
        // PTY 有输出 → 触发即时匹配（仅对未关联 sessionId 的 Tab）
        sessionStore.triggerOutputDrivenMatch(tabId)
        break
      }
    }
  })

  // PTY 退出 → 更新 Tab 状态（不删除 Tab）
  unlistenPtyExit = await onPtyExit(({ id }) => {
    for (const [tabId, instance] of terminalInstances) {
      if (instance.ptyId === id) {
        // 停止匹配轮询
        sessionStore.stopMatchPolling(tabId)

        // 更新 store（Tab 保留，状态变 stopped）
        sessionStore.handlePtyExit(id)

        // 销毁 Terminal 实例（释放资源）
        instance.term.dispose()
        terminalInstances.delete(tabId)
        terminalEls.delete(tabId)
        break
      }
    }
  })

  // 快捷键处理（Ctrl+W 等）
  unlistenKeydown = await onTerminalKeydown(({ key, modifiers }) => {
    const instance = terminalInstances.get(currentDisplayTabId.value ?? '')
    if (instance && modifiers.includes('control') && key.toLowerCase() === 'w') {
      ptyInput(instance.ptyId, '\x17')
    }
  })
}

// 监听 fontSize 变化
watch(() => props.fontSize, (newSize) => {
  if (newSize) {
    for (const instance of terminalInstances.values()) {
      instance.term.options.fontSize = newSize
      instance.fitAddon.fit()
    }
  }
})

// 监听活跃 Tab 变化 → 切换显示
watch(() => sessionStore.activeTabId, async (newTabId, oldTabId) => {
  if (!newTabId) return

  if (newTabId === oldTabId) {
    fitCurrentTerminal()
    return
  }

  if (isPtyStarting.value) return

  currentDisplayTabId.value = newTabId

  await nextTick()

  const existingInstance = terminalInstances.get(newTabId)

  if (existingInstance) {
    requestAnimationFrame(() => existingInstance.fitAddon.fit())
  } else {
    const tab = sessionStore.tabs.get(newTabId)
    if (!tab) return

    if (tab.ptyId && tab.status === 'running') {
      // PTY 在运行但没有 Terminal 实例 → 创建
      await createTerminalForTab(newTabId, tab.ptyId)
    } else if (tab.status === 'stopped') {
      // 已停止的 Tab：不自动启动，等用户操作
    }
  }
})

// 为已有 PTY 的 Tab 创建 Terminal 实例
async function createTerminalForTab(tabId: string, ptyId: string) {
  if (terminalInstances.has(tabId)) return

  const term = createTerminal(tabId)
  const fitAddon = new FitAddon()
  term.loadAddon(fitAddon)

  terminalInstances.set(tabId, { term, fitAddon, ptyId })

  const el = await waitForElement(tabId)
  if (el) {
    term.open(el)
    if (tabId === currentDisplayTabId.value) {
      requestAnimationFrame(() => fitAddon.fit())
    }
  }
}

// ==================== Tab 操作（外部调用） ====================

/**
 * 启动 Tab 的 PTY
 * 由 TerminalView 调用，传入已创建好的 tabId
 */
async function startTab(tabId: string) {
  const tab = sessionStore.tabs.get(tabId)
  if (!tab || isPtyStarting.value) return

  // 已有运行中的 PTY
  if (tab.ptyId && tab.status === 'running') {
    if (!terminalInstances.has(tabId)) {
      await createTerminalForTab(tabId, tab.ptyId)
    }
    return
  }

  isPtyStarting.value = true

  try {
    const args = buildClaudeArgs(tab)
    const cwd = tab.projectPath

    const term = createTerminal(tabId)
    const fitAddon = new FitAddon()
    term.loadAddon(fitAddon)

    const info = await ptySpawn({
      cwd,
      cols: 80,
      rows: 24,
      type: 'claude',
      args,
    })

    if (info) {
      terminalInstances.set(tabId, { term, fitAddon, ptyId: info.id })
      sessionStore.setTabPty(tabId, info.id)
      sessionStore.setActiveTab(tabId)
      currentDisplayTabId.value = tabId

      const el = await waitForElement(tabId)
      if (el) {
        term.open(el)
        requestAnimationFrame(() => fitAddon.fit())
      }

      emit('ptyStarted', tabId, info.id)

      // 如果 Tab 没有 sessionId，启动轮询匹配
      if (!tab.sessionId) {
        sessionStore.startMatchPolling(tabId)
      }
    }
  } catch (err) {
    console.error('[XTerm] startTab ERROR:', err)
  } finally {
    isPtyStarting.value = false
  }
}

/**
 * 重启 Tab（停止的 Tab 恢复运行）
 */
async function restartTab(tabId: string) {
  const tab = sessionStore.tabs.get(tabId)
  if (!tab || isPtyStarting.value) return

  isPtyStarting.value = true

  try {
    // 如果有旧 PTY 在运行，先 kill
    if (tab.ptyId) {
      try { await ptyKill(tab.ptyId) } catch {}
    }

    // 清理旧 Terminal 实例
    const oldInstance = terminalInstances.get(tabId)
    if (oldInstance) {
      oldInstance.term.dispose()
      terminalInstances.delete(tabId)
      terminalEls.delete(tabId)
    }

    const args = buildClaudeArgs(tab)
    const cwd = tab.projectPath

    const term = createTerminal(tabId)
    const fitAddon = new FitAddon()
    term.loadAddon(fitAddon)

    const info = await ptySpawn({
      cwd,
      cols: 80,
      rows: 24,
      type: 'claude',
      args,
    })

    if (info) {
      terminalInstances.set(tabId, { term, fitAddon, ptyId: info.id })
      sessionStore.setTabPty(tabId, info.id)
      sessionStore.setActiveTab(tabId)
      currentDisplayTabId.value = tabId

      const el = await waitForElement(tabId)
      if (el) {
        term.open(el)
        requestAnimationFrame(() => fitAddon.fit())
      }

      emit('ptyStarted', tabId, info.id)

      // 如果仍无 sessionId，启动轮询匹配
      if (!tab.sessionId) {
        sessionStore.startMatchPolling(tabId)
      }
    }
  } catch (err) {
    console.error('[XTerm] restartTab ERROR:', err)
  } finally {
    isPtyStarting.value = false
  }
}

/**
 * 根据 Tab 状态构建 Claude CLI 参数
 */
function buildClaudeArgs(tab: { sessionId: string | null }): string[] {
  const args: string[] = []
  const opts = appStore.claudeOptions

  // 有 sessionId → --resume；无 → 新建会话（不带 --resume）
  if (tab.sessionId) {
    args.push('--resume', tab.sessionId)
  }

  if (opts.skipPermissions) args.push('--dangerously-skip-permissions')
  if (opts.customArgs) {
    const custom = opts.customArgs.trim().split(/\s+/).filter(Boolean)
    args.push(...custom)
  }

  return args
}

/**
 * 使用启动选项创建并启动 Tab（兼容旧的 startWithOptions 入口）
 */
async function startWithOptions(cwd: string, opts: {
  resume?: string
  skipPermissions?: boolean
  customArgs?: string
}) {
  if (isPtyStarting.value) return

  const tabId = sessionStore.createTab(cwd, {
    sessionId: opts.resume || undefined,
  })

  sessionStore.setActiveTab(tabId)
  await startTab(tabId)
  appStore.resetClaudeOptions()
}

/**
 * 新建会话（无参数模式）
 */
async function startNewSession(cwd: string) {
  if (isPtyStarting.value) return

  const tabId = sessionStore.createTab(cwd)
  sessionStore.setActiveTab(tabId)
  await startTab(tabId)
}

// 清理所有 PTY
async function cleanup() {
  for (const instance of terminalInstances.values()) {
    instance.term.dispose()
  }
  terminalInstances.clear()
  terminalEls.clear()
  await sessionStore.cleanupAll()
}

// 向活跃终端发送文字并聚焦
function sendText(text: string) {
  const tabId = currentDisplayTabId.value
  if (!tabId) return
  const instance = terminalInstances.get(tabId)
  if (instance) {
    ptyInput(instance.ptyId, text)
    instance.term.focus()
  }
}

// 聚焦活跃终端
function focus() {
  const tabId = currentDisplayTabId.value
  if (!tabId) return
  const instance = terminalInstances.get(tabId)
  if (instance) instance.term.focus()
}

// 兼容：重启当前活跃 Tab
async function restartCurrentPty() {
  if (sessionStore.activeTabId) {
    await restartTab(sessionStore.activeTabId)
  }
}

onActivated(() => {
  requestAnimationFrame(() => fitCurrentTerminal())
})

onUnmounted(() => {
  resizeObserver?.disconnect()
  unlistenPtyOutput?.()
  unlistenPtyExit?.()
  unlistenKeydown?.()
  for (const instance of terminalInstances.values()) {
    instance.term.dispose()
  }
})

defineExpose({
  startTab,
  restartTab,
  startWithOptions,
  startNewSession,
  restartCurrentPty,
  cleanup,
  sendText,
  focus,
})
</script>

<style scoped>
.xterm-container {
  width: 100%;
  height: 100%;
  padding: 8px;
  box-sizing: border-box;
  background: #f8f9fa;
  border-radius: 8px;
  position: relative;
  overflow: hidden;
}

.terminal-wrapper {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  padding: 8px;
  box-sizing: border-box;
  visibility: hidden;
  pointer-events: none;
}

.terminal-wrapper.active {
  visibility: visible;
  pointer-events: auto;
}

.terminal-wrapper :deep(.xterm) {
  height: 100%;
  padding: 4px;
}

.terminal-wrapper :deep(.xterm-viewport) {
  border-radius: 4px;
}

.terminal-wrapper :deep(.xterm-viewport::-webkit-scrollbar) {
  width: 8px;
}

.terminal-wrapper :deep(.xterm-viewport::-webkit-scrollbar-thumb) {
  background: #d1d5db;
  border-radius: 4px;
}

.terminal-wrapper :deep(.xterm-viewport::-webkit-scrollbar-track) {
  background: transparent;
}
</style>
