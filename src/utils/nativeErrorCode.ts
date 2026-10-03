const SAFE_NATIVE_ERROR_CODES = new Set([
  'BACKEND_INSTANCE_CHANGED',
  'CLI_PROFILE_REQUIRED',
  'DOCUMENT_BRIDGE_UNAVAILABLE',
  'FORBIDDEN',
  'GENERATION_EXHAUSTED',
  'INVALID_PROTOCOL_INPUT_SIZE',
  'INVALID_REQUEST',
  'INVALID_TERMINAL_SIZE',
  'LAUNCH_ATTEMPT_NOT_FOUND',
  'LAUNCH_ATTEMPT_NOT_READY',
  'LAUNCH_CANCEL_UNAVAILABLE',
  'LAUNCH_REQUEST_ID_CONFLICT',
  'LAUNCH_STATE_UNKNOWN',
  'NATIVE_INPUT_PAUSED',
  'NATIVE_OUTPUT_DEGRADED',
  'NATIVE_RUN_NOT_WRITABLE',
  'NATIVE_RUNTIME_NOT_READY',
  'NATIVE_STOP_UNCONFIRMED',
  'NATIVE_TERMINAL_DISPOSED',
  'NATIVE_TERMINAL_NOT_READY',
  'OUTPUT_ACK_FAILED',
  'OUTPUT_FRAME_IDENTITY',
  'OUTPUT_OFFSET_EXHAUSTED',
  'OUTPUT_OFFSET_GAP',
  'OUTPUT_WRITE_FAILED',
  'PROFILE_CLI_MISMATCH',
  'PROFILE_MISMATCH',
  'PROFILE_NOT_FOUND',
  'PROGRAM_TRUST_REQUIRED',
  'PROGRAM_UNAVAILABLE',
  'REVISION_CONFLICT',
  'ROUTE_UNAVAILABLE',
  'RUN_HANDOFF_FAILED',
  'RUN_SUPERVISOR_STOPPING',
  'STALE_OUTPUT_STREAM',
  'UNSUPPORTED_ACTION',
  'WORKING_DIRECTORY_UNAVAILABLE',
  'XTERM_USER_INPUT_PROVENANCE_UNAVAILABLE',
])

function candidate(value: unknown): string | null {
  if (value instanceof Error) return value.message
  if (value && typeof value === 'object' && 'code' in value) {
    const code = (value as { code?: unknown }).code
    return typeof code === 'string' ? code : null
  }
  return null
}

/**
 * Convert a native failure into a fixed public code. Arbitrary backend/OS/user
 * text is never reflected into the DOM or diagnostic surfaces.
 */
export function publicNativeErrorCode(
  value: unknown,
  fallback = 'NATIVE_OPERATION_FAILED',
): string {
  const code = candidate(value)
  return code && SAFE_NATIVE_ERROR_CODES.has(code) ? code : fallback
}
