<template>
  <div
    ref="container"
    class="native-cli-terminal"
    :class="{ active }"
    :data-native-tab="tabId"
  ></div>
</template>

<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { Channel } from '@tauri-apps/api/core'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { createNativeLaunchEntry } from '@/terminal/nativeLaunchEntry'
import { createDeskNativeTerminalBinding } from '@/terminal/deskNativeTerminal'
import type { NativeTerminalBinding } from '@/terminal/nativeTerminalBinding'
import { buildPastePayload, imagePasteBytes } from '@/utils/pasteText'
import { classifyClipboardSnapshot, createImeInputPolicy } from '@/terminal/inputPolicy'
import { platform } from '@/utils/platform'
import { cliResize, cliStop } from '@/api/tauri'
import type { OutputFrame } from '@/types/terminal'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useNativeTabsStore } from '@/stores/nativeTabs'

const props = defineProps<{
  tabId: string
  active: boolean
}>()

const container = ref<HTMLElement | null>(null)
const profiles = useCliProfilesStore()
const tabs = useNativeTabsStore()

let term: Terminal | null = null
let fit: FitAddon | null = null
let binding: NativeTerminalBinding | null = null
let resizeObserver: ResizeObserver | null = null
let pasteListener: ((event: ClipboardEvent) => void) | null = null
let imeCleanup: (() => void) | null = null
let runToken: object = {}
let observedBracketed = false
let modeEpoch = BigInt(1)
let launched = false
let inputEnabled = false
let statusTimer: ReturnType<typeof setInterval> | null = null
let statusSyncInFlight = false

const entry = createNativeLaunchEntry({
  selectedProfile(cli) {
    const tab = tabs.tab(props.tabId)
    if (!tab || tab.cli !== cli) return null
    const profile = profiles.profile(tab.profileId)
    if (!profile || profile.cli !== cli || profile.revision !== tab.profileRevision) return null
    return profile
  },
})

function currentTab() {
  const tab = tabs.tab(props.tabId)
  if (!tab) throw new Error('TAB_NOT_FOUND')
  return tab
}

function safeLaunchCode(error: unknown): string {
  if (error instanceof Error) {
    if ([
      'LAUNCH_STATE_UNKNOWN',
      'CLI_PROFILE_REQUIRED',
      'PROFILE_CLI_MISMATCH',
      'LAUNCH_REQUEST_ID_CONFLICT',
      'LAUNCH_ATTEMPT_NOT_FOUND',
      'LAUNCH_ATTEMPT_NOT_READY',
      'XTERM_USER_INPUT_PROVENANCE_UNAVAILABLE',
    ].includes(error.message)) return error.message
  }
  if (error && typeof error === 'object' && 'code' in error) {
    const code = (error as { code?: unknown }).code
    if (typeof code === 'string' && /^[A-Z][A-Z0-9_]{0,63}$/.test(code)) return code
  }
  return 'NATIVE_LAUNCH_FAILED'
}

function refreshModeEpoch(): string {
  if (!term) return modeEpoch.toString()
  const current = term.modes.bracketedPasteMode
  if (current !== observedBracketed) {
    observedBracketed = current
    modeEpoch += BigInt(1)
  }
  return modeEpoch.toString()
}

function stopStatusSync() {
  if (statusTimer) clearInterval(statusTimer)
  statusTimer = null
}

function disposeRunBinding() {
  runToken = {}
  launched = false
  inputEnabled = false
  stopStatusSync()
  binding?.dispose()
  binding = null
}

function markInputFailure() {
  const tab = tabs.tab(props.tabId)
  if (tab) tabs.setDiagnostic(props.tabId, 'NATIVE_INPUT_PAUSED')
}

function bindClipboard() {
  if (!container.value || !term) return
  const host = container.value
  pasteListener = (event: ClipboardEvent) => {
    if (!term || !binding) return
    const target = event.target as Node | null
    if (!target || !term.element?.contains(target)) return

    const snapshot = classifyClipboardSnapshot({
      text: event.clipboardData?.getData('text/plain') ?? '',
      types: Array.from(event.clipboardData?.types ?? []),
    })
    if (snapshot.kind !== 'text' && snapshot.kind !== 'image') return

    event.preventDefault()
    event.stopPropagation()

    if (snapshot.kind === 'image') {
      // Positive image MIME evidence preserves the CLI's native image-paste
      // shortcut. Route the key bytes through the same ordered native writer.
      void binding.sendUserText(imagePasteBytes(platform)).catch(markInputFailure)
      return
    }

    const payload = buildPastePayload(
      snapshot.text,
      term.modes.bracketedPasteMode,
      term.options.ignoreBracketedPasteMode ?? false,
    )
    if (!payload) return
    const bytes = new TextEncoder().encode(payload)
    const reserved = binding.reserveUserPaste(async () => bytes)
    void reserved.settled.catch(markInputFailure)
  }
  host.addEventListener('paste', pasteListener, true)
}

function bindImeFallback() {
  if (!term?.textarea) return
  const textarea = term.textarea
  const policy = createImeInputPolicy()
  const keyDown = () => policy.keyDown()
  const keyUp = () => policy.keyUp()
  const compositionStart = () => policy.compositionStart()
  const input = (event: Event) => {
    const value = event as InputEvent
    const text = policy.input({
      inputType: value.inputType,
      composed: value.composed,
      data: value.data,
    })
    if (text && binding) void binding.sendUserText(text).catch(markInputFailure)
  }
  const xtermData = term.onData(() => policy.xtermData())

  textarea.addEventListener('keydown', keyDown)
  textarea.addEventListener('keyup', keyUp)
  textarea.addEventListener('compositionstart', compositionStart)
  textarea.addEventListener('input', input)
  imeCleanup = () => {
    xtermData.dispose()
    textarea.removeEventListener('keydown', keyDown)
    textarea.removeEventListener('keyup', keyUp)
    textarea.removeEventListener('compositionstart', compositionStart)
    textarea.removeEventListener('input', input)
  }
}

function configureCopy() {
  if (!term) return
  term.attachCustomKeyEventHandler(event => {
    if (event.type !== 'keydown') return true
    const copy = (
      (event.ctrlKey && !event.metaKey && !event.shiftKey && event.key.toLowerCase() === 'c')
      || (event.metaKey && !event.ctrlKey && event.key.toLowerCase() === 'c')
      || (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 'c')
    )
    if (!copy) return true
    const selection = term?.getSelection() ?? ''
    if (!selection) return true
    event.preventDefault()
    void writeText(selection)
    return false
  })
}

async function resizeNative(cols: number, rows: number) {
  const tab = tabs.tab(props.tabId)
  if (!launched || !tab || (tab.status !== 'running' && tab.status !== 'starting')) return
  await cliResize(
    { runId: tab.runId, generation: tab.generation },
    cols,
    rows,
  ).catch(() => {
    // Resize loss is visible via later terminal/run state; never reroute through legacy PTY.
  })
}

async function start(): Promise<void> {
  if (!term || !fit) throw new Error('NATIVE_TERMINAL_NOT_READY')
  const tab = currentTab()
  const token = {}
  runToken = token
  disposeRunBinding()
  runToken = token
  observedBracketed = term.modes.bracketedPasteMode
  modeEpoch = BigInt(1)

  const runId = tab.runId
  const generation = tab.generation
  tabs.markStarting(props.tabId)

  try {
    binding = createDeskNativeTerminalBinding({
      term: term as any,
      runId,
      generation,
      currentTarget: () => {
        if (!inputEnabled) throw new Error('NATIVE_RUN_NOT_WRITABLE')
        return {
          runId,
          generation,
          modeEpoch: refreshModeEpoch(),
        }
      },
      onDegraded: reason => {
        const live = tabs.tab(props.tabId)
        if (runToken === token && live?.runId === runId && live.generation === generation) {
          tabs.setDiagnostic(props.tabId, reason)
        }
      },
    })
  } catch (error) {
    if (runToken === token) tabs.markError(props.tabId, safeLaunchCode(error))
    return
  }

  const channel = new Channel<OutputFrame>()
  channel.onmessage = frame => {
    const live = tabs.tab(props.tabId)
    if (
      runToken !== token
      || !binding
      || !live
      || live.runId !== runId
      || live.generation !== generation
    ) return
    if (!binding.acceptOutput(frame)) tabs.setDiagnostic(props.tabId, 'NATIVE_OUTPUT_DEGRADED')
  }

  try {
    const result = await entry.start({
      requestId: tab.requestId,
      tabId: tab.tabId,
      runId,
      generation,
      cli: tab.cli,
      launchCwd: tab.projectPath,
      action: tab.action,
      extraArgs: [],
      cols: term.cols,
      rows: term.rows,
    }, channel)
    if (runToken !== token) return
    if (!tabs.applyLaunchStatus(props.tabId, result)) return
    launched = result.phase === 'running' || result.phase === 'starting'
    inputEnabled = launched
    if (launched) {
      await resizeNative(term.cols, term.rows)
      startStatusSync()
    } else {
      stopStatusSync()
    }
  } catch (error) {
    if (runToken !== token) return
    const code = safeLaunchCode(error)
    inputEnabled = false
    if (code === 'LAUNCH_STATE_UNKNOWN') tabs.markUnknown(props.tabId)
    else tabs.markError(props.tabId, code)
  }
}

async function recover(): Promise<void> {
  const tab = currentTab()
  try {
    const result = await entry.recover(tab.requestId)
    tabs.applyLaunchStatus(props.tabId, result)
    launched = result.phase === 'running' || result.phase === 'starting'
    inputEnabled = launched
    if (!launched) stopStatusSync()
    else if (props.active && !statusTimer && !statusSyncInFlight) startStatusSync()
  } catch (error) {
    const code = safeLaunchCode(error)
    launched = false
    inputEnabled = false
    stopStatusSync()
    if (code === 'LAUNCH_STATE_UNKNOWN') tabs.markUnknown(props.tabId)
    else tabs.markError(props.tabId, code)
  }
}

async function syncStatus(): Promise<void> {
  if (statusSyncInFlight || !props.active || !launched) return
  statusSyncInFlight = true
  try {
    await recover()
  } finally {
    statusSyncInFlight = false
  }
}

function startStatusSync() {
  stopStatusSync()
  if (!props.active || !launched) return
  statusTimer = setInterval(() => {
    void syncStatus()
  }, 1500)
}

async function stop(): Promise<void> {
  const tab = currentTab()
  try {
    await cliStop({ runId: tab.runId, generation: tab.generation })
    await recover()
  } catch (error) {
    tabs.setDiagnostic(props.tabId, safeLaunchCode(error))
    throw error
  }
}

function focus() {
  term?.focus()
}

onMounted(async () => {
  if (!container.value) return
  term = new Terminal({
    fontFamily: '"Cascadia Code", "Fira Code", "JetBrains Mono", Consolas, monospace',
    fontSize: 12,
    lineHeight: 1.2,
    cursorBlink: true,
    cursorStyle: 'bar',
    scrollback: 10000,
    allowProposedApi: true,
    macOptionIsMeta: true,
  })
  fit = new FitAddon()
  term.loadAddon(fit)
  term.open(container.value)
  fit.fit()
  configureCopy()
  bindClipboard()
  // D19 provenance binding is installed by start(); bind IME after it exists so
  // xtermData observation and explicit fallback share the same native writer.
  resizeObserver = new ResizeObserver(() => {
    if (!fit || !term) return
    requestAnimationFrame(() => {
      if (!fit || !term) return
      fit.fit()
      void resizeNative(term.cols, term.rows)
    })
  })
  resizeObserver.observe(container.value)

  await start()
  bindImeFallback()
  if (props.active) await nextTick().then(focus)
})

watch(() => props.active, async active => {
  if (!active) {
    stopStatusSync()
    return
  }
  await nextTick()
  fit?.fit()
  focus()
  if (launched) {
    void syncStatus()
    startStatusSync()
  }
})

watch(
  () => tabs.tab(props.tabId)?.generation,
  async (generation, previous) => {
    if (!generation || previous === undefined || generation === previous) return
    await start()
  },
)

onUnmounted(() => {
  stopStatusSync()
  disposeRunBinding()
  imeCleanup?.()
  imeCleanup = null
  if (pasteListener && container.value) {
    container.value.removeEventListener('paste', pasteListener, true)
  }
  pasteListener = null
  resizeObserver?.disconnect()
  resizeObserver = null
  term?.dispose()
  term = null
  fit = null
})

defineExpose({ start, recover, stop, focus })
</script>

<style scoped>
.native-cli-terminal {
  position: absolute;
  inset: 0;
  display: none;
  padding: 6px;
  background: var(--bg-primary);
}

.native-cli-terminal.active {
  display: block;
}

.native-cli-terminal :deep(.xterm) {
  height: 100%;
}
</style>
