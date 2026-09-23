import type { LaunchRequest, U64String } from '@/types/cli'

export type LaunchPhase =
  | 'reserved' | 'starting' | 'running' | 'failed'
  | 'cancelled' | 'indeterminate' | 'exited'

export interface LaunchStatus {
  instanceId: string
  requestId: string
  run: { runId: string; generation: number }
  revision: U64String
  phase: LaunchPhase
  failure: 'route-unavailable' | 'process-start-failed' | 'aborted' | 'outcome-unknown' | null
}

// This is a transport boundary, not authorization. Bind it to authenticated IPC
// only after D11's actual WebView document-lifetime integration is verified.
export interface LaunchAttemptTransport {
  start(request: LaunchRequest): Promise<unknown>
  status(requestId: string): Promise<unknown>
}

export interface LaunchAttempt {
  start(): Promise<LaunchStatus>
  recover(): Promise<LaunchStatus>
  latest(): LaunchStatus | undefined
}

export function createLaunchAttempt(
  _request: LaunchRequest,
  _instanceId: string,
  _transport: LaunchAttemptTransport,
): LaunchAttempt {
  return {
    start: () => Promise.reject(new Error('LAUNCH_ATTEMPT_NOT_IMPLEMENTED')),
    recover: () => Promise.reject(new Error('LAUNCH_ATTEMPT_NOT_IMPLEMENTED')),
    latest: () => undefined,
  }
}
