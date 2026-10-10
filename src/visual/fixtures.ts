import type { UnifiedSession } from '@/types/unifiedSession'
import type { CliProfile } from '@/types/profile'
import type { ProjectResourceItem } from '@/types/projectResources'

export const FIXTURE_TIME = Date.parse('2026-09-28T12:00:00Z')
export const longProjectName = 'Long project name for responsive validation '.repeat(2).slice(0, 80)
export const longSessionTitle = 'Review cross-platform terminal continuity and preserve exact session ownership '.repeat(3).slice(0, 200)
export const projectPaths = ['C:/visual/cc-desk', 'C:/visual/atlas', 'C:/visual/long-project', 'C:/visual/empty-project']
export function fixtureSessions(): UnifiedSession[] {
  return [
    ['Review terminal rendering', 'claude', 'running', 'none', 2],
    ['Implement resource drawer', 'codex', 'running', 'needs-user', 17],
    [longSessionTitle, 'claude', 'starting', 'none', 0],
    ['Awaiting status confirmation', 'codex', 'unknown', 'none', 64],
    ['Settings accessibility review', 'claude', 'stopped', 'none', 140],
    ['Retry project preparation', 'codex', 'failed', 'none', 1440],
    ['Archive: keyboard navigation', 'claude', 'stopped', 'none', 2880],
    ['Archive: read-only resources', 'codex', 'stopped', 'none', 5000],
  ].map(([title, cli, processState, attentionState, age], index) => ({
    id: `visual-session-${index}`, adapterSessionId: `visual-tab-${index}`,
    projectKey: projectPaths[index < 4 ? 0 : index === 4 ? 1 : 2].toLowerCase(),
    projectPath: projectPaths[index < 4 ? 0 : index === 4 ? 1 : 2],
    title: String(title), cli: cli as UnifiedSession['cli'], runtime: index === 4 ? 'legacy-claude' : 'native-cli',
    processState: processState as UnifiedSession['processState'], attentionState: attentionState as UnifiedSession['attentionState'],
    activityState: processState === 'running' ? 'idle' : 'unknown',
    lastActivityAt: FIXTURE_TIME - Number(age) * 60_000, archived: index >= 6, opened: index < 4, preparationState: index === 5 ? 'failed' as const : undefined, resumable: index >= 4,
  }))
}
export const fixtureProfiles: CliProfile[] = ['claude', 'codex'].flatMap(cli => [0, 1].map(index => ({
  id: `visual-${cli}-${index}`, revision: '1', cli: cli as 'claude' | 'codex',
  name: `${cli === 'claude' ? 'Claude Code' : 'Codex CLI'} · ${index === 0 ? 'Everyday development' : 'Read-only review'}`,
  launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'set', value: [] },
  skipPermissions: { mode: 'set', value: false }, observer: { mode: 'inherit' }, env: {},
})))
export const fixtureResources: ProjectResourceItem[] = [
  { type: 'mcp', name: 'Project documentation', transport: 'stdio', origin: 'project', withheld: false },
  { type: 'mcp', name: 'Issue tracker (read only)', transport: 'http', origin: 'project', withheld: false },
  { type: 'mcp', name: 'Shared design references', transport: 'sse', origin: 'managed', withheld: false },
]
