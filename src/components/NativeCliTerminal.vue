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
import { createNativeLaunchEntry, NativeLaunchNotSubmittedError } from '@/terminal/nativeLaunchEntry'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'
import { createDeskNativeTerminalBinding } from '@/terminal/deskNativeTerminal'
import type { NativeTerminalBinding } from '@/terminal/nativeTerminalBinding'
import { createXtermModeEpoch, type XtermModeEpoch } from '@/terminal/modeEpoch'
import { buildPastePayload, imagePasteBytes } from '@/utils/pasteText'
import { classifyClipboardSnapshot, createImeInputPolicy } from '@/terminal/inputPolicy'
import { platform } from '@/utils/platform'
import { publicNativeErrorCode } from '@/utils/nativeErrorCode'
import { cliResize, cliStop } from '@/api/tauri'
import type { OutputFrame } from '@/types/terminal'
import { useHookStore } from '@/stores/hook'
import { useAppStore } from '@/stores/app'
import { terminalAppearanceOptions, applyTerminalAppearance } from '@/config/terminalPreferences'
import type { WebglAddon } from '@xterm/addon-webgl'
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
let webglAddon: WebglAddon | null = null
let preferenceFitPending = false
let binding: NativeTerminalBinding | null = null
let resizeObserver: ResizeObserver | null = null
let pasteListener: ((event: ClipboardEvent) => void) | null = null
let imeCleanup: (() => void) | null = null
let runToken: object = {}
let modeTracker: XtermModeEpoch | null = null
let launched = false
let inputEnabled = false
let stopObservation: (() => void) | null = null
let statusTimer: ReturnType<typeof setInterval> | null = null
let statusSyncInFlight: object | null = null
let disposed = false
const needsFit = ref(true)
let startedAttempt: NativeAttemptIdentity | null = null
let startPromise: Promise<void> | null = null
let terminalGeneration: number | null = null
let parserReset: { attempt: NativeAttemptIdentity; cancel: () => void } | null = null
let stoppingAttempt: NativeAttemptIdentity | null = null
let receiptPublication: object = {}

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

function stopStatusSync() {
  if (statusTimer) clearInterval(statusTimer)
  statusTimer = null
}

function disposeRunBinding() {
  runToken = {}
  receiptPublication = {}
  parserReset?.cancel()
  parserReset = null
  statusSyncInFlight = null
  launched = false
  inputEnabled = false
  stopStatusSync()
  stopObservation?.(); stopObservation = null
  binding?.dispose()
  binding = null
  modeTracker?.dispose()
  modeTracker = null
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
      items: Array.from(event.clipboardData?.items ?? [], item => ({ kind: item.kind, type: item.type })),
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

function createParserFence(target: Terminal): { promise: Promise<void>; cancel: () => void } {
  let finish!: (error?: Error) => void
  const promise = new Promise<void>((resolve, reject) => {
    let settled = false
    const timer = setTimeout(() => finish(new Error('NATIVE_TERMINAL_NOT_READY')), 5000)
    finish = error => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      if (error) reject(error)
      else resolve()
    }
    // xterm reset() leaves partial escape/UTF-8 parser state intact. Fixed VT
    // CAN + RIS bytes cancel it and reset the parser after every old queued
    // write, with provenance detached. Bytes also flush the UTF-8 decoder;
    // writing a string would leave an old byte prefix pending.
    try { target.write(Uint8Array.of(0x18, 0x1b, 0x63), () => finish()) }
    catch { finish(new Error('NATIVE_TERMINAL_NOT_READY')) }
  })
  return { promise, cancel: () => finish(new Error('NATIVE_TERMINAL_DISPOSED')) }
}

async function startAttempt(attempt: NativeAttemptIdentity): Promise<void> {
  if (!term || !fit) throw new Error('NATIVE_TERMINAL_NOT_READY')
  const target = term
  const tab = currentTab()
  const token = {}
  disposeRunBinding()
  runToken = token
  const publication = receiptPublication

  const runId = attempt.runId
  const generation = attempt.generation
  tabs.markStarting(props.tabId)
  try {
    if (terminalGeneration !== null && terminalGeneration !== generation) {
      // Keep the previous parser completely disconnected until its queued
      // output settles. A synchronous reset alone cannot cancel queued writes.
      const fence = createParserFence(target)
      const preparation = { attempt, cancel: fence.cancel }
      parserReset = preparation
      try {
        await fence.promise
        if (runToken !== token || receiptPublication !== publication || term !== target || !attemptIsCurrent(attempt)) return
        target.reset()
      } finally {
        if (parserReset === preparation) parserReset = null
      }
    }
    if (runToken !== token || receiptPublication !== publication || term !== target || !attemptIsCurrent(attempt)) return
    terminalGeneration = generation
  } catch (error) {
    if (runToken === token && receiptPublication === publication && term === target && attemptIsCurrent(attempt)) {
      // No launch request was submitted; a failed parser fence cannot admit it.
      tabs.markError(props.tabId, safeLaunchCode(error))
    }
    return
  }
  // A passive exact-run subscription never enables the optional backend observer.
  // Only its ordered projection can claim attention, never a raw hook event kind.
  try {
    stopObservation = useHookStore().subscribeObservation({ cli: tab.cli, runId, generation, enabled: true }, (_event, state) => {
      if (runToken === token && attemptIsCurrent(attempt)) tabs.applyObservation(props.tabId, attempt, state)
    })
  } catch { /* Optional observation must never block the authoritative terminal. */ }

  try {
    const tracker = createXtermModeEpoch(target)
    modeTracker = tracker
    binding = createDeskNativeTerminalBinding({
      term: target as any,
      runId,
      generation,
      currentTarget: () => {
        if (!inputEnabled || runToken !== token || !attemptIsCurrent(attempt)) throw new Error('NATIVE_RUN_NOT_WRITABLE')
        return {
          runId,
          generation,
          modeEpoch: tracker.current(),
        }
      },
      isUserInputAllowed: () => props.active && inputEnabled && attemptIsCurrent(attempt),
      onActivity: () => {
        if (runToken === token && attemptIsCurrent(attempt)) tabs.touch(props.tabId, attempt)
      },
      onDegraded: reason => {
        const live = tabs.tab(props.tabId)
        if (runToken === token && live?.runId === runId && live.generation === generation) {
          tabs.setDiagnostic(props.tabId, reason)
        }
      },
    })
  } catch (error) {
    modeTracker?.dispose()
    modeTracker = null
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
      cols: target.cols,
      rows: target.rows,
    }, channel)
    if (runToken !== token || receiptPublication !== publication || !attemptIsCurrent(attempt)) return
    if (!applyReceipt(attempt, result)) return
    if (launched && !matchesNativeAttempt(stoppingAttempt ?? undefined, attempt)) {
      await resizeNative(target.cols, target.rows)
      if (runToken !== token || !attemptIsCurrent(attempt)) return
      startStatusSync()
    } else {
      stopStatusSync()
    }
  } catch (error) {
    if (runToken !== token || receiptPublication !== publication || !attemptIsCurrent(attempt)) return
    // Recovery/cancellation can settle before a lost or late start reply. A
    // transport rejection cannot overwrite its authenticated terminal receipt.
    const latest = entry.latest(attempt.requestId)
    if (latest) {
      applyReceipt(attempt, latest)
      return
    }
    const code = safeLaunchCode(error)
    inputEnabled = false
    if (error instanceof NativeLaunchNotSubmittedError) tabs.markError(props.tabId, code)
    else {
      tabs.markUnknown(props.tabId)
      tabs.setDiagnostic(props.tabId, code)
    }
  }
}

function applyReceipt(attempt: NativeAttemptIdentity, result: LaunchStatus): boolean {
  if (!attemptIsCurrent(attempt) || !tabs.applyLaunchStatus(props.tabId, result)) return false
  launched = result.phase === 'running' || result.phase === 'starting'
  const stopping = matchesNativeAttempt(stoppingAttempt ?? undefined, attempt)
  inputEnabled = launched && !stopping
  if (!launched) stopStatusSync()
  else if (!stopping && !statusTimer && !statusSyncInFlight) startStatusSync()
  return true
}

async function recover(attempt: NativeAttemptIdentity = currentAttempt()): Promise<void> {
  if (!attemptIsCurrent(attempt)) throw new Error('STALE_NATIVE_ATTEMPT')
  const publication = receiptPublication
  try {
    const result = await entry.recover(attempt.requestId)
    if (receiptPublication !== publication) return
    applyReceipt(attempt, result)
  } catch (error) {
    if (receiptPublication !== publication || !attemptIsCurrent(attempt)) return
    const latest = entry.latest(attempt.requestId)
    if (latest && ['cancelled', 'failed', 'exited'].includes(latest.phase)) {
      applyReceipt(attempt, latest)
      return
    }
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
  if (!launched || matchesNativeAttempt(stoppingAttempt ?? undefined, currentAttempt())) return
  statusTimer = setInterval(() => {
    void syncStatus()
  }, 1500)
}

async function withinStopDeadline<T>(deadline: number, operation: () => Promise<T>): Promise<T> {
  const remaining = deadline - Date.now()
  if (remaining <= 0) throw new Error('NATIVE_STOP_UNCONFIRMED')
  let timer: ReturnType<typeof setTimeout> | undefined
  try {
    return await Promise.race([
      Promise.resolve().then(operation),
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(() => reject(new Error('NATIVE_STOP_UNCONFIRMED')), remaining)
      }),
    ])
  } finally {
    if (timer !== undefined) clearTimeout(timer)
  }
}

async function stop(attempt: NativeAttemptIdentity = currentAttempt()): Promise<void> {
  if (!attemptIsCurrent(attempt)) throw new Error('STALE_NATIVE_ATTEMPT')
  if (parserReset && matchesNativeAttempt(parserReset.attempt, attempt)) {
    // The parser transition precedes entry.start(), so cancellation has positive
    // local proof that this exact attempt never reached the backend.
    disposeRunBinding()
    tabs.markError(props.tabId, 'LAUNCH_CANCELLED')
    return
  }
  // Earlier start/status callbacks may still update their monotonic attempt
  // receipt, but only this stop or a later explicit recovery may publish it.
  receiptPublication = {}
  stoppingAttempt = attempt
  statusSyncInFlight = null
  inputEnabled = false
  stopStatusSync()
  const deadline = Date.now() + 5000
  try {
    // Cancellation uses the original full frozen request. A missing status is
    // never proof of absence: preparation may still be waiting to reserve it.
    let result = await withinStopDeadline(deadline, () => entry.cancel(attempt.requestId))
    let stopAccepted = false
    for (;;) {
      if (!attemptIsCurrent(attempt)) return
      if (!applyReceipt(attempt, result)) throw new Error('NATIVE_STOP_UNCONFIRMED')
      if (['cancelled', 'failed', 'exited'].includes(result.phase)) return
      if (Date.now() >= deadline) throw new Error('NATIVE_STOP_UNCONFIRMED')
      if (!stopAccepted && ['starting', 'running', 'indeterminate'].includes(result.phase)) {
        try {
          await withinStopDeadline(deadline, () => cliStop({ runId: attempt.runId, generation: attempt.generation }))
          stopAccepted = true
        } catch {
          // Starting can precede supervisor adoption; even RUN_NOT_FOUND or a
          // lost stop response requires another exact receipt, never closure.
        }
        if (!attemptIsCurrent(attempt)) return
      }
      await withinStopDeadline(deadline, () => new Promise(resolve => setTimeout(resolve, 100)))
      if (!attemptIsCurrent(attempt)) return
      result = await withinStopDeadline(deadline, () => entry.recover(attempt.requestId))
    }
  } catch (error) {
    if (!attemptIsCurrent(attempt)) return
    const code = safeLaunchCode(error)
    if (code === 'NATIVE_STOP_UNCONFIRMED') {
      receiptPublication = {}
      statusSyncInFlight = null
      launched = false
      inputEnabled = false
      stopStatusSync()
      tabs.markUnknown(props.tabId)
    }
    tabs.setDiagnostic(props.tabId, code)
    throw error
  } finally {
    if (stoppingAttempt === attempt) {
      stoppingAttempt = null
      if (attemptIsCurrent(attempt) && launched) startStatusSync()
    }
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
  const preferences = app.terminalPreferences
  term = new Terminal({
    ...terminalAppearanceOptions(preferences),
    scrollback: 10000,
    allowProposedApi: true,
    macOptionIsMeta: true,
  })
  fit = new FitAddon()
  term.loadAddon(fit)
  term.open(container.value)
  if (preferences.renderer === 'webgl') void loadWebgl(term)
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

function schedulePreferenceFit() {
  needsFit.value = true
  if (!props.active || preferenceFitPending || disposed) return
  preferenceFitPending = true
  requestAnimationFrame(() => {
    preferenceFitPending = false
    if (needsFit.value) fitVisible()
  })
}
watch(() => app.terminalPreferences, (next, previous) => {
  if (term && applyTerminalAppearance(term.options, next, previous)) schedulePreferenceFit()
})

/** Renderer is chosen once for each terminal. Failure/context loss falls back
 * without replacing xterm, its theme options, scrollback, binding or selection. */
async function loadWebgl(target: Terminal) {
  try {
    const { WebglAddon } = await import('@xterm/addon-webgl')
    if (disposed || term !== target) return
    const addon = new WebglAddon()
    webglAddon = addon
    addon.onContextLoss(() => {
      if (webglAddon !== addon) return
      webglAddon = null
      try { addon.dispose() } catch { /* DOM fallback keeps the same theme. */ }
    })
    target.loadAddon(addon)
  } catch {
    try { webglAddon?.dispose() } catch { /* Optional renderer cleanup only. */ }
    webglAddon = null
  }
}

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
  try { webglAddon?.dispose() } catch { /* Optional renderer. */ }
  webglAddon = null
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
