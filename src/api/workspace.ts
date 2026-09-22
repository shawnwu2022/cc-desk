import { invoke } from '@tauri-apps/api/core'
import type { ProjectChanges, ProjectList } from '@/types/workspace'

export const listRegisteredProjects = (): Promise<ProjectList> =>
  invoke('cli_list_projects')

export const registerProject = (selectedPath: string): Promise<ProjectList> =>
  invoke('cli_register_project', { selectedPath })

export const patchProject = (
  projectId: string, expectedRevision: string, changes: ProjectChanges,
): Promise<ProjectList> =>
  invoke('cli_patch_project', { projectId, expectedRevision, changes })

/** Removes Desk's registration only. There is no filesystem/transcript delete in this API. */
export const removeProject = (projectId: string, expectedRevision: string): Promise<ProjectList> =>
  invoke('cli_remove_project', { projectId, expectedRevision })
