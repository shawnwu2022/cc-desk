import { sameProjectPath } from '@/utils/path'
import type { ConfigSource } from '@/types/config'
import type { ResourceItem } from '@/types/nativeProjection'
import { settingNames, type ProjectResourceItem, type ResourceOrigin, type ResourceSettingName } from '@/types/projectResources'

/** Require an explicit platform-absolute project spelling; never ambient cwd. */
export function isResourceProjectPath(path: string): boolean {
  return typeof path === 'string' && path.length <= 32768 && !/[\p{Cc}\p{Cf}]/u.test(path)
    && (/^\//.test(path) || /^[A-Za-z]:[\\/]/.test(path) || /^\\\\[^\\]/.test(path))
}

/** The Legacy source label alone cannot prove exact project provenance. Keep
 * source paths internal and compare only to fixed files read by the existing API. */
export function hasExactProjectResourceSource(projectPath: string, source: ConfigSource, kind: 'config' | 'mcp'): boolean {
  if (!source || typeof source.path !== 'string' || !isResourceProjectPath(source.path)) return false
  const relative = kind === 'mcp'
    ? source.type === 'project' ? '.mcp.json' : null
    : source.type === 'project' ? '.claude/settings.json' : source.type === 'local' ? '.claude/settings.local.json' : null
  return relative !== null && sameProjectPath(source.path, `${projectPath.replace(/[\\/]+$/, '')}/${relative}`)
}

/** Conservative display filter, additional to the authenticated wire schema.
 * Withhold the whole field rather than accidentally preserving part of a value.
 * This is not a general-purpose secret detector or an export/sanitization API. */
function displayText(value: unknown, maximum: number): string | null {
  if (typeof value !== 'string' || new TextEncoder().encode(value).length > maximum
    || /[\p{Cc}\p{Cf}]/u.test(value.replace(/[\n\r\t]/g, ''))
    || /(?:api[ _-]?key|token|auth(?:orization)?|bearer|credential|password|passwd|secret|private[ _-]?key|headers?|\benv(?:ironment)?\b)/i.test(value)
    || /(?:sk-|gh[pousr]_|github_pat_|AKIA|ASIA|xox[baprs]-|eyJ)[A-Za-z0-9_-]+/.test(value)
    || /\b[A-Z][A-Z0-9_]{2,}\s*[:=]/.test(value)
    || /[A-Za-z]:[\\/]|\\\\|(?:^|[\s"'(=:])[~/\\]|\w:\/\//.test(value)
    || /[\\/]/.test(value)
    || /\{\s*["']|-----BEGIN|[A-Za-z0-9_+/=-]{40,}/.test(value)) return null
  return value
}
function origin(value: string): ResourceOrigin {
  if (['project', 'project-command'].includes(value)) return 'project'
  if (value === 'project-local' || value === 'local') return 'local'
  if (['global', 'global-command', 'user', 'user-config', 'root-user-config'].includes(value)) return 'global'
  if (value === 'plugin' || value.startsWith('plugin:')) return 'plugin'
  if (value === 'builtin' || value === 'managed') return value
  return 'unknown'
}
export function projectResourceItems(rows: ResourceItem[]): ProjectResourceItem[] {
  return rows.slice(0, 200).flatMap((row): ProjectResourceItem[] => {
    if (row.type === 'session' || row.type === 'message') return []
    let withheld = false
    const field = (value: unknown, maximum: number) => {
      if (value == null) return null
      const safe = displayText(value, maximum)
      if (safe === null) withheld = true
      return safe
    }
    const metadata = { origin: origin(row.origin), withheld: false }
    let item: ProjectResourceItem
    switch (row.type) {
      case 'setting':
        if (!settingNames.includes(row.name as ResourceSettingName)) return []
        item = { ...metadata, type: row.type, name: row.name as ResourceSettingName, value: field(row.value, 1024) }; break
      case 'mcp':
        item = { ...metadata, type: row.type, name: field(row.name, 4096), transport: ['stdio', 'http', 'sse'].includes(row.transport) ? row.transport as 'stdio' | 'http' | 'sse' : 'unknown' }; break
      case 'skill':
        item = { ...metadata, type: row.type, name: field(row.name, 4096), description: field(row.description, 2048) }; break
      case 'agent':
        item = { ...metadata, type: row.type, name: field(row.name, 4096), description: field(row.description, 2048), model: field(row.model, 1024) }; break
      case 'plugin':
        item = { ...metadata, type: row.type, name: field(row.name, 4096), version: field(row.version, 1024), enabled: row.enabled, installed: row.installed }; break
      case 'document':
        item = { ...metadata, type: row.type, name: row.name === '.claude/CLAUDE.md' ? row.name : field(row.name, 4096), text: field(row.text, 16384), truncated: row.truncated }; break
    }
    item.withheld = withheld
    return [item]
  })
}
