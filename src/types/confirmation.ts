/** Display requests contain human labels only. Execution ownership stays with the store. */
export type SessionConfirmationRequest =
  | { kind: 'close-running'; sessionId: string; title: string }
  | { kind: 'stop-and-archive'; sessionId: string; title: string }
  | { kind: 'restart-unknown'; sessionId: string; title: string }

export type ProjectConfirmationRequest =
  | { kind: 'remove-project'; title: string }
  | { kind: 'delete-launch-configuration'; title: string }

export interface DeleteLaunchConfigurationRequest {
  kind: 'delete-launch-configuration'
  title: string
  profileId: string
  profileRevision: string
  workspaceRevision: string
}
