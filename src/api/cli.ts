import { invoke } from '@tauri-apps/api/core'
import type { ProfileList, ProfilePatch } from '@/types/profile'

export { cliGetAvailability } from './cliAvailability'
export type { CliAvailability } from './cliAvailability'

/** Reads only Desk preferences; the backend does not project legacy credentials. */
export function cliListProfiles(): Promise<ProfileList> {
  return invoke<ProfileList>('cli_list_profiles')
}

/** Conflict or uncertain commit requires a fresh read and explicit user reconciliation. */
export function cliPatchProfile(expectedRevision: string, patch: ProfilePatch): Promise<ProfileList> {
  return invoke<ProfileList>('cli_patch_profile', { expectedRevision, patch })
}
