import { getCurrentWindow, currentMonitor } from '@tauri-apps/api/window'
import { LogicalSize, LogicalPosition } from '@tauri-apps/api/dpi'
import { listen, UnlistenFn } from '@tauri-apps/api/event'
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

function isTerminalVisible(): boolean {
  const terminalView = document.querySelector('[data-terminal-view]')
  return terminalView !== null && terminalView.checkVisibility()
}

function emitTerminalAction(action: 'newSession' | 'restartSession' | 'backToProjects') {
  window.dispatchEvent(new CustomEvent(`terminal:${action}`))
}

export function useAppShortcuts() {
  const appStore = useAppStore()
  const sessionStore = useSessionStore()
  const sidebarStore = useSidebarStore()

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

  async function setupShortcutListeners(): Promise<UnlistenFn[]> {
    const unlisteners: UnlistenFn[] = []

    // 全局快捷键（不需要终端可见）
    unlisteners.push(
      await listen('shortcut:toggle-settings', () => sidebarStore.toggleSettings())
    )
    unlisteners.push(
      await listen('shortcut:new-instance', () => openNewAppInstance())
    )
    unlisteners.push(
      await listen('shortcut:snap-left', () => snapWindow('left'))
    )
    unlisteners.push(
      await listen('shortcut:snap-right', () => snapWindow('right'))
    )
    unlisteners.push(
      await listen('shortcut:restart-app', async () => {
        try { await ptyKillAll() } catch { /* ignore */ }
        window.location.reload()
      })
    )
    unlisteners.push(
      await listen('shortcut:font-increase', () => appStore.setFontSize(appStore.fontSize + 1))
    )
    unlisteners.push(
      await listen('shortcut:font-decrease', () => appStore.setFontSize(appStore.fontSize - 1))
    )
    unlisteners.push(
      await listen('shortcut:font-reset', () => appStore.setFontSize(12))
    )

    // 终端视图专属快捷键
    unlisteners.push(
      await listen('shortcut:new-session', () => {
        if (!isTerminalVisible()) return
        emitTerminalAction('newSession')
      })
    )
    unlisteners.push(
      await listen('shortcut:restart-session', () => {
        if (!isTerminalVisible()) return
        emitTerminalAction('restartSession')
      })
    )
    unlisteners.push(
      await listen('shortcut:tab-prev', () => {
        if (!isTerminalVisible()) return
        switchTab('prev')
      })
    )
    unlisteners.push(
      await listen('shortcut:tab-next', () => {
        if (!isTerminalVisible()) return
        switchTab('next')
      })
    )
    unlisteners.push(
      await listen('shortcut:back-to-projects', () => {
        if (!isTerminalVisible()) return
        emitTerminalAction('backToProjects')
      })
    )

    return unlisteners
  }

  return { setupShortcutListeners }
}
