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
  startupState: ProjectStartupState
}

export interface AppConfig {
  defaultContinue?: boolean
  defaultSkipPermissions?: boolean
  defaultCustomArgs?: string
  theme?: 'light' | 'dark'
  terminalTheme?: string
  fontSize?: number
  webglRenderer?: boolean
  autoConnectIde?: boolean
  hiddenProjects?: string[]
  lastOpenedProject?: string
  windowSize?: { width: number; height: number }
  claudeEnvVars?: Record<string, string>
  language?: 'en' | 'zh'
}

// 项目置顶 + 会话存档 + 项目别名持久化状态（~/.cc-box/projects.json，与 config.json 分开存储）
// 后端 merge 为顶层替换：写入时须发送完整 pinnedProjects / archivedSessions / displayNames
export interface ProjectsState {
  pinnedProjects: string[]
  archivedSessions: Record<string, string[]>
  displayNames?: Record<string, string>
}

export interface DefaultClaudeOptions {
  skipPermissions: boolean
  customArgs: string
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

// Claude CLI 更新信息
export interface ClaudeCliUpdateInfo {
  installedVersion: string | null
  latestVersion: string
  hasUpdate: boolean
  notInstalled: boolean
}

// Claude CLI 单个历史版本条目
export interface ClaudeVersionEntry {
  version: string
  releaseDate: string
  platforms: Record<string, ClaudePlatformInfo>
}

// Claude CLI 版本的平台产物信息
export interface ClaudePlatformInfo {
  url: string
  checksum: string
  size: number
}

// versions.json 顶层结构
export interface ClaudeVersions {
  latest: string
  updatedAt: string
  versions: ClaudeVersionEntry[]
}

/// 启动摘要：单个项目信息（供前端缓存）
export interface ProjectInfo {
  path: string
  name: string
  exists: boolean
}

/// 启动摘要：项目存在性 + 可见性 + lastOpened 信息
export interface ProjectStartupState {
  hasAnyProject: boolean
  hasVisibleProject: boolean
  lastOpenedProjectInfo: ProjectInfo | null
}
