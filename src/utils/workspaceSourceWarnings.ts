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
export interface WorkspaceSourceConfiguration {
  warningKey: string
  profileId: string
  profileRevision: string
  name: string
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
