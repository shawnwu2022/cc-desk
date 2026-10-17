import type { UnifiedProjectGroup, UnifiedSession } from '@/types/unifiedSession'

export type SessionStatusKind = 'error' | 'permission' | 'unread-reply' | 'completed' | 'needs-user' | 'running'

/** 只允许既有 i18n 词条：这几个键都接受 { count } 参数。 */
export type SessionStatusLabelKey = 'projectErrorCount' | 'projectPermissionCount' | 'projectUnreadReplyCount'
  | 'projectCompletedCount' | 'projectNeedsReplyCount' | 'projectRunningCount'

export interface SessionStatusBadge {
  kind: SessionStatusKind
  count: number
  labelKey: SessionStatusLabelKey
}

/**
 * 项目聚合字段缺失时的行级回退形状。四个可选字段在生产投影上恒为 undefined
 * （`projectManagement.ts` 只构造 runningCount/needsUserCount），所以行级回退是主路径。
 */
type StatusAggregate = Pick<UnifiedProjectGroup,
  'runningCount' | 'needsUserCount' | 'errorCount' | 'permissionCount' | 'completedCount' | 'unreadReplyCount'>

/**
 * 按固定优先级把一个项目的行（或某 CLI 组的行）归并成单个状态徽章。
 * `aggregate` 省略时 needs-user / running 也走行级统计——组头必须只反映本组行，
 * 传项目聚合会让每个组都显示项目总数。
 */
export function summarizeSessionStatus(sessions: UnifiedSession[], aggregate?: StatusAggregate): SessionStatusBadge | null {
  // 注意：needs-user / running 在 aggregate 省略时才走行级统计。项目调用始终传
  // `props.project`，所以项目徽章行为不变；组头不传，所以这两个计数按本组行算——
  // 这是组头需要的新语义，不是对项目徽章的改动。
  // 回退统计只覆盖运行中的行，非运行行的关注原因来自聚合字段。
  const running = sessions.filter(session => session.processState === 'running')
  const error = aggregate?.errorCount ?? running.filter(session => session.attentionKind === 'error' || session.activityState === 'error').length
  const permission = aggregate?.permissionCount ?? running.filter(session => session.attentionKind !== 'error'
    && session.activityState !== 'error' && (session.attentionKind === 'permission' || session.activityState === 'waiting_permission')).length
  const unreadReply = aggregate?.unreadReplyCount ?? sessions.filter(session => session.observationNotice?.unreadReplyEnd?.kind === 'reply-ended').length
  const completed = aggregate?.completedCount ?? running.filter(session => session.attentionKind === 'completed'
    && session.activityState !== 'error' && session.activityState !== 'waiting_permission').length
  const needsUser = aggregate?.needsUserCount ?? sessions.filter(session => session.attentionState === 'needs-user').length
  // 与 unifiedSessions 的聚合口径一致：starting 也算运行中。
  const runningTotal = aggregate?.runningCount ?? sessions.filter(session => session.processState === 'running' || session.processState === 'starting').length
  if (error > 0) return { kind: 'error', count: error, labelKey: 'projectErrorCount' }
  if (permission > 0) return { kind: 'permission', count: permission, labelKey: 'projectPermissionCount' }
  if (unreadReply > 0) return { kind: 'unread-reply', count: unreadReply, labelKey: 'projectUnreadReplyCount' }
  if (completed > 0) return { kind: 'completed', count: completed, labelKey: 'projectCompletedCount' }
  if (needsUser > 0) return { kind: 'needs-user', count: needsUser, labelKey: 'projectNeedsReplyCount' }
  if (runningTotal > 0) return { kind: 'running', count: runningTotal, labelKey: 'projectRunningCount' }
  return null
}
