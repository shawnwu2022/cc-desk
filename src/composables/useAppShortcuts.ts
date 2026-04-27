import { getCurrentWindow } from '@tauri-apps/api/window'
import { useAppStore } from '@/stores/app'
import { useSidebarStore } from '@/stores/sidebar'
import { ptyKillAll } from '@/api/tauri'

export function useAppShortcuts() {
  const appStore = useAppStore()
  const sidebarStore = useSidebarStore()

  async function handleKeydown(e: KeyboardEvent) {
    const ctrl = e.ctrlKey || e.metaKey
    const shift = e.shiftKey
    const key = e.key

    // Ctrl+Shift+N — 新建窗口
    if (ctrl && shift && key === 'N') {
      e.preventDefault()
      e.stopPropagation()
      await getCurrentWindow().create({ label: `main-${Date.now()}` })
      return
    }

    // Ctrl+Shift+R — 重启应用
    if (ctrl && shift && key === 'R') {
      e.preventDefault()
      e.stopPropagation()
      try { await ptyKillAll() } catch { /* ignore */ }
      window.location.reload()
      return
    }

    // Ctrl+, — 打开设置
    if (ctrl && key === ',') {
      e.preventDefault()
      e.stopPropagation()
      sidebarStore.openSettings()
      return
    }

    // Ctrl+Plus / Ctrl+= — 增大字体
    if (ctrl && (key === '+' || key === '=')) {
      e.preventDefault()
      e.stopPropagation()
      appStore.setFontSize(appStore.fontSize + 1)
      return
    }

    // Ctrl+Minus — 缩小字体
    if (ctrl && key === '-') {
      e.preventDefault()
      e.stopPropagation()
      appStore.setFontSize(appStore.fontSize - 1)
      return
    }

    // Ctrl+0 — 重置字体
    if (ctrl && key === '0') {
      e.preventDefault()
      e.stopPropagation()
      appStore.setFontSize(14)
      return
    }

    // F11 — 切换全屏
    if (key === 'F11') {
      e.preventDefault()
      e.stopPropagation()
      const win = getCurrentWindow()
      const fullscreen = await win.isFullscreen()
      await win.setFullscreen(!fullscreen)
      return
    }
  }

  return { handleKeydown }
}
