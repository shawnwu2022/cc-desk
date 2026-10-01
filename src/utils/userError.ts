export type UserErrorContext =
  | 'workspace'
  | 'launch'
  | 'resource'
  | 'settings'
  | 'session'
  | 'update'

export interface UserErrorPresentation {
  context: UserErrorContext
  messageKey: string
  actionKey: string | null
  severity: 'info' | 'warning' | 'error'
  retryable: boolean
  detailCode: string
}

type ErrorTemplate = Omit<UserErrorPresentation, 'context'>

const KNOWN_ERRORS: Readonly<Record<string, ErrorTemplate>> = Object.freeze({
  RECOVERY_UNAVAILABLE: { messageKey: 'errorRecoveryUnavailable', actionKey: 'refresh', severity: 'warning', retryable: false, detailCode: 'RECOVERY_UNAVAILABLE' },
  STALE_SESSION_ATTEMPT: { messageKey: 'errorSessionChanged', actionKey: 'refresh', severity: 'warning', retryable: false, detailCode: 'STALE_SESSION_ATTEMPT' },
  ACTION_CANCELLED: { messageKey: 'errorActionCancelled', actionKey: null, severity: 'info', retryable: false, detailCode: 'ACTION_CANCELLED' },
  NATIVE_STOP_UNCONFIRMED: { messageKey: 'errorStopUnconfirmed', actionKey: 'confirmStatus', severity: 'warning', retryable: false, detailCode: 'NATIVE_STOP_UNCONFIRMED' },
  PROFILE_IN_USE: { messageKey: 'errorConfigurationInUse', actionKey: null, severity: 'warning', retryable: false, detailCode: 'PROFILE_IN_USE' },
  PROFILE_SELECTION_CHANGED: { messageKey: 'resumeConfigurationChanged', actionKey: 'refresh', severity: 'warning', retryable: false, detailCode: 'PROFILE_SELECTION_CHANGED' },
  PROJECT_IDENTITY_CHANGED: { messageKey: 'resumeConfigurationChanged', actionKey: 'refresh', severity: 'warning', retryable: false, detailCode: 'PROJECT_IDENTITY_CHANGED' },
  PROJECT_HAS_OPEN_SESSIONS: { messageKey: 'projectRemoveOpenSessions', actionKey: null, severity: 'warning', retryable: false, detailCode: 'PROJECT_HAS_OPEN_SESSIONS' },
  SESSION_ORIGIN_AMBIGUOUS: { messageKey: 'resumeAmbiguous', actionKey: 'refresh', severity: 'warning', retryable: false, detailCode: 'SESSION_ORIGIN_AMBIGUOUS' },
  SESSION_NOT_RESUMABLE: { messageKey: 'errorSessionNotResumable', actionKey: null, severity: 'warning', retryable: false, detailCode: 'SESSION_NOT_RESUMABLE' },
  REVISION_CONFLICT: {
    messageKey: 'errorRevisionConflict',
    actionKey: 'retry',
    severity: 'warning',
    retryable: true,
    detailCode: 'REVISION_CONFLICT',
  },
  CLI_NOT_FOUND: {
    messageKey: 'errorCliNotFound',
    actionKey: 'viewHelp',
    severity: 'warning',
    retryable: true,
    detailCode: 'CLI_NOT_FOUND',
  },
  AUTH_REQUIRED: {
    messageKey: 'errorAuthRequired',
    actionKey: 'viewHelp',
    severity: 'warning',
    retryable: true,
    detailCode: 'AUTH_REQUIRED',
  },
  LAUNCH_STATE_UNKNOWN: {
    messageKey: 'errorLaunchStateUnknown',
    actionKey: 'confirmStatus',
    severity: 'info',
    retryable: false,
    detailCode: 'LAUNCH_STATE_UNKNOWN',
  },
  SESSION_NOT_FOUND: {
    messageKey: 'errorSessionNotFound',
    actionKey: 'removeHistoryRecord',
    severity: 'warning',
    retryable: false,
    detailCode: 'SESSION_NOT_FOUND',
  },
  INVALID_RAW_ARGV_JSON: {
    messageKey: 'errorInvalidLaunchArguments',
    actionKey: 'editLaunchArguments',
    severity: 'warning',
    retryable: true,
    detailCode: 'INVALID_RAW_ARGV_JSON',
  },
  RESOURCE_UNAVAILABLE: {
    messageKey: 'errorResourceUnavailable',
    actionKey: 'retry',
    severity: 'warning',
    retryable: true,
    detailCode: 'RESOURCE_UNAVAILABLE',
  },
  SOURCE_UNAVAILABLE: {
    messageKey: 'errorResourceUnavailable',
    actionKey: 'retry',
    severity: 'warning',
    retryable: true,
    detailCode: 'SOURCE_UNAVAILABLE',
  },
})

const GENERIC: ErrorTemplate = Object.freeze({
  messageKey: 'errorGenericUnavailable',
  actionKey: 'viewDetails',
  severity: 'error',
  retryable: false,
  detailCode: 'GENERIC_UNAVAILABLE',
})

export function mapSafeUserError(code: string, context: UserErrorContext): UserErrorPresentation {
  const known = Object.prototype.hasOwnProperty.call(KNOWN_ERRORS, code) ? KNOWN_ERRORS[code] : GENERIC
  return { context, ...known }
}

/** Never store or display raw exceptions, even when their payload has a code field. */
export function safeUserErrorCode(value: unknown): string {
  const candidate = value instanceof Error ? value.message
    : value && typeof value === 'object' && Object.prototype.hasOwnProperty.call(value, 'code') ? (value as { code: unknown }).code : null
  return typeof candidate === 'string' && Object.prototype.hasOwnProperty.call(KNOWN_ERRORS, candidate) ? candidate : 'GENERIC_UNAVAILABLE'
}
