import { projectionErrorCode, projectionErrorStage, type ProjectionStage } from '@/api/nativeProjection'

export const workspaceSourceLabels = {
  'project-metadata': 'sourceWarningProjectMetadata',
  'project-discovery': 'sourceWarningProjectDiscovery',
  configurations: 'sourceWarningConfigurations',
  registrations: 'sourceWarningRegistrations',
  'legacy-history': 'sourceWarningLegacyHistory',
  'claude-history': 'sourceWarningClaudeHistory',
  'codex-history': 'sourceWarningCodexHistory',
  catalog: 'sourceWarningCatalog',
} as const
export type WorkspaceWarningSource = keyof typeof workspaceSourceLabels
export interface WorkspaceSourceWarning { source: WorkspaceWarningSource; code: string; stage?: ProjectionStage }
const responseCodes = new Set(['INVALID_PROFILE_RESPONSE', 'INVALID_WORKSPACE_RESPONSE', 'INVALID_PROJECTION', 'RAW_BODY_REQUIRED', 'REQUEST_TOO_LARGE', 'CLOCK_UNAVAILABLE'])
const MAX_SOURCE_WARNINGS = 12

/** Only exact public codes survive. Native fields, paths and exception text never do. */
function safeCode(failure: unknown): string {
  const candidate = failure instanceof Error ? failure.message
    : failure && typeof failure === 'object' && Object.prototype.hasOwnProperty.call(failure, 'code') ? (failure as { code: unknown }).code : null
  return typeof candidate === 'string' && responseCodes.has(candidate) ? candidate : projectionErrorCode({ code: candidate })
}

export function createWorkspaceSourceWarnings(initial: readonly WorkspaceSourceWarning[] = [], truncated = false) {
  const result = {
    items: [] as WorkspaceSourceWarning[],
    truncated,
    add(source: WorkspaceWarningSource, failure?: unknown) {
      const code = safeCode(failure)
      const stage = projectionErrorStage(failure)
      if (result.items.some(row => row.source === source && row.code === code && row.stage === stage)) return
      if (result.items.length === MAX_SOURCE_WARNINGS) { result.truncated = true; return }
      result.items.push({ source, code, ...(stage ? { stage } : {}) })
    },
  }
  for (const row of initial) result.add(row.source, { code: row.code, stage: row.stage })
  return result
}
