/**
 * SessionStart hook monitoring state machine.
 * PTY process startup is authoritative; hooks only enrich activity status.
 */

export type WaiterStatus =
  | 'waiting'
  | 'started'
  | 'unavailable'
  | 'exited'
  | 'failed'
  | 'cancelled'

export type WaiterEvent =
  | { type: 'sessionStart' }
  | { type: 'timeout' }
  | { type: 'ptyExit' }
  | { type: 'spawnFail' }
  | { type: 'unmount' }

export function reduceWaiter(state: WaiterStatus, event: WaiterEvent): WaiterStatus {
  if (state !== 'waiting') return state

  switch (event.type) {
    case 'sessionStart':
      return 'started'
    case 'timeout':
      return 'unavailable'
    case 'ptyExit':
      return 'exited'
    case 'spawnFail':
      return 'failed'
    case 'unmount':
      return 'cancelled'
  }
}

export function isStartupFailure(status: WaiterStatus): boolean {
  return status === 'exited' || status === 'failed' || status === 'cancelled'
}

export const PERSIST_FAILED_CODE = 'persist_failed' as const

export function isPersistFailedError(error: unknown): boolean {
  return (
    error !== null &&
    typeof error === 'object' &&
    'code' in error &&
    (error as { code: unknown }).code === PERSIST_FAILED_CODE
  )
}
