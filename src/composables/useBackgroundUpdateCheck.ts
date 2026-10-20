import { onMounted, onUnmounted } from 'vue'
import { useUpdateStore } from '@/stores/update'

const STARTUP_DELAY_MS = 30_000
const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000

/** 后台只发现版本，不下载、不安装，也不抢占首屏启动。 */
export function useBackgroundUpdateCheck() {
  const updates = useUpdateStore()
  let disposed = false, timer: ReturnType<typeof setTimeout> | undefined
  function check() {
    if (disposed) return
    if (updates.canCheck) void updates.checkForUpdate(() => !disposed).catch(() => { /* 后台失败保持静默，保留已确认版本。 */ })
    timer = setTimeout(check, CHECK_INTERVAL_MS)
  }
  onMounted(() => { timer = setTimeout(check, STARTUP_DELAY_MS) })
  onUnmounted(() => { disposed = true; if (timer) clearTimeout(timer) })
}
