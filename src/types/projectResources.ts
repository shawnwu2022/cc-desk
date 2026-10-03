import type { ResourceKind, ScopeTarget } from './nativeProjection'
import type { UnifiedCliKind } from './unifiedSession'

export type ProjectResourceKind = Extract<ResourceKind, 'instructions' | 'config' | 'mcp' | 'skills' | 'agents' | 'plugins'>
export type ResourceOrigin = 'project' | 'local' | 'global' | 'plugin' | 'builtin' | 'managed' | 'unknown'
export const settingNames = ['model', 'language', 'outputStyle', 'model_reasoning_effort', 'approval_policy', 'sandbox_mode'] as const
export type ResourceSettingName = typeof settingNames[number]
interface DisplayBase { origin: ResourceOrigin; withheld: boolean }
/** Display-only DTOs: no source handles, paths, IDs, env, argv, headers or raw records. */
export type ProjectResourceItem = DisplayBase & (
  | { type: 'setting'; name: ResourceSettingName; value: string | null }
  | { type: 'mcp'; name: string | null; transport: 'stdio' | 'http' | 'sse' | 'unknown' }
  | { type: 'skill'; name: string | null; description: string | null }
  | { type: 'agent'; name: string | null; description: string | null; model: string | null }
  | { type: 'plugin'; name: string | null; version: string | null; enabled: boolean | null; installed: boolean | null }
  | { type: 'document'; name: string | null; text: string | null; truncated: boolean }
)
interface ContextBase { sessionId: string; projectId: string; projectPath: string; cli: UnifiedCliKind; attempt: string }
export type ProjectResourceContext = ContextBase & (
  | { runtime: 'native-cli'; profileId: string; profileRevision: string; target: ScopeTarget }
  | { runtime: 'legacy-claude' }
)
