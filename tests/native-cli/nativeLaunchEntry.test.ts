import { describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import type { CliProfile } from '@/types/profile'
import type { LaunchAttempt, LaunchStatus } from '@/api/cliLaunchAttempt'
import { createNativeLaunchEntry } from '@/terminal/nativeLaunchEntry'

function profile(id: string, cli: 'claude' | 'codex', revision = '7'): CliProfile {
  return {
    id,
    revision,
    cli,
    name: id,
    launcher: { kind: 'native' },
    programPath: { mode: 'inherit' },
    defaultArgs: { mode: 'inherit' },
    skipPermissions: { mode: 'inherit' },
    observer: { mode: 'inherit' },
    env: {},
  }
}

function running(requestId: string, runId: string, generation: number): LaunchStatus {
  return {
    instanceId: 'backend-d23',
    requestId,
    run: { runId, generation },
    revision: '2',
    phase: 'running',
    failure: null,
  }
}

function input(overrides: Record<string, unknown> = {}) {
  return {
    requestId: 'request-1',
    tabId: 'tab-1',
    runId: 'run-1',
    generation: 1,
    cli: 'claude' as const,
    launchCwd: 'C:\\repo',
    action: { kind: 'new' as const },
    extraArgs: [] as string[],
    cols: 120,
    rows: 40,
    ...overrides,
  }
}

describe('D23 native launch/recovery user entry', () => {
  // NativeCliTerminal 传入真实 Pinia tab.action；代理对象必须经过字段校验生成独立请求。
  it.each(['claude', 'codex'] as const)('Launch_RealStoreAction_010: %s', async cli => {
    setActivePinia(createPinia())
    const tabs = useNativeTabsStore()
    const created = tabs.create({ cli, projectId: 'project', projectPath: 'C:\\repo', profileId: `${cli}-main`, profileRevision: '7', action: { kind: 'new' } })
    const tab = tabs.tab(created.tabId)!
    let captured: unknown
    const entry = createNativeLaunchEntry({ selectedProfile: () => profile(`${cli}-main`, cli), createAttempt: request => {
      captured = request.action
      return { start: async () => running(request.requestId, request.runId, request.generation), recover: vi.fn(), latest: vi.fn() }
    } })
    await expect(entry.start({ ...input(), requestId: tab.requestId, tabId: tab.tabId, runId: tab.runId, generation: tab.generation, cli, action: tab.action }, {} as any))
      .resolves.toMatchObject({ phase: 'running', requestId: tab.requestId })
    expect(captured).toEqual({ kind: 'new' })
    expect(captured).not.toBe(tab.action)
  })
  // raw argv 从响应式数组复制后，后续界面更改不能改变已冻结的启动请求。
  it('Launch_ReactiveArgvSnapshot_011', async () => {
    setActivePinia(createPinia())
    const tabs = useNativeTabsStore()
    const created = tabs.create({ cli: 'codex', projectId: 'project', projectPath: 'C:\\repo', profileId: 'codex-main', profileRevision: '7', action: { kind: 'raw', argv: ['two words', '', '--literal= x'] } })
    const tab = tabs.tab(created.tabId)!
    let captured: unknown
    const entry = createNativeLaunchEntry({ selectedProfile: () => profile('codex-main', 'codex'), createAttempt: request => {
      captured = request.action
      return { start: async () => running(request.requestId, request.runId, request.generation), recover: vi.fn(), latest: vi.fn() }
    } })
    await entry.start({ ...input(), cli: 'codex', action: tab.action }, {} as any)
    if (tab.action.kind === 'raw') tab.action.argv.splice(0, 3, 'changed')
    expect(captured).toEqual({ kind: 'raw', argv: ['two words', '', '--literal= x'] })
  })
  it('D23_Launch_FreezesSelectedProfileRevisionIntoOneAttempt_01', async () => {
    const selected = { claude: profile('claude-main', 'claude', '9'), codex: profile('codex-main', 'codex', '4') }
    let captured: any
    const attempt: LaunchAttempt = {
      start: vi.fn(async () => running('request-1', 'run-1', 1)),
      recover: vi.fn(),
      latest: vi.fn(),
    }
    const createAttempt = vi.fn((request: any, _channel: any) => {
      captured = request
      return attempt
    })
    const entry = createNativeLaunchEntry({
      selectedProfile: cli => selected[cli],
      createAttempt,
    })

    const channel = { identity: 'channel-1' } as any
    await entry.start(input(), channel)

    expect(createAttempt).toHaveBeenCalledTimes(1)
    expect(captured).toEqual({
      ...input(),
      profileId: 'claude-main',
      expectedProfileRevision: '9',
    })
    expect(createAttempt.mock.calls[0][1]).toBe(channel)
    expect(attempt.start).toHaveBeenCalledTimes(1)

    selected.claude = profile('claude-other', 'claude', '10')
    expect(captured.profileId).toBe('claude-main')
    expect(captured.expectedProfileRevision).toBe('9')
  })

  it('D23_Launch_LostStartResponseRecoversOriginalAttemptWithoutRespawn_02', async () => {
    const start = vi.fn(async () => { throw new Error('LAUNCH_STATE_UNKNOWN') })
    const recover = vi.fn(async () => running('request-1', 'run-1', 1))
    const createAttempt = vi.fn(() => ({ start, recover, latest: vi.fn() }))
    const entry = createNativeLaunchEntry({
      selectedProfile: () => profile('claude-main', 'claude'),
      createAttempt,
    })

    await expect(entry.start(input(), {} as any)).rejects.toThrow('LAUNCH_STATE_UNKNOWN')
    expect(await entry.recover('request-1')).toEqual(running('request-1', 'run-1', 1))

    expect(createAttempt).toHaveBeenCalledTimes(1)
    expect(start).toHaveBeenCalledTimes(1)
    expect(recover).toHaveBeenCalledTimes(1)
  })

  it('D23_Launch_DuplicateRequestIdNeverCreatesSecondAttempt_03', async () => {
    const start = vi.fn(async () => running('request-1', 'run-1', 1))
    const createAttempt = vi.fn(() => ({ start, recover: vi.fn(), latest: vi.fn() }))
    const entry = createNativeLaunchEntry({
      selectedProfile: () => profile('claude-main', 'claude'),
      createAttempt,
    })
    const channel = {} as any

    const first = entry.start(input(), channel)
    const second = entry.start(input(), channel)
    expect(second).toBe(first)
    await first

    expect(createAttempt).toHaveBeenCalledTimes(1)
    expect(start).toHaveBeenCalledTimes(1)
  })

  it('D23_Launch_ConflictingReuseOfRequestIdFailsClosed_04', async () => {
    const createAttempt = vi.fn(() => ({
      start: vi.fn(async () => running('request-1', 'run-1', 1)),
      recover: vi.fn(),
      latest: vi.fn(),
    }))
    const entry = createNativeLaunchEntry({
      selectedProfile: () => profile('claude-main', 'claude'),
      createAttempt,
    })

    await entry.start(input(), {} as any)
    await expect(entry.start(input({ runId: 'run-2' }), {} as any))
      .rejects.toThrow('LAUNCH_REQUEST_ID_CONFLICT')
    expect(createAttempt).toHaveBeenCalledTimes(1)
  })

  it('D23_Launch_RequiresSelectedProfileForExactCli_05', async () => {
    const createAttempt = vi.fn()
    const entry = createNativeLaunchEntry({
      selectedProfile: cli => cli === 'codex' ? profile('codex-main', 'codex') : null,
      createAttempt,
    })

    await expect(entry.start(input(), {} as any)).rejects.toThrow('CLI_PROFILE_REQUIRED')
    expect(createAttempt).not.toHaveBeenCalled()
  })

  it('D23_Launch_RejectsCrossCliProfileBeforeAttemptCreation_06', async () => {
    const createAttempt = vi.fn()
    const entry = createNativeLaunchEntry({
      selectedProfile: () => profile('codex-main', 'codex'),
      createAttempt,
    })

    await expect(entry.start(input(), {} as any)).rejects.toThrow('PROFILE_CLI_MISMATCH')
    expect(createAttempt).not.toHaveBeenCalled()
  })

  it('D23_Launch_PreservesExplicitResumeIdentity_07', async () => {
    let captured: any
    const entry = createNativeLaunchEntry({
      selectedProfile: () => profile('codex-main', 'codex', '11'),
      createAttempt: request => {
        captured = request
        return {
          start: vi.fn(async () => running(request.requestId, request.runId, request.generation)),
          recover: vi.fn(),
          latest: vi.fn(),
        }
      },
    })

    await entry.start(input({
      cli: 'codex',
      action: { kind: 'resume-id', nativeSessionId: 'codex-session-123' },
    }), {} as any)

    expect(captured.cli).toBe('codex')
    expect(captured.profileId).toBe('codex-main')
    expect(captured.expectedProfileRevision).toBe('11')
    expect(captured.action).toEqual({
      kind: 'resume-id',
      nativeSessionId: 'codex-session-123',
    })
  })

  it('D23_Launch_UnknownRequestCannotBeRecoveredOrRecreated_08', async () => {
    const createAttempt = vi.fn()
    const entry = createNativeLaunchEntry({
      selectedProfile: () => profile('claude-main', 'claude'),
      createAttempt,
    })

    await expect(entry.recover('missing-request')).rejects.toThrow('LAUNCH_ATTEMPT_NOT_FOUND')
    expect(createAttempt).not.toHaveBeenCalled()
  })

  it('D23_Launch_SynchronousStartReentryCannotCreateSecondAttempt_09', async () => {
    let entry: ReturnType<typeof createNativeLaunchEntry>
    let reentered: Promise<LaunchStatus> | undefined
    const channel = {} as any
    const start = vi.fn(() => {
      reentered = entry.start(input(), channel)
      return Promise.resolve(running('request-1', 'run-1', 1))
    })
    const createAttempt = vi.fn(() => ({
      start,
      recover: vi.fn(),
      latest: vi.fn(),
    }))
    entry = createNativeLaunchEntry({
      selectedProfile: () => profile('claude-main', 'claude'),
      createAttempt,
    })

    const first = entry.start(input(), channel)
    await expect(first).resolves.toEqual(running('request-1', 'run-1', 1))
    expect(reentered).toBe(first)
    expect(createAttempt).toHaveBeenCalledTimes(1)
    expect(start).toHaveBeenCalledTimes(1)
  })
})
