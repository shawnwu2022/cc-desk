/** Local prerequisite failure, before an adapter or launch attempt is admitted. */
export class LaunchConfigurationRequiredError extends Error {
  constructor(readonly profileId: string) {
    super('LAUNCH_CONFIGURATION_REQUIRED')
  }
}
