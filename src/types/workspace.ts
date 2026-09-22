import type { ProfileOverride } from './profile'

export interface RegisteredProject {
  projectId: string
  hostId: string
  sourcePathKey: string
  selectedPath: string
  canonicalPath: string | null
  alias: ProfileOverride<string>
  pinned: ProfileOverride<boolean>
  hidden: ProfileOverride<boolean>
}

export type ProjectChanges = Partial<Pick<RegisteredProject, 'alias' | 'pinned' | 'hidden'>>

export interface ProjectMetadata {
  alias: string | null
  pinned: boolean | null
  hidden: boolean | null
}

export interface ProjectList {
  revision: string
  projects: RegisteredProject[]
  metadata?: Record<string, ProjectMetadata>
  warnings?: string[]
  projectId?: string
}
