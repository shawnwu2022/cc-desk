export type Platform = 'macos' | 'windows' | 'linux' | 'unknown'

export function detectPlatform(): Platform {
  const ua = navigator.userAgent.toLowerCase()
  if (ua.includes('mac')) return 'macos'
  if (ua.includes('win')) return 'windows'
  if (ua.includes('linux')) return 'linux'
  return 'unknown'
}

export const platform = detectPlatform()
export const isMac = platform === 'macos'
export const isWindows = platform === 'windows'
