import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { HookEventPayload } from '@/types/hook'

export type ObserverReasonCode = 'OBSERVER_DISABLED' | 'OBSERVER_UNSUPPORTED' | 'OBSERVER_CONNECTING'
  | 'OBSERVER_UNAVAILABLE' | 'OBSERVER_SERVICE_UNAVAILABLE' | 'OBSERVER_PLUGIN_UNAVAILABLE' | 'OBSERVER_PREPARE_FAILED'
export interface ObserverInstallation {
  requestId: string
  run: { runId: string; generation: number }
  state: 'off' | 'connecting' | 'unavailable'
  reasonCode: ObserverReasonCode
}
export async function readObserverInstallation(bridge: { invoke(command: string, payload: unknown): Promise<unknown> }, owner: { requestId: string; runId: string; generation: number }): Promise<ObserverInstallation> {
  const value = await bridge.invoke('cli_get_observer_status', owner)
  try {
    const result = value as ObserverInstallation
    const reasons = { off: ['OBSERVER_DISABLED', 'OBSERVER_UNSUPPORTED'], connecting: ['OBSERVER_CONNECTING'], unavailable: ['OBSERVER_UNAVAILABLE', 'OBSERVER_SERVICE_UNAVAILABLE', 'OBSERVER_PLUGIN_UNAVAILABLE', 'OBSERVER_PREPARE_FAILED'] }
    if (!result || typeof result !== 'object' || Object.keys(result).sort().join() !== 'reasonCode,requestId,run,state'
      || !result.run || Object.keys(result.run).sort().join() !== 'generation,runId'
      || result.requestId !== owner.requestId || result.run.runId !== owner.runId || result.run.generation !== owner.generation
      || !reasons[result.state]?.includes(result.reasonCode)) throw new Error()
    return { requestId: owner.requestId, run: { runId: owner.runId, generation: owner.generation }, state: result.state, reasonCode: result.reasonCode }
  } catch { throw new Error('INVALID_OBSERVER_STATUS') }
}

/** Owner-targeted, low-rate observation metadata. Not a terminal control channel. */
export function onNativeObservation(handler: (payload: HookEventPayload) => void): Promise<UnlistenFn> {
  return listen<HookEventPayload>('native-observation', event => handler(event.payload))
}
