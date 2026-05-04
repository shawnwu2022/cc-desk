import { getCurrentWindow, currentMonitor } from '@tauri-apps/api/window'
import { LogicalSize, LogicalPosition } from '@tauri-apps/api/dpi'
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import { useSidebarStore } from '@/stores/sidebar'
import { ptyKillAll, spawnNewInstance } from '@/api/tauri'
import { isMac } from '@/utils/platform'

export async function snapWindow(side: 'left' | 'right') {
  try {
    const win = getCurrentWindow()
    const monitor = await currentMonitor()
    if (!monitor) return

    const scaleFactor = monitor.scaleFactor
    const halfWidth = Math.floor(monitor.size.width / scaleFactor / 2)
    const height = window.screen.availHeight
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

  function handleGlobalKeydown(e: KeyboardEvent) {
    const mod = isMac ? e.metaKey : e.ctrlKey

    // --- 全局快捷键（所有视图生效） ---

    // Cmd/Ctrl + , => toggle settings
    if (mod && e.key === ',') {
      e.preventDefault()
      e.stopPropagation()
      sidebarStore.toggleSettings()
      return
    }

    // Cmd/Ctrl + Shift + N => new instance
    if (mod && e.shiftKey && e.key === 'N') {
      e.preventDefault()
      e.stopPropagation()
      openNewAppInstance()
      return
    }

    // Cmd/Ctrl + Shift + Left => snap left
    if (mod && e.shiftKey && e.key === 'ArrowLeft') {
      e.preventDefault()
      e.stopPropagation()
      snapWindow('left')
      return
    }

    // Cmd/Ctrl + Shift + Right => snap right
    if (mod && e.shiftKey && e.key === 'ArrowRight') {
      e.preventDefault()
      e.stopPropagation()
      snapWindow('right')
      return
    }

    // Cmd/Ctrl + Shift + R => restart app
    if (mod && e.shiftKey && e.key === 'R') {
      e.preventDefault()
      e.stopPropagation()
      try { ptyKillAll() } catch { /* ignore */ }
      window.location.reload()
      return
    }

    // Cmd/Ctrl + = => font increase
    if (mod && e.key === '=' && !e.altKey) {
      e.preventDefault()
      e.stopPropagation()
      appStore.setFontSize(appStore.fontSize + 1)
      return
    }

    // Cmd/Ctrl + - => font decrease
    if (mod && e.key === '-' && !e.shiftKey) {
      e.preventDefault()
      e.stopPropagation()
      appStore.setFontSize(appStore.fontSize - 1)
      return
    }

    // Cmd/Ctrl + 0 => font reset
    if (mod && e.key === '0') {
      e.preventDefault()
      e.stopPropagation()
      appStore.setFontSize(12)
      return
    }

    // --- 终端视图专属快捷键 ---
    if (!isTerminalVisible()) return

    // Cmd/Ctrl + Shift + H => back to projects
    if (mod && e.shiftKey && e.key === 'H') {
      e.preventDefault()
      e.stopPropagation()
      emitTerminalAction('backToProjects')
      return
    }

    // Alt + N => new session
    if (e.altKey && !mod && e.code === 'KeyN') {
      e.preventDefault()
      e.stopPropagation()
      emitTerminalAction('newSession')
      return
    }

    // Alt + R => restart session
    if (e.altKey && !mod && e.code === 'KeyR') {
      e.preventDefault()
      e.stopPropagation()
      emitTerminalAction('restartSession')
      return
    }

    // Alt + Up => tab prev
    if (e.altKey && e.key === 'ArrowUp') {
      e.preventDefault()
      e.stopPropagation()
      switchTab('prev')
      return
    }

    // Alt + Down => tab next
    if (e.altKey && e.key === 'ArrowDown') {
      e.preventDefault()
      e.stopPropagation()
      switchTab('next')
      return
    }
  }

  function setupShortcutListeners(): (() => void)[] {
    window.addEventListener('keydown', handleGlobalKeydown, true)
    return [() => window.removeEventListener('keydown', handleGlobalKeydown, true)]
  }

  return { setupShortcutListeners }
}
