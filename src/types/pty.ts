// PTY 相关类型定义

export interface PtySpawnResult {
  id: string
  type: 'claude' | 'shell'
  cwd: string
  /** 本次启动冻结的根身份证明：有效环境未覆盖 CLAUDE_CONFIG_DIR，即跑在 ~/.claude。 */
  usesDefaultClaudeRoot: boolean
}

export interface PtyOutputPayload {
  id: string
  data: string
}

export interface PtyExitPayload {
  id: string
  exitCode: number
  signal?: string | null
}