import { ref, onMounted, onUnmounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'

export function useWindowAttention() {
  const win = getCurrentWindow()
  const isFocused = ref(true)
  const focusReady = ref(false)
  let unlistenFocus: (() => void) | null = null
  let disposed = false
  let focusEpoch = 0

  onMounted(async () => {
    const initialEpoch = focusEpoch
    try {
      const unsubscribe = await win.onFocusChanged(({ payload: focused }) => {
        if (disposed) return
        ++focusEpoch
        isFocused.value = focused
        focusReady.value = true
        if (focused) void win.requestUserAttention(null).catch(() => {})
      })
      if (disposed) { unsubscribe(); return }
      unlistenFocus = unsubscribe
      const focused = await win.isFocused()
      if (!disposed && focusEpoch === initialEpoch && typeof focused === 'boolean') {
        isFocused.value = focused
        focusReady.value = true
      }
    } catch { /* Optional window attention cannot block terminal ownership. */ }
  })

  onUnmounted(() => {
    disposed = true
    unlistenFocus?.()
    void win.requestUserAttention(null).catch(() => {})
  })

  return { isFocused, focusReady }
}
