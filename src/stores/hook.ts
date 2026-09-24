import { defineStore } from 'pinia'
import type { HookEventPayload, HookEventDetail } from '@/types/hook'
import { onHookEvent } from '@/api/tauri'
import { fromClaudeHook } from '@/integrations/claudeObserver'
import type { ObservationEvent } from '@/integrations/registry'

export type HookEventType = HookEventDetail['type']
export type HookEventHandler = (payload: HookEventPayload) => void
export type ObservationHandler = (event: ObservationEvent) => void

export const useHookStore = defineStore('hook', () => {
  let initialized = false
  const subscribers = new Map<HookEventType, Set<HookEventHandler>>()
  const observationSubscribers = new Set<ObservationHandler>()

  function subscribe(eventTypes: HookEventType[], handler: HookEventHandler): () => void {
    for (const type of eventTypes) {
      if (!subscribers.has(type)) {
        subscribers.set(type, new Set())
      }
      subscribers.get(type)!.add(handler)
    }
    return () => {
      for (const type of eventTypes) {
        subscribers.get(type)?.delete(handler)
      }
    }
  }

  function subscribeObservation(handler: ObservationHandler): () => void {
    observationSubscribers.add(handler)
    return () => observationSubscribers.delete(handler)
  }

  function dispatch(payload: HookEventPayload) {
    const hasObserverMetadata =
      payload.runId !== undefined ||
      payload.generation !== undefined ||
      payload.eventId !== undefined ||
      payload.observerSource !== undefined

    if (hasObserverMetadata) {
      const observation = fromClaudeHook(payload)
      if (!observation) return
      for (const handler of observationSubscribers) {
        handler(observation)
      }
      return
    }

    if (!payload.ptyId) return
    const handlers = subscribers.get(payload.detail.type)
    if (handlers) {
      for (const handler of handlers) {
        handler(payload)
      }
    }
  }

  function clearSession(_key: string) {
    // placeholder，保持 XTermTerminal 调用兼容
  }

  async function init() {
    if (initialized) return
    initialized = true
    onHookEvent(dispatch)
  }

  return {
    subscribe,
    subscribeObservation,
    clearSession,
    init,
  }
})
