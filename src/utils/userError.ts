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
  FORBIDDEN: { messageKey: 'errorGenericUnavailable', actionKey: 'viewDetails', severity: 'warning', retryable: false, detailCode: 'FORBIDDEN' },
  DOCUMENT_BRIDGE_UNAVAILABLE: { messageKey: 'errorGenericUnavailable', actionKey: 'viewDetails', severity: 'warning', retryable: false, detailCode: 'DOCUMENT_BRIDGE_UNAVAILABLE' },
  INVALID_REQUEST: { messageKey: 'errorGenericUnavailable', actionKey: 'viewDetails', severity: 'warning', retryable: false, detailCode: 'INVALID_REQUEST' },
  LAUNCH_CANCEL_UNAVAILABLE: { messageKey: 'errorStopUnconfirmed', actionKey: 'confirmStatus', severity: 'warning', retryable: false, detailCode: 'LAUNCH_CANCEL_UNAVAILABLE' },
  LAUNCH_CONFIGURATION_REQUIRED: { messageKey: 'launchPreparationConfigurationRequired', actionKey: 'launchConfigEditAction', severity: 'warning', retryable: false, detailCode: 'LAUNCH_CONFIGURATION_REQUIRED' },
  PROGRAM_TRUST_REQUIRED: { messageKey: 'launchPreparationConfigurationRequired', actionKey: 'launchConfigEditAction', severity: 'warning', retryable: false, detailCode: 'PROGRAM_TRUST_REQUIRED' },
  PROGRAM_UNAVAILABLE: { messageKey: 'errorCliNotFound', actionKey: 'launchConfigEditAction', severity: 'warning', retryable: false, detailCode: 'PROGRAM_UNAVAILABLE' },
  RUNNER_UNAVAILABLE: { messageKey: 'launchPreparationConfigurationUnavailable', actionKey: 'launchConfigEditAction', severity: 'warning', retryable: false, detailCode: 'RUNNER_UNAVAILABLE' },
  ENV_SOURCE_MISSING: { messageKey: 'launchPreparationConfigurationUnavailable', actionKey: 'launchConfigEditAction', severity: 'warning', retryable: false, detailCode: 'ENV_SOURCE_MISSING' },
  LEGACY_READ_FAILED: { messageKey: 'launchPreparationConfigurationUnavailable', actionKey: 'launchConfigEditAction', severity: 'warning', retryable: false, detailCode: 'LEGACY_READ_FAILED' },
  LEGACY_TOO_LARGE: { messageKey: 'launchPreparationConfigurationUnavailable', actionKey: 'launchConfigEditAction', severity: 'warning', retryable: false, detailCode: 'LEGACY_TOO_LARGE' },
  LEGACY_INVALID: { messageKey: 'launchPreparationConfigurationUnavailable', actionKey: 'launchConfigEditAction', severity: 'warning', retryable: false, detailCode: 'LEGACY_INVALID' },
  WORKING_DIRECTORY_UNAVAILABLE: { messageKey: 'launchPreparationWorkingDirectoryUnavailable', actionKey: null, severity: 'warning', retryable: false, detailCode: 'WORKING_DIRECTORY_UNAVAILABLE' },
  PROFILE_NOT_FOUND: { messageKey: 'resumeConfigurationChanged', actionKey: 'refresh', severity: 'warning', retryable: false, detailCode: 'PROFILE_NOT_FOUND' },
  PROFILE_CLI_MISMATCH: { messageKey: 'resumeConfigurationChanged', actionKey: 'refresh', severity: 'warning', retryable: false, detailCode: 'PROFILE_CLI_MISMATCH' },
  PROFILE_MISMATCH: { messageKey: 'resumeConfigurationChanged', actionKey: 'refresh', severity: 'warning', retryable: false, detailCode: 'PROFILE_MISMATCH' },
  NATIVE_RUNTIME_NOT_READY: { messageKey: 'errorGenericUnavailable', actionKey: 'viewDetails', severity: 'warning', retryable: false, detailCode: 'NATIVE_RUNTIME_NOT_READY' },
  RUN_SUPERVISOR_STOPPING: { messageKey: 'errorGenericUnavailable', actionKey: 'viewDetails', severity: 'warning', retryable: false, detailCode: 'RUN_SUPERVISOR_STOPPING' },
  RECOVERY_UNAVAILABLE: { messageKey: 'errorRecoveryUnavailable', actionKey: 'refresh', severity: 'warning', retryable: false, detailCode: 'RECOVERY_UNAVAILABLE' },
  STALE_SESSION_ATTEMPT: { messageKey: 'errorSessionChanged', actionKey: 'refresh', severity: 'warning', retryable: false, detailCode: 'STALE_SESSION_ATTEMPT' },
  ACTION_CANCELLED: { messageKey: 'errorActionCancelled', actionKey: null, severity: 'info', retryable: false, detailCode: 'ACTION_CANCELLED' },
  NATIVE_INPUT_PAUSED: { messageKey: 'errorNativeInputPaused', actionKey: null, severity: 'warning', retryable: false, detailCode: 'NATIVE_INPUT_PAUSED' },
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
