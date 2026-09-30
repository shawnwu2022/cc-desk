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
import { publicNativeErrorCode } from '@/utils/nativeErrorCode'
import { cliResize, cliStop } from '@/api/tauri'
import type { OutputFrame } from '@/types/terminal'
import { useAppStore } from '@/stores/app'
import { getTerminalTheme } from '@/config/terminalThemes'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import {
  captureNativeAttempt,
  matchesNativeAttempt,
  useNativeTabsStore,
  type NativeAttemptIdentity,
} from '@/stores/nativeTabs'

const props = defineProps<{
  tabId: string
  active: boolean
}>()

const container = ref<HTMLElement | null>(null)
const profiles = useCliProfilesStore()
const app = useAppStore()
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
let statusSyncInFlight: object | null = null
let disposed = false
const needsFit = ref(true)
let startedAttempt: NativeAttemptIdentity | null = null
let startPromise: Promise<void> | null = null

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

function currentAttempt(): NativeAttemptIdentity {
  return captureNativeAttempt(currentTab())
}

function attemptIsCurrent(attempt: NativeAttemptIdentity): boolean {
  return !disposed && matchesNativeAttempt(tabs.tab(props.tabId), attempt)
}

function safeLaunchCode(error: unknown): string {
  return publicNativeErrorCode(error, 'NATIVE_LAUNCH_FAILED')
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
  statusSyncInFlight = null
  launched = false
  inputEnabled = false
  stopStatusSync()
  binding?.dispose()
  binding = null
}

function markInputFailure(attempt: NativeAttemptIdentity) {
  if (attemptIsCurrent(attempt)) {
    tabs.setDiagnostic(props.tabId, 'NATIVE_INPUT_PAUSED')
  }
}

function bindClipboard() {
  if (!container.value || !term) return
  const host = container.value
  pasteListener = (event: ClipboardEvent) => {
    if (!props.active || !inputEnabled || !term || !binding) return
    const target = event.target as Node | null
    if (!target || !term.element?.contains(target)) return

    const snapshot = classifyClipboardSnapshot({
      text: event.clipboardData?.getData('text/plain') ?? '',
      types: Array.from(event.clipboardData?.types ?? []),
    })
    if (snapshot.kind !== 'text' && snapshot.kind !== 'image') return

    event.preventDefault()
    event.stopPropagation()
    const attempt = currentAttempt()

    if (snapshot.kind === 'image') {
      // Positive image MIME evidence preserves the CLI's native image-paste
      // shortcut. Route the key bytes through the same ordered native writer.
      void binding.sendUserText(imagePasteBytes(platform)).catch(() => markInputFailure(attempt))
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
    void reserved.settled.catch(() => markInputFailure(attempt))
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
    if (props.active && inputEnabled && text && binding) {
      const attempt = currentAttempt()
      void binding.sendUserText(text).catch(() => markInputFailure(attempt))
    }
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

function handleNativeCopy(event: ClipboardEvent) {
  if (!props.active || !term?.element?.contains(document.activeElement)) return
  const selection = term.getSelection()
  if (!selection) return
  event.preventDefault()
  void writeText(selection).catch(() => {})
}

// Do not use xterm disableStdin for visibility: it also suppresses parser replies.
// User-event/provenance admission owns the hidden-input boundary.
function configureCopy() {
  if (!term) return
  term.attachCustomKeyEventHandler(event => {
    if (!props.active) return false
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

function start(): Promise<void> {
  const attempt = currentAttempt()
  if (startedAttempt && matchesNativeAttempt(startedAttempt, attempt)) return startPromise ?? Promise.resolve()
  // A remounted known/unknown attempt has no safe new output route. Never replay it.
  if (currentTab().status !== 'stopped' || currentTab().launchRevision !== null) {
    return Promise.reject(new Error('NATIVE_ATTACH_UNAVAILABLE'))
  }
  startedAttempt = attempt
  startPromise = startAttempt(attempt)
  return startPromise
}

async function startAttempt(attempt: NativeAttemptIdentity): Promise<void> {
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
        if (!inputEnabled || runToken !== token || !attemptIsCurrent(attempt)) throw new Error('NATIVE_RUN_NOT_WRITABLE')
        return {
          runId,
          generation,
          modeEpoch: refreshModeEpoch(),
        }
      },
      isUserInputAllowed: () => props.active && inputEnabled && attemptIsCurrent(attempt),
      onDegraded: reason => {
        const live = tabs.tab(props.tabId)
        if (runToken === token && live?.runId === runId && live.generation === generation) {
          tabs.setDiagnostic(props.tabId, reason)
        }
      },
    })
  } catch (error) {
    if (runToken === token && attemptIsCurrent(attempt)) tabs.markError(props.tabId, safeLaunchCode(error))
    return
  }

  const channel = new Channel<OutputFrame>()
  channel.onmessage = frame => {
    const live = tabs.tab(props.tabId)
    if (
      runToken !== token
      || !attemptIsCurrent(attempt)
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
    if (runToken !== token || !attemptIsCurrent(attempt)) return
    if (!tabs.applyLaunchStatus(props.tabId, result)) return
    launched = result.phase === 'running' || result.phase === 'starting'
    inputEnabled = launched
    if (launched) {
      await resizeNative(term.cols, term.rows)
      if (runToken !== token || !attemptIsCurrent(attempt)) return
      startStatusSync()
    } else {
      stopStatusSync()
    }
  } catch (error) {
    if (runToken !== token || !attemptIsCurrent(attempt)) return
    const code = safeLaunchCode(error)
    inputEnabled = false
    if (code === 'LAUNCH_STATE_UNKNOWN') tabs.markUnknown(props.tabId)
    else tabs.markError(props.tabId, code)
  }
}

async function recover(attempt: NativeAttemptIdentity = currentAttempt()): Promise<void> {
  if (!attemptIsCurrent(attempt)) throw new Error('STALE_NATIVE_ATTEMPT')
  try {
    const result = await entry.recover(attempt.requestId)
    if (!attemptIsCurrent(attempt)) return
    if (!tabs.applyLaunchStatus(props.tabId, result)) return
    launched = result.phase === 'running' || result.phase === 'starting'
    inputEnabled = launched
    if (!launched) stopStatusSync()
    else if (!statusTimer && !statusSyncInFlight) startStatusSync()
  } catch (error) {
    if (!attemptIsCurrent(attempt)) return
    const code = safeLaunchCode(error)
    launched = false
    inputEnabled = false
    stopStatusSync()
    // A failed status read is not evidence that an admitted process ended.
    tabs.markUnknown(props.tabId)
    tabs.setDiagnostic(props.tabId, code)
  }
}

async function syncStatus(): Promise<void> {
  if (statusSyncInFlight || !launched) return
  const syncOwner = {}
  statusSyncInFlight = syncOwner
  try {
    await recover()
  } finally {
    if (statusSyncInFlight === syncOwner) statusSyncInFlight = null
  }
}

function startStatusSync() {
  stopStatusSync()
  if (!launched) return
  statusTimer = setInterval(() => {
    void syncStatus()
  }, 1500)
}

async function stop(attempt: NativeAttemptIdentity = currentAttempt()): Promise<void> {
  if (!attemptIsCurrent(attempt)) throw new Error('STALE_NATIVE_ATTEMPT')
  try {
    await cliStop({ runId: attempt.runId, generation: attempt.generation })
    if (!attemptIsCurrent(attempt)) return
    await recover(attempt)
    if (!attemptIsCurrent(attempt)) return
    if (!['stopped', 'exited', 'failed'].includes(currentTab().status)) throw new Error('NATIVE_STOP_UNCONFIRMED')
  } catch (error) {
    if (!attemptIsCurrent(attempt)) return
    tabs.setDiagnostic(props.tabId, safeLaunchCode(error))
    throw error
  }
}

function focus() {
  if (props.active && !disposed) term?.focus()
}

function fitVisible() {
  needsFit.value = true
  if (!props.active || !term || !fit || disposed) return
  fit.fit()
  needsFit.value = false
  void resizeNative(term.cols, term.rows)
}

onMounted(async () => {
  if (!container.value) return
  term = new Terminal({
    fontFamily: '"Cascadia Code", "Fira Code", "JetBrains Mono", Consolas, monospace',
    fontSize: app.fontSize,
    theme: getTerminalTheme(app.terminalTheme),
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
  fitVisible()
  configureCopy()
  window.addEventListener('copy', handleNativeCopy)
  bindClipboard()
  // D19 provenance binding is installed by start(); bind IME after it exists so
  // xtermData observation and explicit fallback share the same native writer.
  resizeObserver = new ResizeObserver(() => {
    needsFit.value = true
    if (props.active) requestAnimationFrame(fitVisible)
  })
  resizeObserver.observe(container.value)

  await start().catch(() => { /* A known attempt must be recovered, never relaunched. */ })
  if (disposed) return
  bindImeFallback()
  if (props.active) await nextTick().then(focus)
})

watch(() => props.active, async active => {
  needsFit.value = true
  if (!active) return
  await nextTick()
  fitVisible()
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

watch(() => app.fontSize, size => {
  if (term) term.options.fontSize = size
  fitVisible()
})
watch(() => app.terminalTheme, theme => {
  if (term) term.options.theme = getTerminalTheme(theme)
})

onUnmounted(() => {
  disposed = true
  window.removeEventListener('copy', handleNativeCopy)
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

defineExpose({ start, recover, stop, focus, fitVisible, needsFit })
</script>

<style scoped>
.native-cli-terminal {
  position: absolute;
  inset: 0;
  display: none;
  padding: 6px;
  background: var(--terminal-surface-bg);
}

.native-cli-terminal.active {
  display: block;
}

.native-cli-terminal :deep(.xterm) {
  height: 100%;
}
</style>
