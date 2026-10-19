import { createApp, h, nextTick, ref } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import NativeCliTerminal from '@/components/NativeCliTerminal.vue'
import { useAppStore } from '@/stores/app'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { launches, sizes, terminal } from './host'
import '@/styles/global.css'

if (!import.meta.env.DEV || import.meta.env.MODE !== 'terminal-fit') throw new Error('TERMINAL_FIT_FIXTURE_DISABLED')
const pinia = createPinia()
setActivePinia(pinia)
const settings = useAppStore()
const active = ref(true)
const cli = new URLSearchParams(location.search).get('cli') === 'claude' ? 'claude' : 'codex'
const tab = useNativeTabsStore().create({ cli, projectId: 'fixture', projectPath: 'E:/fixture', profileId: 'fixture', profileRevision: '1', action: { kind: 'new' } })
createApp({ render: () => h('div', { id: 'terminal-host', style: { position: 'relative', height: 'calc(100vh - 40px)', width: 'calc(100vw - 40px)', margin: '20px', overflow: 'hidden' } }, [h(NativeCliTerminal, { tabId: tab.tabId, active: active.value })]) }).use(pinia).mount('#app')

const fixture = {
  settings, active,
  async show(value: boolean) { active.value = value; await nextTick() },
  async markLastLine() {
    await new Promise<void>(resolve => terminal.write(`\x1b[2J\x1b[${terminal.rows};1H\x1b[42mLAST_ROW_\u2588\u2584\u2588\x1b[0m`, resolve))
    // 连续缩小/放大可能保留旧 scrollback 视口；此案例明确验证正在查看底部的末行。
    terminal.scrollToBottom()
    await new Promise(requestAnimationFrame)
    await new Promise(requestAnimationFrame)
  },
  measure() {
    const host = document.querySelector('#terminal-host')!.getBoundingClientRect()
    const screen = terminal.element!.querySelector('.xterm-screen')!.getBoundingClientRect()
    const lastRow = terminal.element!.querySelector('.xterm-rows')!.lastElementChild!
    const last = lastRow.getBoundingClientRect()
    return { rows: terminal.rows, cols: terminal.cols, host: { bottom: host.bottom, right: host.right }, screen: { top: screen.top, bottom: screen.bottom, right: screen.right, height: screen.height }, last: { bottom: last.bottom, height: last.height, text: lastRow.textContent }, sent: sizes[sizes.length - 1], sizes: [...sizes], launches }
  },
}
declare global { interface Window { terminalFit: typeof fixture } }
window.terminalFit = fixture
