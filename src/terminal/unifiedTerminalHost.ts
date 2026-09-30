import type { NativeAttemptIdentity } from '@/stores/nativeTabs'
import type { UnifiedSession } from '@/types/unifiedSession'

/** Only open runtime records, never history rows, own mounted terminals. */
export type OpenTerminalSession = Pick<UnifiedSession, 'id' | 'runtime' | 'adapterSessionId'>
export interface UnifiedTerminalHostPort {
  startLegacy(tabId: string): Promise<void>
  stopLegacy(tabId: string): Promise<void>
  restartLegacy(tabId: string): Promise<void>
  renameLegacy(tabId: string, title: string): Promise<void>
  stopNative(tabId: string, attempt: NativeAttemptIdentity): Promise<void>
  recoverNative(tabId: string, attempt: NativeAttemptIdentity): Promise<void>
  focus(): void
}
