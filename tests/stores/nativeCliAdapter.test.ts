import { describe, expect, it, vi } from 'vitest'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import type { NativeCliTab } from '@/stores/nativeTabs'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'

function tab(overrides: Partial<NativeCliTab> = {}): NativeCliTab {
  return {
    tabId: 'tab-1', cli: 'codex', projectId: 'project-1', projectPath: '/repo', profileId: 'codex-main', profileRevision: '7',
    requestId: 'req-1', runId: 'run-1', generation: 1, action: { kind: 'resume-id', nativeSessionId: 'session-1' },
    status: 'running', errorCode: null, launchRevision: '7', title: 'Codex active', createdAt: 1, lastActivityAt: 30,
    ...overrides,
  }
}

function history(cli: 'claude' | 'codex', root: string, nativeSessionId: string, projectPath = '/repo'): NativeHistoryEntry {
  const profileId = cli === 'claude' ? 'claude-main' : 'codex-main'
  return {
    key: `${cli}:${root}`,
    context: { cli, profileId, profileRevision: '7', projectId: 'project-1', projectPath },
    sessions: [{ type: 'session', sessionKey: JSON.stringify(['local', cli, root, nativeSessionId]), nativeSessionId, title: `${cli}-${root}`, truncated: false, cwd: projectPath, updatedAt: '2026-09-29T00:00:00Z' }],
    loading: false, loaded: true, error: null, requestEpoch: '1',
  }
}

function setup(tabs: NativeCliTab[], entries: NativeHistoryEntry[]) {
  const map = new Map(tabs.map(value => [value.tabId, value]))
  const setActive = vi.fn()
  const close = vi.fn((id: string) => map.delete(id))
  const rename = vi.fn((id: string, title: string) => { const value = map.get(id); if (value) value.title = title })
  const runtime = {
    createTab: vi.fn((input: any) => tab({ tabId: 'created', cli: input.cli, projectPath: input.projectPath, action: input.action, title: input.title ?? 'Created' })),
    restartTab: vi.fn((id: string) => tab({ ...(map.get(id) ?? {}), tabId: id, generation: 2, status: 'stopped' })),
    stopTab: vi.fn().mockResolvedValue(undefined),
  }
  const archive = { archiveSession: vi.fn().mockResolvedValue(undefined), restoreSession: vi.fn().mockResolvedValue(undefined) }
  const adapter = createNativeCliAdapter({ tabs: { tabs: map, activeTabId: null, setActive, close, rename }, history: { all: () => entries }, runtime, archive })
  return { adapter, map, runtime, archive, setActive }
}

describe('native CLI adapter', () => {
  it('mixes Claude and Codex history and preserves duplicate native ids across source roots', async () => {
    const { adapter } = setup([], [history('claude', '/claude-a', 'same'), history('codex', '/codex-a', 'same'), history('codex', '/codex-b', 'same')])
    const sessions = await adapter.listSessions('/repo')
    expect(sessions).toHaveLength(3)
    expect(new Set(sessions.map(x => x.id)).size).toBe(3)
    expect(sessions.map(x => x.cli).sort()).toEqual(['claude', 'codex', 'codex'])
  })

  it('active tab claims only exact cli/profile/project/native identity', async () => {
    const active = tab()
    const { adapter } = setup([active], [history('codex', '/root', 'session-1'), history('claude', '/root', 'session-1')])
    const sessions = await adapter.listSessions('/repo')
    expect(sessions.filter(x => x.nativeSessionId === 'session-1')).toHaveLength(2)
    expect(sessions.some(x => x.id === 'native-tab:tab-1')).toBe(true)
    expect(sessions.some(x => x.cli === 'claude')).toBe(true)
  })

  it('resuming an already-open native session activates instead of duplicating', async () => {
    const active = tab()
    const { adapter, runtime, setActive } = setup([active], [])
    const resumed = await adapter.resumeSession({ projectKey: '/repo', projectPath: '/repo', cli: 'codex', adapterSessionId: 'key', nativeSessionId: 'session-1' })
    expect(resumed.id).toBe('native-tab:tab-1')
    expect(setActive).toHaveBeenCalledWith('tab-1')
    expect(runtime.createTab).not.toHaveBeenCalled()
  })

  it('rejects restart from unknown state and archives only after stop succeeds', async () => {
    const unknown = tab({ status: 'unknown' })
    const one = setup([unknown], [])
    await expect(one.adapter.restartSession('native-tab:tab-1')).rejects.toThrow('LAUNCH_STATE_UNKNOWN')

    const running = tab({ tabId: 'tab-2' })
    const two = setup([running], [])
    await two.adapter.archiveSession('native-tab:tab-2')
    expect(two.runtime.stopTab).toHaveBeenCalledTimes(1)
    expect(two.archive.archiveSession).toHaveBeenCalledWith('/repo', 'session-1')
  })
})