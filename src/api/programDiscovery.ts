import type { ProfileLauncher } from '@/types/profile'
import { invoke } from '@tauri-apps/api/core'
import { parseU64 } from '@/utils/nativeIdentity'

export interface ProgramCandidate { programPath: string; launcher: ProfileLauncher }
export interface ProgramDiscovery {
  profileId: string; profileRevision: string; workspaceRevision: string; projectId: string
  cli: 'claude' | 'codex'; candidates: ProgramCandidate[]
}

function invalid(): never { throw new Error('DISCOVERY_UNAVAILABLE') }
function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return invalid()
  return value as Record<string, unknown>
}
function revision(value: unknown): string {
  if (typeof value !== 'string' || value.length > 20) return invalid()
  try { parseU64(value) } catch { return invalid() }
  return value
}
function path(value: unknown): string {
  if (typeof value !== 'string' || value.length > 32768 || /\p{Cc}/u.test(value) || !/^(?:\/|[A-Za-z]:[\\/]|\\\\)/.test(value)) return invalid()
  return value
}
export async function cliDiscoverPrograms(profileId: string, expectedRevision: string, projectId: string): Promise<ProgramDiscovery> {
  if (![profileId, projectId].every(value => /^[A-Za-z0-9_-]{1,128}$/.test(value))) return invalid()
  revision(expectedRevision)
  const value = record(await invoke('cli_discover_programs', { request: { profileId, expectedRevision, projectId } }))
  if (value.profileId !== profileId || value.profileRevision !== expectedRevision || value.projectId !== projectId
    || (value.cli !== 'claude' && value.cli !== 'codex') || !Array.isArray(value.candidates) || value.candidates.length > 32) return invalid()
  const candidates = value.candidates.map(item => {
    const candidate = record(item), launcher = record(candidate.launcher)
    let selected: ProfileLauncher
    if (launcher.kind === 'native') selected = { kind: 'native' }
    else if (launcher.kind === 'shim' && launcher.dialect === 'cmd') selected = { kind: 'shim', runner: path(launcher.runner), dialect: 'cmd' }
    else return invalid()
    return { programPath: path(candidate.programPath), launcher: selected }
  })
  return { profileId, profileRevision: expectedRevision, projectId, workspaceRevision: revision(value.workspaceRevision), cli: value.cli, candidates }
}
