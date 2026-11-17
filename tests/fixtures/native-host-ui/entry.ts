import { createApp, h } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import type { LaunchRequest } from '@/types/cli'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'
import RemoteHostPanel from '@/components/workspace/RemoteHostPanel.vue'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import '@/styles/global.css'

// 原生产组件、store 与 IPC；夹具只提供实际 cli_start 收据和公开样本。
export async function mount(request: LaunchRequest, candidate: string) {
  if (request.generation !== 1) throw Error('FIXTURE_INITIAL_GENERATION_REQUIRED')
  const bridge = (window as any).__CC_DESK_DOCUMENT__
  const native = (window as any).__TAURI_INTERNALS__
  const channelId = native.transformCallback(() => {}, false)
  const receipt: LaunchStatus = await bridge.invoke('cli_start', request, { toJSON: () => `__CHANNEL__:${channelId}` })
  if (receipt.phase !== 'running') throw Error('FIXTURE_LAUNCH_NOT_RUNNING')
  const pinia = createPinia()
  setActivePinia(pinia)
  const tabs = useNativeTabsStore()
  tabs.create({ preparedIdentity: { tabId: request.tabId, requestId: request.requestId, runId: request.runId }, cli: 'codex', projectId: 'isolated', projectPath: request.launchCwd, profileId: request.profileId, profileRevision: request.expectedProfileRevision, action: request.action })
  if (!tabs.applyLaunchStatus(request.tabId, receipt)) throw Error('FIXTURE_LAUNCH_IDENTITY_MISMATCH')
  document.body.innerHTML = '<h1>TEST ONLY · Isolated Host UI</h1><p>Production Vue panel + real WebView IPC. Synthetic Node only. Start command is NOT registered.</p><label>Public synthetic candidate<textarea id="sample" readonly aria-label="合成公开候选"></textarea></label><div id="app"></div>'
  ;(document.getElementById('sample') as HTMLTextAreaElement).value = candidate
  document.getElementById('sample')!.style.cssText = 'display:block;width:42%;height:200px;font-size:10px'
  createApp({ render: () => h(RemoteHostPanel, { tabId: request.tabId }) }).use(pinia).mount('#app')
}
