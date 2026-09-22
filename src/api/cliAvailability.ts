import { invoke } from '@tauri-apps/api/core'
import type { CliKind, SafeError } from '@/types/cli'

export interface CliAvailability {
  profileId: string
  profileRevision: string
  cli: CliKind
  /** Filesystem/configuration preflight only, never proof of an agent launch. */
  state: 'available-unverified' | 'unavailable' | 'configuration-required'
  hostStatus: 'not-checked' | 'available' | 'unavailable'
  certified: false
  issue?: SafeError
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function canonicalRevision(value: string): boolean {
  return /^(0|[1-9][0-9]{0,19})$/.test(value)
    && BigInt(value) <= BigInt('18446744073709551615')
}

function decodeIssue(value: unknown): SafeError {
  if (!isRecord(value) || typeof value.code !== 'string'
    || !/^[A-Z][A-Z0-9_]{0,63}$/.test(value.code)
    || typeof value.retryable !== 'boolean') {
    throw new Error('INVALID_AVAILABILITY_RESPONSE')
  }
  // Only fixed error codes are used by this view; never copy arbitrary messages/fields.
  return { code: value.code, retryable: value.retryable }
}

/** One request, no global startup checks, automatic retries or executable probing in JS. */
export async function cliGetAvailability(
  profileId: string,
  expectedRevision: string,
): Promise<CliAvailability> {
  if (typeof profileId !== 'string' || !/^[A-Za-z0-9_-]{1,128}$/.test(profileId)
    || typeof expectedRevision !== 'string' || !canonicalRevision(expectedRevision)) {
    throw new Error('INVALID_AVAILABILITY_REQUEST')
  }
  const value = await invoke<unknown>('cli_get_availability', {
    request: { profileId, expectedRevision },
  })
  if (!isRecord(value) || value.profileId !== profileId
    || value.profileRevision !== expectedRevision || value.certified !== false
    || (value.cli !== 'claude' && value.cli !== 'codex' && value.cli !== 'shell')
    || (value.state !== 'available-unverified' && value.state !== 'unavailable'
      && value.state !== 'configuration-required')
    || (value.hostStatus !== 'not-checked' && value.hostStatus !== 'available'
      && value.hostStatus !== 'unavailable')) {
    throw new Error('INVALID_AVAILABILITY_RESPONSE')
  }
  const issue = value.issue === undefined ? undefined : decodeIssue(value.issue)
  if ((value.state === 'available-unverified' && issue !== undefined)
    || (value.state !== 'available-unverified' && issue === undefined)
    || (value.state === 'configuration-required' && issue?.code !== 'PROGRAM_TRUST_REQUIRED')) {
    throw new Error('INVALID_AVAILABILITY_RESPONSE')
  }
  return {
    profileId,
    profileRevision: expectedRevision,
    cli: value.cli,
    state: value.state,
    hostStatus: value.hostStatus,
    certified: false,
    ...(issue === undefined ? {} : { issue }),
  }
}
