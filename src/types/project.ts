// Project 相关类型定义

export interface Project {
  path: string
  name: string
  lastSessionId?: string
  lastCost?: number
  lastDuration?: number
}