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
  isUserInputAllowed?: () => boolean
  onDegraded?: (reason: string) => void
  onActivity?: () => void
}

/**
 * Production D19 composition root. All native input/output control traffic goes
 * through the authenticated document bridge APIs defined in api/tauri.ts.
 */
export function createDeskNativeTerminalBinding(
  options: DeskNativeTerminalBindingOptions,
): NativeTerminalBinding {
  return createNativeTerminalBinding({
    term: options.term,
    runId: options.runId,
    generation: options.generation,
    currentTarget: options.currentTarget,
    isUserInputAllowed: options.isUserInputAllowed,
    writeUser: cliWriteInput,
    writeProtocol: cliWriteProtocol,
    ackOutput: cliAckOutput,
    onDegraded: options.onDegraded,
    onActivity: options.onActivity,
  })
}
