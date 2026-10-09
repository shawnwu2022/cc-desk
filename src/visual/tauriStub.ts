// All @tauri-apps imports resolve here ONLY in the explicit visual serve mode.
// No host bridge is installed and no operation forwards to an actual API.
import { ref } from 'vue'
export const blockedHostCalls = ref(0)
function blocked(): never { ++blockedHostCalls.value; throw new Error('VISUAL_HOST_ACCESS_BLOCKED') }
export const invoke = blocked, listen = blocked, open = blocked, check = blocked, relaunch = blocked
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
