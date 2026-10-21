import { projectionErrorCode, projectionErrorStage, type ProjectionStage } from '@/api/nativeProjection'
import type { InjectionKey } from 'vue'
import type { UnifiedSession } from '@/types/unifiedSession'

/** 只控制警告展示，不提供读取、恢复或运行权限。 */
export const SESSION_HISTORY_WARNING_VISIBLE: InjectionKey<(session: UnifiedSession) => boolean> = Symbol('session-history-warning-visible')

export const workspaceSourceLabels = {
  'project-metadata': 'sourceWarningProjectMetadata',
  'project-discovery': 'sourceWarningProjectDiscovery',
  configurations: 'sourceWarningConfigurations',
  registrations: 'sourceWarningRegistrations',
  'legacy-history': 'sourceWarningLegacyHistory',
  'claude-history': 'sourceWarningClaudeHistory',
  'codex-history': 'sourceWarningCodexHistory',
  'opencode-history': 'sourceWarningOpencodeHistory',
  catalog: 'sourceWarningCatalog',
} as const
export type WorkspaceWarningSource = keyof typeof workspaceSourceLabels
export interface WorkspaceSourceWarning { source: WorkspaceWarningSource; code: string; stage?: ProjectionStage }
export interface WorkspaceSourceConfiguration {
  warningKey: string
  profileId: string
  profileRevision: string
  name: string
}
export interface WorkspaceHistoryIssue {
  key: string
  entryKey: string
  warningKey: string
  source: WorkspaceWarningSource
  code: string
  stage?: ProjectionStage
  configurationName: string
  projectPath: string
  sessionId?: string
  sessionTitle?: string
}
/** 仅解释后端明确保留的类别，不从泛化读取失败猜测损坏、权限或缺失文件。 */
export function historyRecoveryHint(code: string): string {
  if (code === 'SOURCE_UNSUPPORTED' || code === 'UNSUPPORTED_SCHEMA') return 'historyRecoveryUnsupported'
  if (code === 'SOURCE_INVALID' || code === 'SOURCE_INVALID_TEXT') return 'historyRecoveryInvalid'
  if (code === 'SOURCE_READ_FORBIDDEN' || code === 'FORBIDDEN') return 'historyRecoveryPermission'
  if (code === 'PROFILE_NOT_FOUND' || code === 'PROJECT_NOT_FOUND' || code === 'ENV_SOURCE_MISSING') return 'historyRecoveryMissing'
  if (code === 'SOURCE_PATH_REJECTED' || code === 'SOURCE_NOT_REGULAR' || code === 'UNSAFE_WORKSPACE_PATH') return 'historyRecoveryUnsafePath'
  if (code === 'SOURCE_TOO_LARGE' || code === 'SOURCE_TOO_MANY_ENTRIES' || code === 'SOURCE_RESPONSE_TOO_LARGE') return 'historyRecoveryLimit'
  if (code === 'SOURCE_BUSY' || code === 'SOURCE_CHANGED' || code === 'SOURCE_BUDGET_EXCEEDED' || code === 'SOURCE_SNAPSHOT_EXPIRED') return 'historyRecoveryTemporary'
  if (code.startsWith('SCOPE_') || code === 'REVISION_CONFLICT' || code === 'PROJECT_IDENTITY_CHANGED') return 'historyRecoveryScope'
  return 'historyRecoveryUnavailable'
}
export function workspaceWarningKey(warning: WorkspaceSourceWarning): string {
  return JSON.stringify([warning.source, warning.code, warning.stage ?? ''])
}
const responseCodes = new Set(['INVALID_PROFILE_RESPONSE', 'INVALID_WORKSPACE_RESPONSE', 'INVALID_PROJECTION', 'RAW_BODY_REQUIRED', 'REQUEST_TOO_LARGE', 'CLOCK_UNAVAILABLE'])
const MAX_SOURCE_WARNINGS = 12

/** Only exact public codes survive. Native fields, paths and exception text never do. */
function safeCode(failure: unknown): string {
  const candidate = failure instanceof Error ? failure.message
    : failure && typeof failure === 'object' && Object.prototype.hasOwnProperty.call(failure, 'code') ? (failure as { code: unknown }).code : null
  return typeof candidate === 'string' && responseCodes.has(candidate) ? candidate : projectionErrorCode({ code: candidate })
}

export function createWorkspaceSourceWarnings(initial: readonly WorkspaceSourceWarning[] = [], truncated = false) {
  // Fixed allowlisted triples only: complete identity is independent of the UI cap.
  const identities = new Set<string>()
  const result = {
    items: [] as WorkspaceSourceWarning[],
    truncated,
    identityKeys: () => [...identities],
    add(source: WorkspaceWarningSource, failure?: unknown): WorkspaceSourceWarning {
      const code = safeCode(failure)
      const stage = projectionErrorStage(failure)
      identities.add(workspaceWarningKey({ source, code, ...(stage ? { stage } : {}) }))
      const existing = result.items.find(row => row.source === source && row.code === code && row.stage === stage)
      if (existing) return existing
      const warning = { source, code, ...(stage ? { stage } : {}) }
      result.items.push(warning)
      if (result.items.length > MAX_SOURCE_WARNINGS) result.truncated = true
      // Once capped, stable selection prevents completion order from turning the
      // same failures into a different apparent warning set on every refresh.
      if (result.truncated) result.items.sort((a, b) => workspaceWarningKey(a).localeCompare(workspaceWarningKey(b), 'en'))
      if (result.items.length > MAX_SOURCE_WARNINGS) result.items.length = MAX_SOURCE_WARNINGS
      return warning
    },
  }
  for (const row of initial) result.add(row.source, { code: row.code, stage: row.stage })
  return result
}
