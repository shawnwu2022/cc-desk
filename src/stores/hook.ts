import { defineStore } from 'pinia'
import { onScopeDispose } from 'vue'
import type { HookEventPayload, HookEventDetail } from '@/types/hook'
import type { CliKind } from '@/types/cli'
import { onHookEvent } from '@/api/tauri'
import { onNativeObservation } from '@/api/observer'
import { fromClaudeHook } from '@/integrations/claudeObserver'
import { createObservationRegistry, type ObservationEvent, type ObservationState, type RunRef } from '@/integrations/registry'

export type HookEventType = HookEventDetail['type']
export type HookEventHandler = (payload: HookEventPayload) => void
export type ObservationHandler = (event: ObservationEvent, state: ObservationState) => void
export interface ObservationTarget extends RunRef { cli: CliKind; enabled: boolean }

export const useHookStore = defineStore('hook', () => {
  const subscribers = new Map<HookEventType, Set<HookEventHandler>>()
  const observations = createObservationRegistry()
  const targets = new Map<string, { run: RunRef; handlers: Set<ObservationHandler>; timer: ReturnType<typeof setTimeout> }>()
  const key = (run: RunRef) => JSON.stringify([run.runId, run.generation])
  let disposed = false
  let legacyPending: Promise<void> | null = null
  let nativePending: Promise<void> | null = null
  let legacyStop: (() => void) | undefined
  let nativeStop: (() => void) | undefined

  function subscribe(eventTypes: HookEventType[], handler: HookEventHandler): () => void {
    for (const type of eventTypes) {
      if (!subscribers.has(type)) subscribers.set(type, new Set())
      subscribers.get(type)!.add(handler)
    }
    return () => { for (const type of eventTypes) subscribers.get(type)?.delete(handler) }
  }
  function dispatch(payload: HookEventPayload) {
    if (disposed || !payload || !payload.detail || !payload.ptyId) return
    if (payload.runId !== undefined || payload.observerSource !== undefined) {
      // The backend explicitly tags the legacy lease, not a guessed native run.
      if (!fromClaudeHook(payload) || payload.runId !== payload.ptyId || payload.generation !== 1) return
    }
    for (const handler of [...(subscribers.get(payload.detail.type) ?? [])]) {
      try { handler(payload) } catch { /* optional observer consumers are isolated */ }
    }
  }
  function publish(event: ObservationEvent) {
    const target = targets.get(key(event))
    if (!target || disposed) return
    const reducer = observations.get(target.run)
    if (!reducer) return
    reducer.accept(event)
    for (const handler of [...target.handlers]) {
      // A previous callback may have synchronously detached this binding.
      if (!target.handlers.has(handler) || targets.get(key(event)) !== target) continue
      try { handler(Object.freeze({ ...event }), reducer.state()) } catch { /* never log arbitrary observer errors */ }
    }
  }
  function dispatchNative(payload: HookEventPayload) {
    if (disposed) return
    const event = fromClaudeHook(payload)
    if (!event) return
    const target = targets.get(key(event))
    if (!target) return
    clearTimeout(target.timer)
    publish(event)
  }
  function ensureNativeListener() {
    if (nativeStop || nativePending || disposed) return
    nativePending = onNativeObservation(dispatchNative).then(stop => {
      if (disposed) stop()
      else nativeStop = stop
    }).catch(() => {
      for (const target of targets.values()) publish({ kind: 'timeout', ...target.run })
    }).finally(() => { nativePending = null })
  }
  function subscribeObservation(target: ObservationTarget, handler: ObservationHandler): () => void {
    if (disposed || target.cli !== 'claude' || !target.enabled) return () => {}
    const run = { runId: target.runId, generation: target.generation }
    let entry = targets.get(key(run))
    if (!entry) {
      const reducer = observations.attach(run)
      reducer.accept({ kind: 'connecting', ...run })
      entry = { run, handlers: new Set(), timer: setTimeout(() => publish({ kind: 'timeout', ...run }), 30_000) }
      targets.set(key(run), entry)
    }
    entry.handlers.add(handler)
    ensureNativeListener()
    const owned = entry
    return () => {
      owned.handlers.delete(handler)
      if (owned.handlers.size === 0 && targets.get(key(run)) === owned) {
        clearTimeout(owned.timer)
        observations.detach(run)
        targets.delete(key(run))
      }
    }
  }
  function clearSession(runId: string) {
    for (const [id, entry] of targets) {
      if (entry.run.runId !== runId) continue
      clearTimeout(entry.timer)
      entry.handlers.clear()
      observations.detach(entry.run)
      targets.delete(id)
    }
  }
  function init(): Promise<void> {
    if (legacyStop || disposed) return Promise.resolve()
    if (legacyPending) return legacyPending
    legacyPending = onHookEvent(dispatch).then(stop => {
      if (disposed) stop()
      else legacyStop = stop
    }).catch(() => { /* native process success is independent of monitoring */ })
      .finally(() => { legacyPending = null })
    return legacyPending
  }
  onScopeDispose(() => {
    disposed = true
    legacyStop?.()
    nativeStop?.()
    for (const entry of targets.values()) { clearTimeout(entry.timer); observations.detach(entry.run) }
    targets.clear()
    subscribers.clear()
  })
  return { subscribe, subscribeObservation, clearSession, init, observationFor: (run: RunRef) => observations.get(run)?.state() }
})
