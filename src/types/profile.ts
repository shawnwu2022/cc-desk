import type { CliKind } from './cli'

/** Set(false), Set('') and Unset are deliberately distinct from inheritance. */
export type ProfileOverride<T> =
  | { mode: 'inherit' }
  | { mode: 'set'; value: T }
  | { mode: 'unset' }

export type ProfileEnvValue =
  | { kind: 'literal'; value: string; nonSecret: true }
  | { kind: 'host-ref'; name: string }

export type ShellDialect = 'bash' | 'power-shell' | 'cmd'
export type ProfileLauncher =
  | { kind: 'native' }
  | { kind: 'shell'; program: string; dialect: ShellDialect }
  | { kind: 'shim'; runner: string; dialect: ShellDialect }

/** Stored Desk preferences, never the resolved native environment or credentials. */
export interface CliProfile {
  id: string
  revision: string
  cli: CliKind
  name: string
  launcher: ProfileLauncher
  programPath: ProfileOverride<string>
  defaultArgs: ProfileOverride<string[]>
  skipPermissions: ProfileOverride<boolean>
  observer: ProfileOverride<boolean>
  env: Record<string, ProfileOverride<ProfileEnvValue>>
}

export type ProfileChanges = Partial<Pick<CliProfile,
  'name' | 'launcher' | 'programPath' | 'defaultArgs' | 'skipPermissions' | 'observer' | 'env'
>>

export type ProfilePatch =
  | { op: 'create'; profile: CliProfile }
  | { op: 'update'; id: string; changes: ProfileChanges }
  | { op: 'delete'; id: string }

export interface ProfileList {
  /** Workspace CAS revision. A profile's own revision is not interchangeable with this. */
  revision: string
  profiles: CliProfile[]
}
