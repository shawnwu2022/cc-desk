<template>
  <div ref="containerRef" class="xterm-container" :class="{ 'drag-over': isDragOver }">
    <!-- 动态渲染每个 Tab 的终端容器 -->
    <div
      v-for="[tabId] in terminalInstances"
      :key="tabId"
      :ref="(el: Element | ComponentPublicInstance | null) => setTerminalEl(tabId, el as HTMLElement | null)"
      :data-tab="tabId"
      class="terminal-wrapper"
      :class="{ active: tabId === currentDisplayTabId }"
    ></div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, watch, onMounted, onUnmounted, nextTick, type ComponentPublicInstance } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebglAddon } from '@xterm/addon-webgl'
import { Unicode11Addon } from '@xterm/addon-unicode11'
import { debounce } from 'lodash-es'
import '@xterm/xterm/css/xterm.css'
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import { useHookStore } from '@/stores/hook'
import { useAttentionStore } from '@/stores/attention'
import { isMac } from '@/utils/platform'
import { getTerminalTheme } from '@/config/terminalThemes'
import {
  ptySpawn,
  ptyInput,
  ptyResize,
  ptyKill,
  onPtyOutput,
  onPtyExit,
  logMessage,
} from '@/api/tauri'
import { registerTerminalCommand } from '@/composables/useTerminalCommand'
import { safeDispose } from '@/utils/dispose'
import { relativizePath } from '@/utils/path'
import { PtyIndex } from '@/utils/ptyIndex'
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { getCurrentWebview } from '@tauri-apps/api/webview'

const props = defineProps<{
  fontSize?: number
}>()

const emit = defineEmits<{
  ptyStarted: [tabId: string, ptyId: string]
  // PTY 退出通知（供 TerminalView settle sessionStart waiter）
  ptyExited: [tabId: string, ptyId: string]
}>()

const appStore = useAppStore()
const sessionStore = useSessionStore()
const hookStore = useHookStore()
const containerRef = ref<HTMLElement>()
const isDragOver = ref(false)

// 等待 DOM 元素可用
async function waitForElement(tabId: string, timeout = 10000): Promise<HTMLElement | null> {
  const start = Date.now()
  while (Date.now() - start < timeout) {
    let el = terminalEls.get(tabId)
    if (el) return el

    el = containerRef.value?.querySelector(
      `.terminal-wrapper[data-tab="${tabId}"]`
    ) as HTMLElement
    if (el) {
      terminalEls.set(tabId, el)
      return el
    }

    await new Promise(r => requestAnimationFrame(r))
  }
  return null
}

// 每个 Tab 独立的 Terminal 实例（key 为 tabId）
const terminalInstances = reactive(new Map<string, {
  term: Terminal
  fitAddon: FitAddon
  ptyId: string
}>())

// Terminal DOM 元素引用
const terminalEls = reactive(new Map<string, HTMLElement | null>())

// ptyId → tabId 反查索引：PTY 输出/退出事件按 ptyId O(1) 定位 tab，
// 替代遍历 terminalInstances 的 O(n) 线性扫描（多并行会话高频输出时随 N 线性放大）。
// 与 terminalInstances 同生命周期：spawn 成功赋 ptyId 时 link，实例销毁时 unlink。
// 抽取为 PtyIndex 类（@/utils/ptyIndex）便于单元测试；非 reactive（仅事件路由用，不驱动渲染）。
const ptyToTab = new PtyIndex()

// 当前显示的 Tab ID
const currentDisplayTabId = ref<string | null>(null)

// 是否正在启动 PTY（防止并发）
const isPtyStarting = ref<boolean>(false)

// macOS 原生 Copy 事件：Tauri MenuBuilder 注册了 Copy 菜单项后，
// Cmd+C 会派发 copy 事件到 WebView，此处将 xterm 选中文本写入剪贴板
function handleNativeCopy(e: ClipboardEvent) {
  const tabId = currentDisplayTabId.value
  if (!tabId) return
  const instance = terminalInstances.get(tabId)
  if (!instance) return
  const selection = instance.term.getSelection()
  if (selection) {
    e.preventDefault()
    writeText(selection).catch(() => {})
  }
}

// Unlisten functions
let unlistenPtyOutput: (() => void) | null = null
let unlistenPtyExit: (() => void) | null = null
let unlistenDragDrop: (() => void) | null = null
let unlistenWindowResized: (() => void) | null = null

// ResizeObserver
let resizeObserver: ResizeObserver | null = null

// 窗口最小化状态（最小化期间跳过 fit，恢复后主动刷新）
let isMinimized = false

// 设置 Terminal DOM 元素引用
function setTerminalEl(tabId: string, el: HTMLElement | null) {
  if (el) {
    terminalEls.set(tabId, el)
    const instance = terminalInstances.get(tabId)
    if (instance && !instance.term.element) {
      instance.term.open(el)
      loadRendererAddons(instance.term)
      if (tabId === currentDisplayTabId.value) {
        requestAnimationFrame(() => instance.fitAddon.fit())
      }
    }
  }
}

// Fit 当前显示的终端（防抖，频繁调用时只有最后一次生效，最小化期间跳过）
const fitCurrentTerminal = debounce(() => {
  if (isMinimized || !currentDisplayTabId.value) return
  const instance = terminalInstances.get(currentDisplayTabId.value)
  if (instance) {
    requestAnimationFrame(() => instance.fitAddon.fit())
  }
}, 50)

// 按平台选择字体：CJK 用等宽字体（Microsoft YaHei / Noto Sans CJK），
// emoji 在主字体中缺失时回退到系统 emoji 字体。把 emoji 字体放在 monospace 前，
// 确保渲染层能找到 emoji 字形（实际宽度由 Unicode 11 wcwidth 决定，与字体回退无关）
function pickFontFamily(): string {
  if (isMac) {
    return '"Cascadia Code", "Fira Code", "JetBrains Mono", Consolas, "Apple Color Emoji", monospace'
  }
  return '"Cascadia Code", "Fira Code", "JetBrains Mono", Consolas, "Microsoft YaHei", "Noto Sans CJK SC", "Segoe UI Emoji", monospace'
}

// 在 term.open(el) 之后加载 Unicode 11，并选择渲染后端。
//
// 渲染后端默认 DOM renderer（不加载 WebGL）。
// 原因：@xterm/addon-webgl 的 glyph atlas 渲染 CJK 宽字符时会概率性留白/错位
// （某个字画成空白，或画错位覆盖邻居；Ctrl+L 全量重绘才修复）。DOM renderer 没有
// glyph atlas 机制（每个字符直接是 DOM 节点），这个问题在 DOM 下不存在。
// CC Desk 的负载是 Claude CLI 交互式文本，DOM 性能足够；WebGL 的收益（高频刷屏）
// 用不上，且附带 GPU context loss / 黑屏 / 驱动兼容等维护成本。
//
// 需要高频滚动性能时切 WebGL：外观设置「终端渲染后端」选 WebGL。此时保留每 5 分钟
// reload + onContextLoss reload 的 glyph atlas 规避（xtermjs/xterm.js#4325），
// 副作用是 reload 瞬间 <50ms 闪烁。
function loadRendererAddons(term: Terminal) {
  try {
    const unicode11 = new Unicode11Addon()
    term.loadAddon(unicode11)
    term.unicode.activeVersion = '11'
  } catch (err) {
    console.warn('[XTerm] Unicode 11 addon unavailable, fallback to default:', err)
  }

  // 渲染后端：外观设置 webglRenderer 控制。默认 DOM renderer（无 glyph atlas，
  // 规避 CJK 渲染留白/错位）；WebGL 高频滚动更流畅但附带该问题。
  // 仅对新开终端生效（renderer 在 term.open 时设定，运行时不切换）。
  if (!appStore.webglRenderer) {
    attachImeInputFix(term)
    return
  }

  // ---- 可选：WebGL renderer（外观设置 webglRenderer=true 时启用）----
  let webglAddon: WebglAddon | null = null

  const reloadWebgl = () => {
    if (webglAddon) {
      try { webglAddon.dispose() } catch { /* 已 dispose */ }
      webglAddon = null
    }
    try {
      webglAddon = new WebglAddon()
      // context loss 也走 reload（修复之前只 dispose 导致的黑屏隐患）
      webglAddon.onContextLoss(() => reloadWebgl())
      term.loadAddon(webglAddon)
    } catch (err) {
      console.warn('[XTerm] WebGL reload failed, fallback to DOM renderer:', err)
      webglAddon = null
    }
  }

  try {
    reloadWebgl()
    const timer = setInterval(reloadWebgl, 5 * 60 * 1000)
    atlasTimers.set(term, timer)
  } catch (err) {
    console.warn('[XTerm] WebGL init failed:', err)
  }

  // term.open 后 textarea 已创建，绑定 IME 输入修复（幂等）
  attachImeInputFix(term)
}

// 跟踪每个 Terminal 实例的 WebGL reload timer
const atlasTimers = new WeakMap<Terminal, ReturnType<typeof setInterval>>()

// 统一清理 terminal：先停 timer，再 dispose
async function disposeTerminal(term: Terminal, context: string) {
  const timer = atlasTimers.get(term)
  if (timer) {
    clearInterval(timer)
    atlasTimers.delete(term)
  }
  const imeTa = term.textarea
  if (imeTa) {
    imeFixStates.get(imeTa)?.dispose()
    imeFixStates.delete(imeTa)
  }
  await safeDispose(term, context)
}

// 修复：搜狗等中文 IME 用 composed=true 的 insertText 提交候选词/拼音（如 Shift 切换中英文时
// 把已输入的拼音作为字母提交）。xterm.js 的 _inputEvent 发送条件为
// `(!ev.composed || !this._keyDownSeen)`，Shift 的 keydown 已把 _keyDownSeen 置 true，
// 于是 composed=true && _keyDownSeen=true 的 input 被 xterm 丢弃，字符不进 PTY。
//
// 注意：xterm 的 cancel() 默认无效（cancelEvents=false），既不 preventDefault 也不
// stopPropagation，所以不能用「bubble 监听是否触发」判断 xterm 是否已处理。此处镜像 xterm 的
// _keyDownSeen，只在精确漏发分支（composed=true && keyDownSeen=true）补发，绝不与 xterm 重复；
// 并排除走了真实 composition 生命周期的输入（微软拼音等，由 xterm 原生 composition 路径处理）。
interface ImeFixState {
  keyDownSeen: boolean      // 镜像 xterm _keyDownSeen：keydown 置 true，keyup 置 false
  compositionSeen: boolean  // 本次输入周期见过 compositionstart（走 composition 的 IME），不补发
  dataSeen: boolean         // 本次 keydown 后 xterm 是否已通过 onData 发送（普通字母已发→不补；IME 漏发→补）
  dispose: () => void
}
const imeFixStates = new WeakMap<HTMLTextAreaElement, ImeFixState>()

// term.open 之后调用（textarea 已存在）；幂等。镜像 _keyDownSeen 并在 xterm 漏发分支补发。
function attachImeInputFix(term: Terminal) {
  const ta = term.textarea
  if (!ta) return
  // 用 textarea（DOM 元素，不被 Vue reactive proxy）作 key，避免 proxy term 与原始 term
  // 视为不同 key 导致重复绑定（setTerminalEl 的 instance.term 是 proxy，startTab 的 term 是原始）
  if (imeFixStates.has(ta)) return
  const state: ImeFixState = { keyDownSeen: false, compositionSeen: false, dataSeen: false, dispose: () => {} }

  const onKeyDown = () => { state.keyDownSeen = true; state.compositionSeen = false; state.dataSeen = false }
  const onKeyUp = () => { state.keyDownSeen = false }
  const onCompositionStart = () => { state.compositionSeen = true }
  const onInput = (e: Event) => {
    const ie = e as InputEvent
    // 仅补发 xterm 真正漏发的：composed insertText、本次 keydown 后、未见 composition、
    // 且 xterm 自己没通过 onData 发送过。普通字母 xterm keydown 已发 onData（dataSeen=true）→不补，
    // 避免搜狗英文状态 Shift+I 出现 "II" 重复；搜狗中文 Shift 切换提交拼音时 xterm 因 IME 拦截
    // keydown 未发 onData（dataSeen=false）→ 补，修复拼音丢失。
    if (ie.inputType === 'insertText' && ie.composed && ie.data && state.keyDownSeen && !state.compositionSeen && !state.dataSeen) {
      term.input(ie.data)
    }
  }
  ta.addEventListener('keydown', onKeyDown)
  ta.addEventListener('keyup', onKeyUp)
  ta.addEventListener('compositionstart', onCompositionStart)
  ta.addEventListener('input', onInput)
  const onDataDisp = term.onData(() => { state.dataSeen = true })
  state.dispose = () => {
    ta.removeEventListener('keydown', onKeyDown)
    ta.removeEventListener('keyup', onKeyUp)
    ta.removeEventListener('compositionstart', onCompositionStart)
    ta.removeEventListener('input', onInput)
    onDataDisp.dispose()
  }
  imeFixStates.set(ta, state)
}

// 创建新的 Terminal 实例
function createTerminal(tabId: string): Terminal {
  const term = new Terminal({
    fontFamily: pickFontFamily(),
    fontSize: props.fontSize ?? 12,
    lineHeight: 1.2,
    cursorBlink: true,
    cursorStyle: 'bar',
    theme: getTerminalTheme(appStore.terminalTheme),
    allowProposedApi: true,
    macOptionIsMeta: true,
    scrollback: 10000,
  })

  const fitAddon = new FitAddon()
  term.loadAddon(fitAddon)

  // 用户输入 → 发送到对应的 PTY
  term.onData(data => {
    const instance = terminalInstances.get(tabId)
    if (instance) {
      // ptyId 空（fit 在 spawn 前发生）时跳过发送；Escape 仍清 working 状态
      if (instance.ptyId) ptyInput(instance.ptyId, data)

      // Escape 按键：Claude 的 Stop hook 不在用户中断时触发，立即清除 working
      if (data === '\x1b') {
        const tab = sessionStore.tabs.get(tabId)
        if (tab?.working) tab.working = false
      }
    }
  })

  // 终端尺寸变化 → resize 对应的 PTY
  term.onResize(({ cols, rows }) => {
    const instance = terminalInstances.get(tabId)
    if (instance && instance.ptyId) {
      ptyResize(instance.ptyId, cols, rows)
    }
  })

  // 复制粘贴处理
  term.attachCustomKeyEventHandler((event: KeyboardEvent) => {
    if (event.type !== 'keydown') return true

    // Cmd+C (macOS) 复制选中内容
    if (event.metaKey && !event.ctrlKey && event.key === 'c') {
      const selection = term.getSelection()
      if (selection) {
        event.preventDefault()
        writeText(selection).catch(() => {})
        return false
      }
      return true
    }

    // Ctrl+C 复制（有选中）或 SIGINT（无选中）
    if (event.ctrlKey && !event.metaKey && event.key === 'c' && !event.shiftKey) {
      const selection = term.getSelection()
      if (selection) {
        event.preventDefault()
        writeText(selection).catch(() => {})
        return false
      }
      return true
    }

    // Ctrl+Shift+C 强制复制
    if (event.ctrlKey && event.shiftKey && event.key === 'C') {
      event.preventDefault()
      const selection = term.getSelection()
      if (selection) {
        writeText(selection).catch(() => {})
      }
      return false
    }

    // Ctrl+V / Cmd+V 粘贴
    if ((event.ctrlKey || event.metaKey) && event.key === 'v') {
      event.preventDefault()
      readText().then(text => {
        if (text) term.paste(text)
      }).catch(() => {})
      return false
    }

    // Shift+Enter => 插入换行（模拟 \ + Enter）
    if (event.shiftKey && event.key === 'Enter') {
      event.preventDefault()
      const instance = terminalInstances.get(tabId)
      if (instance) {
        ptyInput(instance.ptyId, '\\\r')
      }
      return false
    }

    return true
  })

  return term
}

onMounted(async () => {
  await setupEventListeners()
  window.addEventListener('copy', handleNativeCopy)

  if (containerRef.value) {
    resizeObserver = new ResizeObserver(() => fitCurrentTerminal())
    resizeObserver.observe(containerRef.value)
  }

  // 监听窗口 resize → 追踪最小化状态，恢复时主动刷新
  const win = getCurrentWindow()
  unlistenWindowResized = await win.onResized(async () => {
    const minimized = await win.isMinimized()
    if (isMinimized && !minimized) {
      // 从最小化恢复 → fit + 刷新渲染 + 滚动到底部
      isMinimized = false
      const tabId = currentDisplayTabId.value
      if (tabId) {
        const instance = terminalInstances.get(tabId)
        if (instance) {
          await nextTick()
          instance.fitAddon.fit()
          instance.term.refresh(0, instance.term.rows - 1)
          instance.term.scrollToBottom()
        }
      }
    } else {
      isMinimized = minimized
    }
  })

  registerTerminalCommand(sendText)
})

// 设置事件监听器
async function setupEventListeners() {
  // 文件拖放 → 将路径输入终端
  // 项目内文件转换为相对路径，便于 Claude 直接引用
  unlistenDragDrop = await getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === 'drop') {
      isDragOver.value = false
      const paths = event.payload.paths
      if (paths.length > 0) {
        const projectPath = sessionStore.tabs.get(currentDisplayTabId.value ?? '')?.projectPath ?? ''
        const text = paths
          .map(p => relativizePath(p, projectPath))
          .map(p => p.includes(' ') ? `"${p}"` : p)
          .join(' ')
        sendText(text)
      }
    } else if (event.payload.type === 'enter' || event.payload.type === 'over') {
      isDragOver.value = true
    } else {
      isDragOver.value = false
    }
  })

  // PTY 输出 → 写入对应 Tab 的 Terminal（ptyToTab 反查 O(1)，替代遍历 terminalInstances）
  unlistenPtyOutput = await onPtyOutput(({ id, data }) => {
    const tabId = ptyToTab.get(id)
    if (!tabId) return
    const instance = terminalInstances.get(tabId)
    if (instance) instance.term.write(data)
  })

  // PTY 退出 → 更新 Tab 状态（不删除 Tab）；ptyToTab 反查 tabId
  unlistenPtyExit = await onPtyExit(({ id }) => {
    const tabId = ptyToTab.get(id)
    if (!tabId) return
    const instance = terminalInstances.get(tabId)
    if (!instance) return

    // 更新 store（Tab 保留，状态变 stopped）
    sessionStore.handlePtyExit(id)

    // 清理 hook 状态（防止残留旧数据）
    hookStore.clearSession(id)

    // 销毁 Terminal 实例（释放资源）
    void disposeTerminal(instance.term, `onPtyExit(tabId=${tabId})`)
    terminalInstances.delete(tabId)
    terminalEls.delete(tabId)
    ptyToTab.unlink(id)
    // 通知 TerminalView settle sessionStart waiter（PTY 退出）
    emit('ptyExited', tabId, id)
  })
}

// 监听 fontSize 变化
watch(() => props.fontSize, (newSize) => {
  if (newSize) {
    for (const instance of terminalInstances.values()) {
      instance.term.options.fontSize = newSize
      instance.fitAddon.fit()
    }
  }
})

// 监听终端主题变化，更新所有终端实例（与 GUI 浅/暗独立）
watch(() => appStore.terminalTheme, (newId) => {
  const themeConfig = getTerminalTheme(newId)
  for (const instance of terminalInstances.values()) {
    instance.term.options.theme = themeConfig
  }
})

// 监听活跃 Tab 变化 → 切换显示
watch(() => sessionStore.activeTabId, async (newTabId, oldTabId) => {
  if (!newTabId) return

  if (newTabId === oldTabId) {
    fitCurrentTerminal()
    return
  }

  if (isPtyStarting.value) return

  currentDisplayTabId.value = newTabId

  await nextTick()

  const existingInstance = terminalInstances.get(newTabId)

  if (existingInstance) {
    const buf = existingInstance.term.buffer.active
    existingInstance.term.refresh(0, Math.max(buf.length - 1, 0))
    requestAnimationFrame(() => existingInstance.fitAddon.fit())
  } else {
    const tab = sessionStore.tabs.get(newTabId)
    if (!tab) return

    if (tab.ptyId && tab.status === 'running') {
      // PTY 在运行但没有 Terminal 实例 → 创建
      await createTerminalForTab(newTabId, tab.ptyId)
    } else if (tab.status === 'stopped') {
      // 已停止的 Tab：不自动启动，等用户操作
    }
  }
})

// 为已有 PTY 的 Tab 创建 Terminal 实例
async function createTerminalForTab(tabId: string, ptyId: string) {
  if (terminalInstances.has(tabId)) return

  const term = createTerminal(tabId)
  const fitAddon = new FitAddon()
  term.loadAddon(fitAddon)

  terminalInstances.set(tabId, { term, fitAddon, ptyId })
  ptyToTab.link(ptyId, tabId)

  const el = await waitForElement(tabId)
  if (el) {
    // display:none 时 xterm 无法获取尺寸，临时显示
    const isActive = tabId === currentDisplayTabId.value
    if (!isActive) el.style.display = 'block'

    term.open(el)
    loadRendererAddons(term)

    if (!isActive) {
      requestAnimationFrame(() => {
        fitAddon.fit()
        el.style.display = ''
      })
    } else {
      requestAnimationFrame(() => fitAddon.fit())
    }
  }
}

// ==================== Tab 操作（外部调用） ====================

/**
 * 启动 Tab 的 PTY。
 * 由 TerminalView 调用，传入已创建好的 tabId。
 * @returns 成功 {ok:true}；失败 {ok:false,error}（已统一清理 tab/terminal instance，不刷历史）
 */
async function startTab(tabId: string): Promise<{ ok: true } | { ok: false; error: string }> {
  const tab = sessionStore.tabs.get(tabId)
  if (!tab) {
    logMessage('warn', `startTab: tab not found, tabId=${tabId}`)
    return { ok: false, error: 'tab not found' }
  }
  if (isPtyStarting.value) {
    logMessage('warn', `startTab: blocked by isPtyStarting, tabId=${tabId}`)
    return { ok: false, error: 'blocked by isPtyStarting' }
  }

  // 已有运行中的 PTY
  if (tab.ptyId && tab.status === 'running') {
    if (!terminalInstances.has(tabId)) {
      await createTerminalForTab(tabId, tab.ptyId)
    }
    return { ok: true }
  }

  isPtyStarting.value = true

  try {
    const args = buildClaudeArgs(tab)
    const cwd = tab.projectPath

    const term = createTerminal(tabId)
    const fitAddon = new FitAddon()
    term.loadAddon(fitAddon)

    // 先注册实例（ptyId 暂空）+ open + fit 拿实际 cols/rows，再 spawn，
    // 避免 Claude CLI 按 80 列输出 resume 历史、实际窗口更宽导致历史挤左
    terminalInstances.set(tabId, { term, fitAddon, ptyId: '' })
    sessionStore.setActiveTab(tabId)
    currentDisplayTabId.value = tabId

    let cols = 80
    let rows = 24
    const el = await waitForElement(tabId)
    if (el) {
      term.open(el)
      loadRendererAddons(term)
      fitAddon.fit()
      cols = term.cols
      rows = term.rows
    }

    const info = await ptySpawn({
      cwd,
      cols,
      rows,
      type: 'claude',
      args,
    })

    if (info) {
      const instance = terminalInstances.get(tabId)
      if (instance) instance.ptyId = info.id
      sessionStore.setTabPty(tabId, info.id)
      ptyToTab.link(info.id, tabId)
      emit('ptyStarted', tabId, info.id)
      return { ok: true }
    }
    // info==null：统一清理（P2.7 修复：原仅 delete terminalInstances，tab 残留）
    discardUnstartedTab(tabId)
    return { ok: false, error: 'no pty info' }
  } catch (err) {
    // 异常统一清理（P2.7 修复：原裸 terminalInstances.delete 绕过 disposeTerminal）
    discardUnstartedTab(tabId)
    console.error('[XTerm] startTab ERROR:', err)
    logMessage('error', `startTab failed, tabId=${tabId}: ${err}`)
    return { ok: false, error: String(err) }
  } finally {
    isPtyStarting.value = false
  }
}

/**
 * 统一清理未成功启动的 Tab（P2.7）：
 * - disposeTerminal（停 atlas/IME timer + safeDispose，非裸 term.dispose）
 * - sessionStore.removeTab（删 tab 不 kill PTY 不刷历史，区别于 closeTab）
 */
function discardUnstartedTab(tabId: string) {
  const instance = terminalInstances.get(tabId)
  if (instance) {
    void disposeTerminal(instance.term, `discardUnstartedTab(tabId=${tabId})`)
    terminalInstances.delete(tabId)
    terminalEls.delete(tabId)
  }
  sessionStore.removeTab(tabId)
}

/**
 * 清理指定 Tab 的 Terminal 实例（不动 store tab、不 kill PTY）。
 * 用于 startProjectSession timeout 路径：PTY 由调用方 ptyKill，onPtyExit 也会清此实例；
 * 此处显式清为幂等兜底（先到者清实例 + 删 Map，后到者 no-op），避免依赖 pty-exit 事件时序。
 * 保留 tab（status 由 onPtyExit -> handlePtyExit 置 stopped），调用方不 removeTab。
 */
function disposeTabInstance(tabId: string) {
  const instance = terminalInstances.get(tabId)
  if (instance) {
    if (instance.ptyId) ptyToTab.unlink(instance.ptyId)
    void disposeTerminal(instance.term, `disposeTabInstance(tabId=${tabId})`)
    terminalInstances.delete(tabId)
    terminalEls.delete(tabId)
  }
}

/**
 * 重启 Tab（停止的 Tab 恢复运行）
 */
async function restartTab(tabId: string) {
  const tab = sessionStore.tabs.get(tabId)
  if (!tab || isPtyStarting.value) return

  isPtyStarting.value = true

  try {
    // 如果有旧 PTY 在运行，先 kill
    if (tab.ptyId) {
      // 重启退役旧 PTY：clearPty 清焦点队列关注项 + 标 tombstone，
      // 防 pty-exit 晚于 ptyToTab unlink 导致退出处理 return（不走 handlePtyExit）漏清（codex P2）
      useAttentionStore().clearPty(tab.ptyId)
      try { await ptyKill(tab.ptyId) } catch {}
      hookStore.clearSession(tab.ptyId)
    }

    // 清理旧 Terminal 实例（dispose 失败不阻断重启流程，仍需清理 Map 引用）
    const oldInstance = terminalInstances.get(tabId)
    if (oldInstance) {
      if (oldInstance.ptyId) ptyToTab.unlink(oldInstance.ptyId)
      await disposeTerminal(oldInstance.term, `restartTab(old term, tabId=${tabId})`)
      terminalInstances.delete(tabId)
      terminalEls.delete(tabId)
    }

    const args = buildClaudeArgs(tab)
    const cwd = tab.projectPath

    const term = createTerminal(tabId)
    const fitAddon = new FitAddon()
    term.loadAddon(fitAddon)

    // 先注册实例（ptyId 暂空）+ open + fit 拿实际 cols/rows，再 spawn，
    // 避免 Claude CLI 按 80 列输出 resume 历史、实际窗口更宽导致历史挤左
    terminalInstances.set(tabId, { term, fitAddon, ptyId: '' })
    sessionStore.setActiveTab(tabId)
    currentDisplayTabId.value = tabId

    let cols = 80
    let rows = 24
    const el = await waitForElement(tabId)
    if (el) {
      term.open(el)
      loadRendererAddons(term)
      fitAddon.fit()
      cols = term.cols
      rows = term.rows
    }

    const info = await ptySpawn({
      cwd,
      cols,
      rows,
      type: 'claude',
      args,
    })

    if (info) {
      const instance = terminalInstances.get(tabId)
      if (instance) instance.ptyId = info.id
      sessionStore.setTabPty(tabId, info.id)
      ptyToTab.link(info.id, tabId)
      emit('ptyStarted', tabId, info.id)
    } else {
      terminalInstances.delete(tabId)
    }

    appStore.resetClaudeOptions()
  } catch (err) {
    terminalInstances.delete(tabId)
    console.error('[XTerm] restartTab ERROR:', err)
    logMessage('error', `restartTab failed, tabId=${tabId}: ${err}`)
  } finally {
    isPtyStarting.value = false
  }
}

/**
 * 根据 Tab 状态构建 Claude CLI 参数
 */
function buildClaudeArgs(tab: { sessionId: string | null }): string[] {
  const args: string[] = []
  const opts = appStore.claudeOptions

  // 有 sessionId → --resume；无 → 新建会话（不带 --resume）
  if (tab.sessionId) {
    args.push('--resume', tab.sessionId)
  }

  if (opts.skipPermissions) args.push('--dangerously-skip-permissions')
  if (opts.customArgs) {
    const custom = opts.customArgs.trim().split(/\s+/).filter(Boolean)
    args.push(...custom)
  }

  return args
}

/**
 * 使用启动选项创建并启动 Tab（兼容旧的 startWithOptions 入口）
 */
async function startWithOptions(cwd: string, opts: {
  resume?: string
  skipPermissions?: boolean
  customArgs?: string
}) {
  if (isPtyStarting.value) return

  const tabId = sessionStore.createTab(cwd, {
    sessionId: opts.resume || undefined,
  })

  sessionStore.setActiveTab(tabId)
  await startTab(tabId)
  appStore.resetClaudeOptions()
}

/**
 * 新建会话（无参数模式）
 */
async function startNewSession(cwd: string) {
  if (isPtyStarting.value) {
    logMessage('warn', `startNewSession: blocked by isPtyStarting, cwd=${cwd}`)
    return
  }

  const tabId = sessionStore.createTab(cwd)
  sessionStore.setActiveTab(tabId)
  await startTab(tabId)
  appStore.resetClaudeOptions()
}

// 清理所有 PTY
async function cleanup() {
  for (const [tabId, instance] of terminalInstances.entries()) {
    await disposeTerminal(instance.term, `cleanup(tabId=${tabId})`)
  }
  terminalInstances.clear()
  terminalEls.clear()
  ptyToTab.clear()
  await sessionStore.cleanupAll()
}

// 向活跃终端发送文字并聚焦
function sendText(text: string) {
  const tabId = currentDisplayTabId.value
  if (!tabId) return
  const instance = terminalInstances.get(tabId)
  if (instance) {
    ptyInput(instance.ptyId, text)
    instance.term.focus()
  }
}

// 聚焦活跃终端
function focus() {
  const tabId = currentDisplayTabId.value
  if (!tabId) return
  const instance = terminalInstances.get(tabId)
  if (instance) instance.term.focus()
}

// 兼容：重启当前活跃 Tab
async function restartCurrentPty() {
  if (sessionStore.activeTabId) {
    await restartTab(sessionStore.activeTabId)
  }
}

onUnmounted(() => {
  fitCurrentTerminal.cancel()
  resizeObserver?.disconnect()
  window.removeEventListener('copy', handleNativeCopy)
  unlistenPtyOutput?.()
  unlistenPtyExit?.()
  unlistenDragDrop?.()
  unlistenWindowResized?.()
  for (const [tabId, instance] of terminalInstances.entries()) {
    void disposeTerminal(instance.term, `onUnmounted(tabId=${tabId})`)
  }
})

defineExpose({
  startTab,
  restartTab,
  startWithOptions,
  startNewSession,
  restartCurrentPty,
  cleanup,
  sendText,
  focus,
  fitCurrentTerminal,
  disposeTabInstance,
})
</script>

<style scoped>
.xterm-container {
  width: 100%;
  height: 100%;
  box-sizing: border-box;
  background: var(--terminal-surface-bg);
  border-radius: 8px;
  position: relative;
  overflow: hidden;
  transition: box-shadow 0.15s ease;
}

.xterm-container.drag-over {
  box-shadow: inset 0 0 0 2px var(--accent-gold);
}

.terminal-wrapper {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  padding: 0 12px;
  box-sizing: border-box;
  display: none;
}

.terminal-wrapper.active {
  display: block;
}

.terminal-wrapper :deep(.xterm) {
  height: 100%;
}

.terminal-wrapper :deep(.xterm-viewport) {
  border-radius: 4px;
}

.terminal-wrapper :deep(.xterm-viewport::-webkit-scrollbar) {
  width: 8px;
}

.terminal-wrapper :deep(.xterm-viewport::-webkit-scrollbar-thumb) {
  background: var(--terminal-scrollbar);
  border-radius: 4px;
}

.terminal-wrapper :deep(.xterm-viewport::-webkit-scrollbar-track) {
  background: transparent;
}
</style>
