// Session 相关类型定义

export interface SessionInfo {
  sessionId: string
  name: string
  projectPath: string
  lastActiveAt: number
}

export interface SessionDetails {
  sessionId: string
  name: string
  messageCount: number
  totalTokens?: number
  totalCost?: number
  createdAt?: number
  lastActiveAt: number
}