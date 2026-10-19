import type { Terminal } from '@xterm/xterm'
import type { NativeLaunchEntryInput } from '@/terminal/nativeLaunchEntry'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'

export * from '../../../src/visual/tauriStub'
export class Channel<T> { onmessage: ((frame: T) => void) | null = null }
export let terminal: Terminal
export const sizes: { cols: number; rows: number }[] = []
export let launches = 0
export async function cliResize(_owner: unknown, cols: number, rows: number) { sizes.push({ cols, rows }) }
export class NativeLaunchNotSubmittedError extends Error {}
export function createNativeLaunchEntry() {
  let receipt: LaunchStatus
  return {
    async start(input: NativeLaunchEntryInput) {
      ++launches
      sizes.push({ cols: input.cols, rows: input.rows })
      receipt = { requestId: input.requestId, run: { runId: input.runId, generation: input.generation }, instanceId: 'fixture-instance', phase: 'running', revision: '1', failure: null }
      return receipt
    },
    async recover() { return receipt }, latest() { return receipt },
  }
}
export function createDeskNativeTerminalBinding(options: { term: Terminal }) {
  terminal = options.term
  return { dispose() {} }
}
