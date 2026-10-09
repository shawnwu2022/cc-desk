const messages = {
  UPDATER_PROXY_INVALID: 'updateProxyInvalid',
  UPDATER_CONFIGURATION_INVALID: 'updateConfigurationInvalid',
  UPDATER_SETTINGS_SAVE_FAILED: 'updateProxySaveFailed',
  UPDATER_REQUEST_FAILED: 'updateRequestFailed',
  UPDATER_TIMEOUT: 'updateRequestTimeout',
  UPDATER_HTTP_REJECTED: 'updateHttpRejected',
  UPDATER_RATE_LIMITED: 'updateRateLimited',
  UPDATER_MANIFEST_INVALID: 'updateManifestInvalid',
  UPDATER_PLATFORM_UNAVAILABLE: 'updatePlatformUnavailable',
  UPDATER_NOT_OFFICIAL: 'updateSourceUnverified',
  UPDATER_SIGNATURE_INVALID: 'updateSignatureInvalid',
  UPDATER_PACKAGE_MISMATCH: 'updatePackageMismatch',
  UPDATER_ADMISSION_STALE: 'updateAdmissionStale',
  UPDATER_SESSIONS_BUSY: 'updateSessionsBusy',
  UPDATER_INSTALL_OUTCOME_UNKNOWN: 'updateInstallUnknown',
  UPDATER_CALLER_DENIED: 'updateCheckSafeFailed',
  UPDATER_FAILED: 'updateCheckSafeFailed',
} as const
const stages = new Set(['check', 'configuration', 'proxy', 'provenance', 'download', 'install', 'admission'])
export function updateFailure(cause: unknown): { code: keyof typeof messages; stage: string; key: string } {
  if (cause && typeof cause === 'object') {
    const value = cause as Record<string, unknown>
    if (typeof value.code === 'string' && Object.prototype.hasOwnProperty.call(messages, value.code)
      && typeof value.stage === 'string' && stages.has(value.stage)) {
      const code = value.code as keyof typeof messages
      return { code, stage: value.stage, key: messages[code] }
    }
  }
  return { code: 'UPDATER_FAILED', stage: 'unknown', key: 'updateCheckSafeFailed' }
}
