import {
  cliAckOutput,
  cliWriteInput,
  cliWriteProtocol,
} from '@/api/tauri'
import {
  createNativeTerminalBinding,
  type NativeTerminalBinding,
  type NativeTerminalLike,
} from './nativeTerminalBinding'
import type { InputTarget } from './inputQueue'

export interface DeskNativeTerminalBindingOptions {
  term: NativeTerminalLike
  runId: string
  generation: number
  currentTarget: () => InputTarget
  onDegraded?: (reason: string) => void
}

/**
 * Production D19 composition root. All native input/output control traffic goes
 * through the authenticated document bridge APIs defined in api/tauri.ts.
 *
 * UI/store ownership is intentionally left to D22; D19 only wires one already
 * identified native run to its terminal host.
 */
export function createDeskNativeTerminalBinding(
  options: DeskNativeTerminalBindingOptions,
): NativeTerminalBinding {
  return createNativeTerminalBinding({
    term: options.term,
    runId: options.runId,
    generation: options.generation,
    currentTarget: options.currentTarget,
    writeUser: cliWriteInput,
    writeProtocol: cliWriteProtocol,
    ackOutput: cliAckOutput,
    onDegraded: options.onDegraded,
  })
}
