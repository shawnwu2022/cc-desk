import { safeUserErrorCode } from './userError'

/** Local prerequisite failure, before an adapter or launch attempt is admitted. */
export class LaunchConfigurationRequiredError extends Error {
  readonly issueCode: string
  constructor(readonly profileId: string, issue?: unknown) {
    super('LAUNCH_CONFIGURATION_REQUIRED')
    this.issueCode = safeUserErrorCode(issue)
  }
}
