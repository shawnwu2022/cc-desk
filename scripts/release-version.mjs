const releaseVersion = /^\d+\.\d+\.\d+(?:-rc\.[1-9]\d*)?$/

export function releaseChannel(version) {
  if (typeof version !== 'string' || !releaseVersion.test(version)) return null
  return version.includes('-rc.') ? 'prerelease' : 'stable'
}
