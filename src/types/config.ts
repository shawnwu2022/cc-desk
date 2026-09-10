// Config 相关类型定义

export interface ConfigSource {
  type: 'project' | 'user' | 'local' | 'managed' | 'builtin' | 'plugin'
  label: string
  path?: string
}

export interface BasicConfigItem {
  model?: string
  theme?: string
  editorMode?: string
  autoConnectIde?: boolean
  permissions?: {
    allow?: string[]
    deny?: string[]
  }
  env?: Record<string, string>
  source: ConfigSource
}

export interface McpServerItem {
  name: string
  type?: 'stdio' | 'http' | 'sse'
  command?: string
  args?: string[]
  env?: Record<string, string>
  url?: string
  source: ConfigSource
}

export interface SkillItem {
  name: string
  description: string
  path: string
  source: ConfigSource
}

export interface AgentItem {
  name: string
  description: string
  path: string
  source: ConfigSource
}

export interface HookItem {
  event: string
  matcher?: string
  command?: string
  type?: string
  source: ConfigSource
}

export interface ProjectConfigResult {
  basic: BasicConfigItem[]
  mcp: McpServerItem[]
  skills: SkillItem[]
  agents: AgentItem[]
  hooks: HookItem[]
}

export interface SkillInfo {
  name: string
  displayName: string
  description?: string
  sourceType: 'project' | 'user' | 'plugin'
  sourceLabel: string
  invokeFormat: string
  enabled?: boolean
}

export interface AgentInfo {
  name: string
  displayName: string
  description?: string
  sourceType: 'builtin' | 'plugin' | 'user' | 'project'
  sourceLabel: string
  model?: string
  invokeFormat: string
  enabled?: boolean
}

/**
 * MCP 配置的只读投影。认证 headers 与 env 不进入 WebView；
 * CC Desk 不连接或启动 MCP Server 来探测运行时详情。
 */
export interface McpServerInfo {
  name: string
  displayName: string
  description?: string
  sourceType: 'plugin' | 'user' | 'project' | 'local' | 'managed'
  sourceLabel: string
  serverType?: string
  status?: string
  url?: string
  command?: string
  args?: string[]
  prompts: McpPromptInfo[]
  enabled?: boolean
}

export interface McpPromptInfo {
  name: string
  description?: string
  invokeFormat: string
}

export interface PluginSkill {
  name: string
  description?: string
  invokeFormat: string
}

export interface PluginAgent {
  name: string
  description?: string
  model?: string
  invokeFormat: string
}

export interface PluginInfo {
  id: string
  name: string
  version: string
  scope: 'user' | 'project'
  enabled: boolean
  installPath: string
  installedAt?: string
  lastUpdated?: string
  projectPath?: string
  skills?: PluginSkill[]
  agents?: PluginAgent[]
  mcpServers?: Record<string, {
    type?: string
    command?: string
    args?: string[]
  }>
}
