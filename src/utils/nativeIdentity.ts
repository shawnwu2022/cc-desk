import type {
  CliKind,
  LaunchAction,
  LaunchRequest,
  NativeSessionRef,
  ResumeScope,
} from '@/types/cli'

const MAX_U64 = 18_446_744_073_709_551_615n
const MAX_U32 = 4_294_967_295
const MAX_U16 = 65_535
const U64_PATTERN = /^(?:0|[1-9]\d*)$/

function invalid(field: string): never {
  throw new Error(`INVALID_REQUEST:${field}`)
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function requireRecord(value: unknown, field: string): Record<string, unknown> {
  if (!isRecord(value)) invalid(field)
  return value
}

function requireString(value: unknown, field: string, allowEmpty = false): string {
  if (typeof value !== 'string') invalid(field)
  if (!allowEmpty && value.length === 0) invalid(field)
  if (value.includes('\0')) invalid(field)
  return value
}

function requireStringArray(value: unknown, field: string): string[] {
  if (!Array.isArray(value)) invalid(field)
  return value.map((item, index) => requireString(item, `${field}[${index}]`, true))
}

function requireInteger(
  value: unknown,
  field: string,
  minimum: number,
  maximum: number,
): number {
  if (
    typeof value !== 'number'
    || !Number.isInteger(value)
    || value < minimum
    || value > maximum
  ) {
    invalid(field)
  }
  return value
}

function validateCliKind(value: unknown): CliKind {
  if (value === 'claude' || value === 'codex' || value === 'shell') return value
  return invalid('cli')
}

function validateResumeScope(value: unknown): ResumeScope {
  if (value === 'current-project' || value === 'all') return value
  return invalid('action.scope')
}

function validateLaunchAction(value: unknown): LaunchAction {
  const action = requireRecord(value, 'action')
  const kind = action.kind

  switch (kind) {
    case 'new':
      return { kind }
    case 'resume-picker':
      return { kind, scope: validateResumeScope(action.scope) }
    case 'resume-id':
      return {
        kind,
        nativeSessionId: requireString(action.nativeSessionId, 'action.nativeSessionId'),
      }
    case 'raw':
      return { kind, argv: requireStringArray(action.argv, 'action.argv') }
    default:
      return invalid('action.kind')
  }
}

export function parseU64(value: unknown): bigint {
  if (typeof value !== 'string' || !U64_PATTERN.test(value)) invalid('u64')
  const parsed = BigInt(value)
  if (parsed > MAX_U64) invalid('u64')
  return parsed
}

export function validateLaunchRequest(value: unknown): LaunchRequest {
  const request = requireRecord(value, 'request')
  const expectedProfileRevision = requireString(
    request.expectedProfileRevision,
    'expectedProfileRevision',
  )
  parseU64(expectedProfileRevision)

  const action = validateLaunchAction(request.action)
  const extraArgs = requireStringArray(request.extraArgs, 'extraArgs')
  if (action.kind === 'raw' && extraArgs.length !== 0) invalid('extraArgs')

  return {
    requestId: requireString(request.requestId, 'requestId'),
    tabId: requireString(request.tabId, 'tabId'),
    runId: requireString(request.runId, 'runId'),
    generation: requireInteger(request.generation, 'generation', 0, MAX_U32),
    profileId: requireString(request.profileId, 'profileId'),
    expectedProfileRevision,
    cli: validateCliKind(request.cli),
    launchCwd: requireString(request.launchCwd, 'launchCwd'),
    action,
    extraArgs,
    cols: requireInteger(request.cols, 'cols', 1, MAX_U16),
    rows: requireInteger(request.rows, 'rows', 1, MAX_U16),
  }
}

export function nativeSessionKey(reference: NativeSessionRef): string {
  return JSON.stringify([
    reference.hostId,
    reference.cli,
    reference.sourceRootKey,
    reference.nativeSessionId,
  ])
}

export function validateWireBytes(value: unknown): Uint8Array {
  if (!Array.isArray(value)) invalid('bytes')
  const bytes = new Uint8Array(value.length)
  for (let index = 0; index < value.length; index += 1) {
    const byte = value[index]
    if (typeof byte !== 'number' || !Number.isInteger(byte) || byte < 0 || byte > 255) {
      invalid(`bytes[${index}]`)
    }
    bytes[index] = byte
  }
  return bytes
}
