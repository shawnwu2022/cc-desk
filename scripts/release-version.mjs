const releaseVersion = /^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-(beta|rc|preview)\.[1-9]\d*)?$/

export function releaseChannel(version) {
  if (typeof version !== 'string' || version !== version.trim()) return null
  const match = releaseVersion.exec(version)
  if (!match) return null
  return match[1] ? 'prerelease' : 'stable'
}
