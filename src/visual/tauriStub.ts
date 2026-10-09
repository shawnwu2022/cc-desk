// All @tauri-apps imports resolve here ONLY in the explicit visual serve mode.
// No host bridge is installed and no operation forwards to an actual API.
import { ref } from 'vue'
export const blockedHostCalls = ref(0)
function blocked(): never { ++blockedHostCalls.value; throw new Error('VISUAL_HOST_ACCESS_BLOCKED') }
// Synthetic read-only settings and an inert progress subscription keep the
// production UpdateSection renderable. Every effectful update command is blocked.
export function invoke(command: string) {
  if (command === 'get_updater_settings') return Promise.resolve({ proxy: null })
  return blocked()
}
export function listen(event: string) {
  if (event === 'desktop-update-progress') return Promise.resolve(() => {})
  return blocked()
}
export const open = blocked, check = blocked, relaunch = blocked
export const writeText = blocked, readText = blocked, readImage = blocked, message = blocked
export class Channel { constructor() { blocked() } }
export const getCurrentWebview = blocked
export const UserAttentionType = { Critical: 1, Informational: 2 }
export function getCurrentWindow() {
  return {
    isMaximized: async () => false, onResized: async () => () => {},
    minimize: blocked, toggleMaximize: blocked, close: blocked, setAlwaysOnTop: blocked,
  }
}
