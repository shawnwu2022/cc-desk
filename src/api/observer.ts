import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { HookEventPayload } from '@/types/hook'

/** Owner-targeted, low-rate observation metadata. Not a terminal control channel. */
export function onNativeObservation(handler: (payload: HookEventPayload) => void): Promise<UnlistenFn> {
  return listen<HookEventPayload>('native-observation', event => handler(event.payload))
}
