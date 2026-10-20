import { ref } from 'vue'
import { defineStore } from 'pinia'
import type { UnifiedCliKind } from '@/types/unifiedSession'

// GUI 层的 per-CLI 支持开关：关闭后不出现新建入口、不加载/不列出该 CLI 的
// 历史与会话行。仅是显示与入口语义，不触碰 CLI 文件、profile 或运行中进程；
// 已打开的终端（活进程）始终保留可选中与关闭。持久化跟随 newSessionDraft 的
// localStorage 先例：这是纯 GUI 偏好，不进入后端 AppConfig 结构化配置。
const storageKey = 'cc-desk-cli-support-v1'

type CliSupport = Record<UnifiedCliKind, boolean>

function readSupport(): CliSupport {
  try {
    const value = JSON.parse(localStorage.getItem(storageKey) ?? 'null')
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      return { claude: typeof value.claude === 'boolean' ? value.claude : true,
        codex: typeof value.codex === 'boolean' ? value.codex : false }
    }
  } catch { /* Optional UI preference storage must never block startup. */ }
  return { claude: true, codex: false }
}

export const useCliSupportStore = defineStore('cli-support', () => {
  const support = ref<CliSupport>(readSupport())
  function isEnabled(cli: UnifiedCliKind): boolean {
    return support.value[cli]
  }
  function setCliEnabled(cli: UnifiedCliKind, enabled: boolean) {
    support.value = { ...support.value, [cli]: enabled }
    try { localStorage.setItem(storageKey, JSON.stringify(support.value)) } catch { /* In-memory preference remains usable. */ }
  }
  /** 关闭的默认 CLI 回落到仍启用的 CLI；两者都关闭时保持原值（入口已全部隐藏）。 */
  function fallbackDefault(preferred: UnifiedCliKind): UnifiedCliKind {
    if (support.value[preferred]) return preferred
    return preferred === 'claude' ? (support.value.codex ? 'codex' : 'claude') : (support.value.claude ? 'claude' : 'codex')
  }
  return { support, isEnabled, setCliEnabled, fallbackDefault }
})
