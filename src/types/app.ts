import type { UpdateChannel } from '@/utils/updatePolicy'
import type { ShortcutBindings } from '@/config/appShortcuts'
import type { SessionRuntimeKind, UnifiedCliKind } from './unifiedSession'

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

export type GuiThemeMode = 'light' | 'dark' | 'system'
export type GuiDensity = 'standard' | 'compact'
export type StartupDestination = 'workspace' | 'projects'

export interface AppConfig {
  shortcutBindings?: ShortcutBindings
  defaultContinue?: boolean
  defaultSkipPermissions?: boolean
  defaultCustomArgs?: string
  theme?: 'light' | 'dark'
  guiThemeMode?: GuiThemeMode
  guiDensity?: GuiDensity
  sidebarWidth?: number
  startupDestination?: StartupDestination
  defaultNewCli?: UnifiedCliKind
  terminalTheme?: string
  fontSize?: number
  terminalFontFamily?: string
  terminalLineHeight?: number
  terminalCursorStyle?: 'bar' | 'block' | 'underline'
  terminalCursorBlink?: boolean
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
export interface SessionUiRecord {
  runtime: SessionRuntimeKind
  cli: UnifiedCliKind
  projectPath: string
  adapterSessionId: string
  nativeSessionId?: string | null
  title: string
  lastActivityAt: number
  /** Accepted opening time, independent of output and status activity. */
  lastOpenedAt?: number
}

export interface ProjectLaunchPreference {
  lastCli: UnifiedCliKind
  claudeLaunchConfigId?: string | null
  codexLaunchConfigId?: string | null
}

export interface ProjectsState {
  pinnedProjects: string[]
  archivedSessions: Record<string, string[]>
  displayNames?: Record<string, string>
  sessionRecords?: Record<string, SessionUiRecord>
  launchPreferences?: Record<string, ProjectLaunchPreference>
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
  admissionId?: string | null
  officialRelease?: { id: number; tag: string; sourceSha: string } | null
  eligibilityReason?: string | null
  channel?: UpdateChannel
  installEligible?: boolean
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
