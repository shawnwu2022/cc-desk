import type { UnifiedSession } from '@/types/unifiedSession'
import { mapSafeUserError } from './userError'

export interface SessionDiagnostics {
  cli: 'claude' | 'codex'
  runtime: 'native-cli' | 'legacy-claude'
  stateKey: string
  open: boolean
  preparing: boolean
  generation: number | null
  errorCode: string | null
}
const stateKeys = {
  starting: 'sessionStatusStarting', running: 'sessionStatusRunning', unknown: 'sessionStatusConfirming',
  stopped: 'sessionStatusEnded', failed: 'sessionStatusFailed',
} as const

/** Deliberate allowlist: no title, IDs, paths, argv, configuration or raw errors. */
export function projectSessionDiagnostics(row: UnifiedSession, open: boolean, generation?: number, preparing = false): SessionDiagnostics {
  return {
    cli: row.cli === 'codex' ? 'codex' : 'claude',
    runtime: row.runtime === 'native-cli' ? 'native-cli' : 'legacy-claude',
    stateKey: row.attentionState === 'needs-user' ? 'sessionStatusNeedsUser'
      : Object.prototype.hasOwnProperty.call(stateKeys, row.processState) ? stateKeys[row.processState] : 'sessionStatusConfirming',
    open,
    preparing,
    generation: Number.isSafeInteger(generation) && generation! > 0 && generation! <= 0xffffffff ? generation! : null,
    errorCode: row.safeErrorCode ? mapSafeUserError(row.safeErrorCode === 'LAUNCH_CONFIGURATION_REQUIRED'
      ? row.preparationIssueCode ?? row.safeErrorCode : row.safeErrorCode, 'session').detailCode : null,
  }
}
