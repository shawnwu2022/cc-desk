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
import { computed, ref, reactive, watch, onMounted, onUnmounted, nextTick, toRaw, type ComponentPublicInstance } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import type { WebglAddon } from '@xterm/addon-webgl'
import { Unicode11Addon } from '@xterm/addon-unicode11'
import { debounce } from 'lodash-es'
import '@xterm/xterm/css/xterm.css'
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import { useHookStore } from '@/stores/hook'
import { useAttentionStore } from '@/stores/attention'
import { platform } from '@/utils/platform'
import { terminalAppearanceOptions, applyTerminalAppearance } from '@/config/terminalPreferences'
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
import { TerminalRendererRegistry } from '@/utils/rendererRegistry'
import { bindNativePaste, buildPastePayload, commitPasteWithEvidence, imagePasteBytes } from '@/utils/pasteText'
import type { XtermProvenanceSource } from '@/terminal/xtermProvenance'
import { createImeInputPolicy, isPasteShortcut } from '@/terminal/inputPolicy'
import { readImage, readText, writeText } from '@tauri-apps/plugin-clipboard-manager'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { getCurrentWebview } from '@tauri-apps/api/webview'

const props = defineProps<{
  visible?: boolean
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
const visible = computed(() => props.visible !== false)
const needsFit = new Set<string>()
const pendingFits = new Map<string, Terminal>()
const rendererPreferences = new WeakMap<Terminal, boolean>()
let disposed = false
let unregisterCommand: (() => void) | null = null
function isVisibleTab(tabId: string) { return !disposed && visible.value && currentDisplayTabId.value === tabId }
function fitTab(tabId: string) {
  needsFit.add(tabId)
  if (!isVisibleTab(tabId) || isMinimized) return
  const instance = terminalInstances.get(tabId)
  if (!instance || pendingFits.get(tabId) === instance.term) return
  pendingFits.set(tabId, instance.term)
  requestAnimationFrame(() => {
    if (pendingFits.get(tabId) !== instance.term) return
    pendingFits.delete(tabId)
    if (!isVisibleTab(tabId) || isMinimized || terminalInstances.get(tabId) !== instance) return
    instance.fitAddon.fit()
    needsFit.delete(tabId)
  })
}
function fitVisible() {
  for (const tabId of terminalInstances.keys()) needsFit.add(tabId)
  if (currentDisplayTabId.value) fitTab(currentDisplayTabId.value)
}

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
const currentDisplayTabId = ref<string | null>(sessionStore.activeTabId)

// 是否正在启动 PTY（防止并发）
const isPtyStarting = ref<boolean>(false)

// PTY ID 由前端预分配，使 ptyId -> tabId 路由在后端启动进程前就绪。
function createPtyId(): string {
  return crypto.randomUUID()
}

// macOS 原生 Copy 事件：Tauri MenuBuilder 注册了 Copy 菜单项后，
// Cmd+C 会派发 copy 事件到 WebView，此处将 xterm 选中文本写入剪贴板
function handleNativeCopy(e: ClipboardEvent) {
  if (!visible.value || !containerRef.value?.contains(document.activeElement)) return
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
let unbindNativePaste: (() => void) | null = null

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
      void loadRendererAddons(instance.term)
      fitTab(tabId)
    }
  }
}

// Fit 当前显示的终端（防抖，频繁调用时只有最后一次生效，最小化期间跳过）
const fitCurrentTerminal = debounce(() => {
  fitVisible()
}, 50)

// Terminal 渲染生命周期注册表：per-terminal 单飞初始化、dispose 标记、reload timer。
// key 一律 toRaw 归一——terminalInstances 是深度 reactive Map，setTerminalEl 取出的
// instance.term 是 Vue proxy，与创建路径的 raw terminal 身份不同；不归一会让 WeakMap
// 建立两个 key（单飞失效、timer 句柄被覆盖泄漏）。详见 utils/rendererRegistry.ts。
const rendererRegistry = new TerminalRendererRegistry()

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
async function loadRendererAddons(term: Terminal): Promise<void> {
  // 单飞（registry 内 toRaw 归一 key）：并发调用复用同一 Promise，
  // 避免重复 loadAddon 与 interval 句柄覆盖泄漏。
  await rendererRegistry.runOnce(term, () => loadRendererAddonsOnce(term))
}

async function loadRendererAddonsOnce(term: Terminal) {
  // dispose 已发生则不初始化（极端竞态：创建后立刻关）。
  if (rendererRegistry.isDisposed(term)) {
    return
  }
  try {
    const unicode11 = new Unicode11Addon()
    term.loadAddon(unicode11)
    term.unicode.activeVersion = '11'
  } catch (err) {
    console.warn('[XTerm] Unicode 11 addon unavailable, fallback to default:', err)
  }

  // term.open 后 textarea 已创建，绑定 IME 输入修复（幂等）
  try {
    attachImeInputFix(term)
  } catch (err) {
    console.warn('[XTerm] IME input fix unavailable:', err)
  }

  // 渲染后端：外观设置 webglRenderer 控制。默认 DOM renderer（无 glyph atlas，
  // 规避 CJK 渲染留白/错位）；WebGL 高频滚动更流畅但附带该问题。
  // 仅对新开终端生效（renderer 在 term.open 时设定，运行时不切换）。
  if (!rendererPreferences.get(toRaw(term))) {
    return
  }

  let WebglAddonCtor: (typeof import('@xterm/addon-webgl'))['WebglAddon']
  try {
    const module = await import('@xterm/addon-webgl')
    // 竞态守卫：await 期间 terminal 可能已被 disposeTerminal 销毁（关 tab / 重启 / 退出）。
    // 恢复后不得再加载 addon 或创建 timer，否则会操作已销毁实例并留下无人清除的定时器。
    if (rendererRegistry.isDisposed(term)) {
      return
    }
    WebglAddonCtor = module.WebglAddon
  } catch (err) {
    console.warn('[XTerm] WebGL addon unavailable, fallback to DOM renderer:', err)
    return
  }

  // ---- 可选：WebGL renderer（外观设置 webglRenderer=true 时启用）----
  let webglAddon: WebglAddon | null = null

  const reloadWebgl = () => {
    // reload 定时器每次触发前也校验：dispose 后定时器本应被 clearTimer，
    // 但万一竞态漏清，这里兜底防止操作已销毁 terminal。
    if (rendererRegistry.isDisposed(term)) {
      return
    }
    if (webglAddon) {
      try { webglAddon.dispose() } catch { /* 已 dispose */ }
      webglAddon = null
    }
    try {
      webglAddon = new WebglAddonCtor()
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
    const handle = setInterval(reloadWebgl, 5 * 60 * 1000)
    rendererRegistry.setTimer(term, () => clearInterval(handle))
  } catch (err) {
    console.warn('[XTerm] WebGL init failed:', err)
  }
}

// 统一清理 terminal：先停 timer，再 dispose
async function disposeTerminal(term: Terminal, context: string) {
  rendererRegistry.markDisposed(term)
  rendererRegistry.clearTimer(term)
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
  const policy = createImeInputPolicy()
  const state: ImeFixState = { dispose: () => {} }

  const onKeyDown = () => policy.keyDown()
  const onKeyUp = () => policy.keyUp()
  const onCompositionStart = () => policy.compositionStart()
  const onInput = (e: Event) => {
    const ie = e as InputEvent
    const text = policy.input({
      inputType: ie.inputType,
      composed: ie.composed,
      data: ie.data,
    })
    if (!text) return

    // The fallback is a proven user action. Send it directly instead of feeding
    // it back through term.input(), which would re-emerge as ambiguous onData.
    for (const instance of terminalInstances.values()) {
      if (isVisibleTab(currentDisplayTabId.value ?? '') && instance.term.textarea === ta && instance.ptyId && instance === terminalInstances.get(currentDisplayTabId.value!)) {
        void ptyInput(instance.ptyId, text, 'ime-fallback')
        return
      }
    }
  }
  ta.addEventListener('keydown', onKeyDown)
  ta.addEventListener('keyup', onKeyUp)
  ta.addEventListener('compositionstart', onCompositionStart)
  ta.addEventListener('input', onInput)
  const onDataDisp = term.onData(() => policy.xtermData())
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
    ...terminalAppearanceOptions(appStore.terminalPreferences),
    allowProposedApi: true,
    macOptionIsMeta: true,
    scrollback: 10000,
  })

  rendererPreferences.set(term, appStore.terminalPreferences.renderer === 'webgl')

  const fitAddon = new FitAddon()
  term.loadAddon(fitAddon)

  // disableStdin also suppresses parser replies, so it must stay off.
  // The pinned xterm user-input signal separates hidden user events from
  // parser replies. Background DSR/DA replies must keep the Legacy CLI live;
  // payload bytes themselves never establish provenance.
  const userSignal = (term as unknown as XtermProvenanceSource)._core?.coreService?.onUserInput
  let pendingUserSignals = 0
  userSignal?.(() => { ++pendingUserSignals })
  term.onData(data => {
    const fromUser = pendingUserSignals > 0
    if (fromUser) --pendingUserSignals
    if (!isVisibleTab(tabId) && (fromUser || !userSignal)) return
    const instance = terminalInstances.get(tabId)
    if (instance) {
      // ptyId 空（fit 在 spawn 前发生）时跳过发送；Escape 仍清 working 状态
      if (instance.ptyId) {
      const pasteLike = data.includes('\x1b[200~') || data.includes('\x1b[201~')
      ptyInput(instance.ptyId, data, pasteLike ? 'xterm-ondata-paste' : 'terminal-ondata')
    }

      // A local Escape invalidates an activity hint; it does not prove CLI idle/exit.
      if (data === '\x1b') {
        const tab = sessionStore.tabs.get(tabId)
        if (tab) { tab.working = false; tab.activity = 'unknown' }
      }
    }
  })

  // 终端尺寸变化 → resize 对应的 PTY
  term.onResize(({ cols, rows }) => {
    const instance = terminalInstances.get(tabId)
    if (!isVisibleTab(tabId)) { needsFit.add(tabId); return }
    if (instance && instance.ptyId) {
      ptyResize(instance.ptyId, cols, rows)
    }
  })

  // 复制粘贴处理
  term.attachCustomKeyEventHandler((event: KeyboardEvent) => {
    if (!isVisibleTab(tabId)) return false
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
    if (isPasteShortcut(event)) {
      event.preventDefault()
      // 不走 term.paste：xterm 会把 \r?\n 转成 \r（回车），在 Claude 的 Ink TUI 里
      // 触发光标回行首、后续覆盖前面（表现为"只显尾部"）。这里用 commitPaste 走完整
      // 流程：capture ptyId → readText → isPasteStale 复核（防 restart 重建后写到新 PTY）
      // → 构造 payload（原文规范化 LF + bracketed 包装，见 utils/pasteText.ts）。
      // JSON 不再自动压缩；Windows 粘贴帧由 Rust 生产 writer 保护。
      // 剪贴板无文本（截图场景 readText reject）时经 imageFallback 转发 CLI 图片粘贴键
      // 字节，由 CLI 自行读剪贴板插 [Image #N]（键位契约见 docs/interaction.md）。
      commitPasteWithEvidence(
        readText,
        readImage,
        () => isVisibleTab(tabId) ? terminalInstances.get(tabId) : undefined,
        text => buildPastePayload(text, term.modes.bracketedPasteMode, term.options.ignoreBracketedPasteMode ?? false),
        (id, payload) => ptyInput(id, payload, 'clipboard-keyboard'),
        () => imagePasteBytes(platform),
      ).catch(() => {})
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
  // 捕获阶段拦截原生 DOM paste，抢在 xterm 的 textarea/element 粘贴监听之前；
  // 注册必须先于任何 await，消除启动窗口期漏拦
  if (containerRef.value) {
    unbindNativePaste = bindNativePaste({
      container: containerRef.value,
      getTabId: () => visible.value ? currentDisplayTabId.value : null,
      getInstance: tabId => isVisibleTab(tabId) ? terminalInstances.get(tabId) : undefined,
      write: (id, payload) => ptyInput(id, payload, 'clipboard-dom'),
      imageFallback: () => imagePasteBytes(platform),
    })
  }
  // Drag/drop is optional. Only output/exit registration gates process spawn.
  void setupDragDropListener().catch(() => {})
  await ensureCoreListeners().catch(() => { /* startTab reports the safe readiness error. */ })
  if (disposed) { unlistenPtyOutput?.(); unlistenPtyExit?.(); unlistenDragDrop?.(); return }
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
          fitTab(tabId)
          instance.term.refresh(0, instance.term.rows - 1)
          instance.term.scrollToBottom()
        }
      }
    } else {
      isMinimized = minimized
    }
  })

  if (disposed) { unlistenWindowResized?.(); return }
  unregisterCommand = registerTerminalCommand(sendText)
  for (const tab of sessionStore.tabs.values()) {
    if (tab.ptyId && tab.status === 'running') await createTerminalForTab(tab.tabId, tab.ptyId)
  }
})

// 设置事件监听器
async function setupDragDropListener() {
  // 文件拖放 → 将路径输入终端
  // 项目内文件转换为相对路径，便于 Claude 直接引用
  const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
    if (!visible.value) { isDragOver.value = false; return }
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

  if (disposed) unlisten()
  else unlistenDragDrop = unlisten
}

let coreListenersReady: Promise<void> | null = null
function ensureCoreListeners(): Promise<void> {
  if (disposed) return Promise.reject(new Error('LEGACY_TERMINAL_NOT_READY'))
  if (unlistenPtyOutput && unlistenPtyExit) return Promise.resolve()
  if (coreListenersReady) return coreListenersReady
  const registerOutput = async () => {
    if (unlistenPtyOutput) return
    const unlisten = await onPtyOutput(({ id, data }) => {
      if (disposed) return
      const tabId = ptyToTab.get(id)
      if (!tabId) return
      const instance = terminalInstances.get(tabId)
      if (instance) instance.term.write(data)
    })
    if (disposed) unlisten()
    else unlistenPtyOutput = unlisten
  }
  const registerExit = async () => {
    if (unlistenPtyExit) return
    const unlisten = await onPtyExit(({ id }) => {
      if (disposed) return
      const tabId = ptyToTab.get(id)
      sessionStore.handlePtyExit(id)
      hookStore.clearSession(id)
      if (!tabId) return
      const instance = terminalInstances.get(tabId)
      // An ended open terminal still owns its scrollback until close/restart.
      if (instance?.ptyId === id) instance.ptyId = ''
      ptyToTab.unlink(id)
      emit('ptyExited', tabId, id)
    })
    if (disposed) unlisten()
    else unlistenPtyExit = unlisten
  }
  coreListenersReady = Promise.allSettled([registerOutput(), registerExit()]).then(results => {
    if (disposed || results.some(result => result.status === 'rejected')) throw new Error('LEGACY_LISTENER_UNAVAILABLE')
  }).catch(error => {
    // Wait for both registrations to settle before allowing an explicit retry;
    // keep any successful listener, so partial failure cannot double-subscribe.
    coreListenersReady = null
    throw error
  })
  return coreListenersReady
}

// Both runtimes consume this same store-owned computed preference object.
watch(() => appStore.terminalPreferences, (next, previous) => {
  let metricsChanged = false
  for (const instance of terminalInstances.values()) {
    if (applyTerminalAppearance(instance.term.options, next, previous)) metricsChanged = true
  }
  if (metricsChanged) fitVisible()
})

// 监听活跃 Tab 变化 → 切换显示
watch(() => sessionStore.activeTabId, async (newTabId, oldTabId) => {
  currentDisplayTabId.value = newTabId
  if (!newTabId) return

  if (newTabId === oldTabId) {
    fitCurrentTerminal()
    return
  }

  await nextTick()

  const existingInstance = terminalInstances.get(newTabId)

  if (existingInstance) {
    const buf = existingInstance.term.buffer.active
    existingInstance.term.refresh(0, Math.max(buf.length - 1, 0))
    fitTab(newTabId)
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
    if (!term.element) term.open(el)
    void loadRendererAddons(term)
    fitTab(tabId)
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

  if (tab.ptyId && tab.status === 'running') {
    if (!terminalInstances.has(tabId)) {
      await createTerminalForTab(tabId, tab.ptyId)
    }
    return { ok: true }
  }

  isPtyStarting.value = true
  let startingPtyId: string | null = null
  let startingGeneration = tab.ptyGeneration ?? 0

  try {
    await ensureCoreListeners()
    if (disposed || sessionStore.tabs.get(tabId) !== tab || (tab.ptyGeneration ?? 0) !== startingGeneration) throw new Error('STALE_LEGACY_ATTEMPT')
    const args = buildClaudeArgs(tab)
    const cwd = tab.projectPath
    const ptyId = createPtyId()
    startingPtyId = ptyId
    const term = createTerminal(tabId)
    const fitAddon = new FitAddon()
    term.loadAddon(fitAddon)

    // 先注册路由和 store，再启动后端进程。即使 CLI 立即输出或退出，事件也能定位 tab。
    terminalInstances.set(tabId, { term, fitAddon, ptyId })
    sessionStore.setTabPty(tabId, ptyId)
    startingGeneration = tab.ptyGeneration ?? 0
    ptyToTab.link(ptyId, tabId)
    sessionStore.setActiveTab(tabId)
    currentDisplayTabId.value = tabId

    let cols = 80
    let rows = 24
    const el = await waitForElement(tabId)
    if (el) {
      if (!term.element) term.open(el)
      void loadRendererAddons(term)
      fitTab(tabId)
      cols = term.cols
      rows = term.rows
    }

    if (disposed || sessionStore.tabs.get(tabId) !== tab || tab.ptyId !== ptyId) throw new Error('STALE_LEGACY_ATTEMPT')
    const info = await ptySpawn({
      id: ptyId,
      cwd,
      cols,
      rows,
      type: 'claude',
      args,
    })

    if (!info || info.id !== ptyId) {
      throw new Error('PTY spawn returned an unexpected identifier')
    }

    const liveInstance = terminalInstances.get(tabId)
    const liveTab = sessionStore.tabs.get(tabId)
    if (
      liveInstance?.ptyId !== ptyId ||
      liveTab?.ptyId !== ptyId ||
      liveTab.status !== 'running'
    ) {
      discardUnstartedTab(tabId, startingPtyId, startingGeneration)
      return { ok: false, error: 'PTY exited during startup' }
    }

    emit('ptyStarted', tabId, ptyId)
    return { ok: true }
  } catch (err) {
    discardUnstartedTab(tabId, startingPtyId, startingGeneration)
    console.error('[XTerm] startTab ERROR:', err)
    void logMessage('error', `startTab failed, tabId=${tabId}: ${err}`)
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
function discardUnstartedTab(tabId: string, ptyId: string | null, generation: number) {
  const candidate = terminalInstances.get(tabId)
  const instance = candidate?.ptyId === ptyId ? candidate : undefined
  if (ptyId) {
    ptyToTab.unlink(ptyId)
    hookStore.clearSession(ptyId)
    useAttentionStore().clearPty(ptyId)
    void ptyKill(ptyId).catch(() => {})
  }
  if (instance) {
    void disposeTerminal(instance.term, `discardUnstartedTab(tabId=${tabId})`)
    terminalInstances.delete(tabId)
    terminalEls.delete(tabId)
  }
  const tab = sessionStore.tabs.get(tabId)
  if (tab && (tab.ptyGeneration ?? 0) === generation && (tab.ptyId === ptyId || tab.ptyId === null)) sessionStore.removeTab(tabId)
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
  if (!tab) throw new Error('LEGACY_SESSION_NOT_FOUND')
  if (isPtyStarting.value) throw new Error('LEGACY_LAUNCH_BUSY')
  const generation = tab.ptyGeneration ?? 0
  const owns = () => !disposed && sessionStore.tabs.get(tabId) === tab && (tab.ptyGeneration ?? 0) === generation
  const oldInstance = terminalInstances.get(tabId)
  isPtyStarting.value = true
  try {
    await stopTab(tabId)
    if (!owns()) throw new Error('STALE_LEGACY_ATTEMPT')
    if (oldInstance) {
      await disposeTerminal(oldInstance.term, `restartTab(tabId=${tabId})`)
      if (!owns() || terminalInstances.get(tabId) !== oldInstance) throw new Error('STALE_LEGACY_ATTEMPT')
      terminalInstances.delete(tabId)
      terminalEls.delete(tabId)
    }
  } finally {
    isPtyStarting.value = false
  }
  if (!owns()) throw new Error('STALE_LEGACY_ATTEMPT')
  const result = await startTab(tabId)
  if (!result.ok) throw new Error('LEGACY_LAUNCH_FAILED')
  appStore.resetClaudeOptions()
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
  if (!visible.value) return false
  const tabId = currentDisplayTabId.value
  if (!tabId) return false
  const instance = terminalInstances.get(tabId)
  if (instance?.ptyId) {
    void ptyInput(instance.ptyId, text)
    instance.term.focus()
    return true
  }
  return false
}

// 聚焦活跃终端
function focus() {
  if (!visible.value) return
  const tabId = currentDisplayTabId.value
  if (!tabId) return
  const instance = terminalInstances.get(tabId)
  if (instance) instance.term.focus()
}

// Lifecycle calls capture the exact PTY before awaiting. No completion resolves
// a new current tab or selects another runtime to finish an old operation.
async function stopTab(tabId: string) {
  const ptyId = sessionStore.tabs.get(tabId)?.ptyId
  if (!ptyId) return
  await ptyKill(ptyId)
  sessionStore.handlePtyExit(ptyId)
  hookStore.clearSession(ptyId)
  ptyToTab.unlink(ptyId)
  const instance = terminalInstances.get(tabId)
  if (instance?.ptyId === ptyId) instance.ptyId = ''
}
async function recover() { throw new Error('LEGACY_RECOVERY_UNSUPPORTED') }
async function renameTab(tabId: string, title: string) {
  if (!title.trim() || /[\x00-\x1f\x7f]/.test(title)) throw new Error('SESSION_TITLE_REQUIRED')
  const tab = sessionStore.tabs.get(tabId)
  if (!tab) throw new Error('LEGACY_SESSION_NOT_FOUND')
  const ptyId = tab.ptyId
  if (tab.status === 'running' && ptyId) await ptyInput(ptyId, `/rename ${title}\r`)
}
watch(() => props.visible, async () => {
  for (const id of terminalInstances.keys()) needsFit.add(id)
  if (!visible.value) return
  await nextTick()
  fitVisible()
})
watch(() => [...sessionStore.tabs.keys()], () => {
  for (const id of terminalInstances.keys()) {
    if (!sessionStore.tabs.has(id)) { disposeTabInstance(id); needsFit.delete(id) }
  }
})

// 兼容：重启当前活跃 Tab
async function restartCurrentPty() {
  if (sessionStore.activeTabId) {
    await restartTab(sessionStore.activeTabId)
  }
}

onUnmounted(() => {
  disposed = true
  pendingFits.clear()
  unregisterCommand?.()
  fitCurrentTerminal.cancel()
  resizeObserver?.disconnect()
  window.removeEventListener('copy', handleNativeCopy)
  unbindNativePaste?.()
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
  fitCurrentTerminal, fitVisible, stopTab, stop: stopTab, recover, renameTab,
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
