import { getCurrentWindow, currentMonitor } from '@tauri-apps/api/window'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { LogicalSize, LogicalPosition } from '@tauri-apps/api/dpi'
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import { useSidebarStore } from '@/stores/sidebar'
import { ptyKillAll, spawnNewInstance } from '@/api/tauri'

export async function snapWindow(side: 'left' | 'right') {
  try {
    const win = getCurrentWindow()
    const monitor = await currentMonitor()
    if (!monitor) return

    const scaleFactor = monitor.scaleFactor
    const halfWidth = Math.floor(monitor.size.width / scaleFactor / 2)
    const height = window.screen.availHeight - 21
    const x = side === 'left'
      ? monitor.position.x / scaleFactor
      : monitor.position.x / scaleFactor + halfWidth
    const y = monitor.position.y / scaleFactor

    await win.setPosition(new LogicalPosition(x, y))
    await win.setSize(new LogicalSize(halfWidth, height))
  } catch (err) {
    console.error(`Failed to snap ${side}:`, err)
  }
}

export async function openNewAppInstance() {
  try {
    await spawnNewInstance()
  } catch (err) {
    console.error('Failed to open new instance:', err)
  }
}

export function useAppShortcuts() {
  const appStore = useAppStore()
  const sessionStore = useSessionStore()
  const sidebarStore = useSidebarStore()

  let unlistenFocus: (() => void) | null = null

  // 检查终端视图是否可见
  function isTerminalVisible(): boolean {
    const terminalView = document.querySelector('[data-terminal-view]')
    return terminalView !== null && terminalView.checkVisibility()
  }

  // 发送终端操作事件（由 TerminalView 监听）
  function emitTerminalAction(action: 'newSession' | 'restartSession' | 'backToProjects') {
    window.dispatchEvent(new CustomEvent(`terminal:${action}`))
  }

  function switchTab(direction: 'next' | 'prev') {
    const cwd = appStore.cwd
    if (!cwd) return
    const tabs = sessionStore.getProjectTabs(cwd)
    if (tabs.length < 2) return

    const currentId = sessionStore.activeTabId
    const currentIndex = tabs.findIndex(t => t.tabId === currentId)

    let nextIndex: number
    if (currentIndex === -1) {
      nextIndex = 0
    } else if (direction === 'next') {
      nextIndex = (currentIndex + 1) % tabs.length
    } else {
      nextIndex = (currentIndex - 1 + tabs.length) % tabs.length
    }

    sessionStore.setActiveTab(tabs[nextIndex].tabId)
  }

  async function handleKeydown(e: KeyboardEvent) {
    const ctrl = e.ctrlKey || e.metaKey
    const shift = e.shiftKey
    const alt = e.altKey
    const key = e.key.toLowerCase()
    // console.log(ctrl, shift, alt, key);

    // Alt+N — 新建会话（仅终端视图有效）
    if (alt && key === 'n' && !ctrl && !shift) {
      if (!isTerminalVisible()) return
      e.preventDefault()
      e.stopPropagation()
      emitTerminalAction('newSession')
      return
    }

    // Alt+R — 重启会话（仅终端视图有效）
    if (alt && key === 'r' && !ctrl && !shift) {
      if (!isTerminalVisible()) return
      e.preventDefault()
      e.stopPropagation()
      emitTerminalAction('restartSession')
      return
    }

    // Alt+ArrowDown — 切换到下一个 tab（仅终端视图有效）
    if (alt && key === 'arrowdown' && !ctrl && !shift) {
      if (!isTerminalVisible()) return
      e.preventDefault()
      e.stopPropagation()
      switchTab('next')
      return
    }

    // Alt+ArrowUp — 切换到上一个 tab（仅终端视图有效）
    if (alt && key === 'arrowup' && !ctrl && !shift) {
      if (!isTerminalVisible()) return
      e.preventDefault()
      e.stopPropagation()
      switchTab('prev')
      return
    }

    // Ctrl+Shift+N — 新建应用实例（全局有效）
    if (ctrl && shift && key === 'n') {
      e.preventDefault()
      e.stopPropagation()
      await openNewAppInstance()
      return
    }

    // Ctrl+Shift+← — 窗口左移半屏（全局有效）
    if (ctrl && shift && key === 'arrowleft') {
      e.preventDefault()
      e.stopPropagation()
      await snapWindow('left')
      return
    }

    // Ctrl+Shift+→ — 窗口右移半屏（全局有效）
    if (ctrl && shift && key === 'arrowright') {
      e.preventDefault()
      e.stopPropagation()
      await snapWindow('right')
      return
    }

    // Ctrl+Shift+R — 重启应用（全局有效）
    if (ctrl && shift && key === 'r') {
      e.preventDefault()
      e.stopPropagation()
      try { await ptyKillAll() } catch { /* ignore */ }
      window.location.reload()
      return
    }

    // Ctrl+Shift+H — 回到 Project Select（仅终端视图有效）
    if (ctrl && shift && key === 'h') {
      if (!isTerminalVisible()) return
      e.preventDefault()
      e.stopPropagation()
      emitTerminalAction('backToProjects')
      return
    }

    // Ctrl+, — 打开设置（全局有效）
    if (ctrl && key === ',') {
      e.preventDefault()
      e.stopPropagation()
      sidebarStore.openSettings()
      return
    }

    // Ctrl+Plus / Ctrl+= — 增大字体（全局有效）
    if (ctrl && (key === '+' || key === '=')) {
      e.preventDefault()
      e.stopPropagation()
      appStore.setFontSize(appStore.fontSize + 1)
      return
    }

    // Ctrl+Minus — 缩小字体（全局有效）
    if (ctrl && key === '-') {
      e.preventDefault()
      e.stopPropagation()
      appStore.setFontSize(appStore.fontSize - 1)
      return
    }

    // Ctrl+0 — 重置字体（全局有效）
    if (ctrl && key === '0') {
      e.preventDefault()
      e.stopPropagation()
      appStore.setFontSize(12)
      return
    }
  }

  async function setupFocusRecovery() {
    const win = getCurrentWindow()
    unlistenFocus = await win.onFocusChanged(({ payload: focused }) => {
      if (focused) {
        // 窗口获得焦点时，恢复 webview 焦点
        getCurrentWebview().setFocus().catch(() => {})
      }
    })
  }

  function cleanup() {
    unlistenFocus?.()
  }

  return { handleKeydown, setupFocusRecovery, cleanup, isTerminalVisible }
}
