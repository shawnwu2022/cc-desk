// PTY 相关类型定义

export interface PtySpawnResult {
  id: string
  type: 'claude' | 'shell'
  cwd: string
}

export interface PtyOutputPayload {
  id: string
  data: string
}

export interface PtyExitPayload {
  id: string
  exitCode: number
}