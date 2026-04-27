// App 配置相关类型定义

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
