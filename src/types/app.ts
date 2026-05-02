// App 配置相关类型定义

export interface CheckResult {
  name: string
  passed: boolean
  message: string
  detectedPath?: string
  action?: string
  url?: string
}

export interface HomeData {
  projects: import('./project').Project[]
  recentSessions: import('./session').SessionInfo[]
  hasMore: boolean
}

export interface AppConfig {
  defaultContinue?: boolean
  defaultSkipPermissions?: boolean
  defaultCustomArgs?: string
  theme?: 'light' | 'dark'
  fontSize?: number
  autoConnectIde?: boolean
  hiddenProjects?: string[]
  lastOpenedProject?: string
  windowSize?: { width: number; height: number }
}

export interface DefaultClaudeOptions {
  skipPermissions?: boolean
  customArgs?: string
}

// Claude 启动选项（前端使用）
export interface ClaudeOptions {
  resume: string
  skipPermissions: boolean
  customArgs: string
}

// 软件更新信息
export interface PlatformAsset {
  name: string
  url: string
  size: number
}

export interface UpdateInfo {
  version: string
  currentVersion: string
  hasUpdate: boolean
  releaseNotes: string
  downloadUrl: string
  platformAsset: PlatformAsset | null
}

export interface DownloadProgress {
  downloaded: number
  total: number
  percent: number
}
