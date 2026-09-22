import type { CliKind, SafeError } from '@/types/cli'

export interface CliAvailability {
  profileId: string
  profileRevision: string
  cli: CliKind
  state: 'available-unverified' | 'unavailable' | 'configuration-required'
  hostStatus: 'not-checked' | 'available' | 'unavailable'
  certified: false
  issue?: SafeError
}

/** Behavior follows its failing API contract tests. */
export async function cliGetAvailability(
  _profileId: string,
  _expectedRevision: string,
): Promise<CliAvailability> {
  throw new Error('AVAILABILITY_NOT_IMPLEMENTED')
}
