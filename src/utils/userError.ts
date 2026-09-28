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
  const known = KNOWN_ERRORS[code] ?? GENERIC
  return { context, ...known }
}
