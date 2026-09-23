import { describe, expect, it, vi } from 'vitest'
import type { LaunchRequest } from '@/types/cli'
import { createLaunchAttempt, type LaunchStatus } from '@/api/cliLaunchAttempt'

function request(): LaunchRequest {
  return {
    requestId: 'request-one', tabId: 'tab-one', runId: 'run-one', generation: 1,
    profileId: 'codex-one', expectedProfileRevision: '1', cli: 'codex',
    launchCwd: '/project', action: { kind: 'raw', argv: ['', '中文', '--future'] },
    extraArgs: [], cols: 80, rows: 24,
  }
}

function receipt(changes: Partial<LaunchStatus> = {}): LaunchStatus {
  return {
    instanceId: 'backend-one', requestId: 'request-one',
    run: { runId: 'run-one', generation: 1 }, revision: '2',
    phase: 'running', failure: null, ...changes,
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

describe('D11 frontend launch attempt', () => {
  it('D11_Retry_OneAttemptSendsOnce_01', async () => {
    const transport = { start: vi.fn(async () => receipt()), status: vi.fn() }
    const attempt = createLaunchAttempt(request(), 'backend-one', transport)
    const first = attempt.start()
    const second = attempt.start()
    // Keep RED controlled even when promise identity is the first failed assertion.
    void first.catch(() => undefined)
    void second.catch(() => undefined)
    expect(second).toBe(first)
    const results = await Promise.all(Array.from({ length: 100 }, () => attempt.start()))
    expect(transport.start).toHaveBeenCalledTimes(1)
    expect(results.every((value) => value.phase === 'running')).toBe(true)
    expect(transport.status).not.toHaveBeenCalled()
  })

  it('D11_Retry_LostStartResponseOnlyQueries_02', async () => {
    const transport = {
      start: vi.fn(async () => { throw new Error('private-transport-payload') }),
      status: vi.fn(async () => receipt()),
    }
    const attempt = createLaunchAttempt(request(), 'backend-one', transport)
    await expect(attempt.start()).rejects.toThrow('LAUNCH_STATE_UNKNOWN')
    expect(await attempt.recover()).toEqual(receipt())
    expect(transport.start).toHaveBeenCalledTimes(1)
    expect(transport.status).toHaveBeenCalledExactlyOnceWith('request-one')
    await expect(attempt.start()).rejects.toThrow('LAUNCH_STATE_UNKNOWN')
    expect(transport.start).toHaveBeenCalledTimes(1)
  })

  it('D11_Retry_MissingQueryNeverCreatesReplacement_03', async () => {
    const transport = {
      start: vi.fn(async () => { throw new Error('lost') }),
      status: vi.fn(async () => { throw new Error('LAUNCH_NOT_FOUND') }),
    }
    const attempt = createLaunchAttempt(request(), 'backend-one', transport)
    await expect(attempt.start()).rejects.toThrow('LAUNCH_STATE_UNKNOWN')
    await expect(attempt.recover()).rejects.toThrow('LAUNCH_STATE_UNKNOWN')
    expect(transport.start).toHaveBeenCalledTimes(1)
    expect(attempt.latest()).toBeUndefined()
  })

  it('D11_Retry_RequestIsOwnedAndFrozen_04', async () => {
    const input = request()
    const transport = {
      start: vi.fn(async (frozen: LaunchRequest) => {
        expect(Object.isFrozen(frozen)).toBe(true)
        expect(Object.isFrozen(frozen.action)).toBe(true)
        expect(Object.isFrozen(frozen.extraArgs)).toBe(true)
        if (frozen.action.kind === 'raw') expect(Object.isFrozen(frozen.action.argv)).toBe(true)
        return receipt()
      }),
      status: vi.fn(),
    }
    const attempt = createLaunchAttempt(input, 'backend-one', transport)
    input.launchCwd = '/changed'
    if (input.action.kind === 'raw') input.action.argv[0] = 'changed'
    await attempt.start()
    expect(transport.start.mock.calls[0][0]).toEqual(request())
  })

  it('D11_Retry_LateStartCannotOverwriteExit_05', async () => {
    const startReply = deferred<unknown>()
    const exit = receipt({ revision: '3', phase: 'exited' })
    const transport = {
      start: vi.fn(() => startReply.promise), status: vi.fn(async () => exit),
    }
    const attempt = createLaunchAttempt(request(), 'backend-one', transport)
    const starting = attempt.start()
    void starting.catch(() => undefined)
    expect(await attempt.recover()).toEqual(exit)
    startReply.resolve(receipt())
    expect(await starting).toEqual(exit)
    expect(attempt.latest()).toEqual(exit)
  })

  it('D11_Retry_RejectsBadReceiptsWithoutMutation_06', async () => {
    const badValues: unknown[] = [
      null,
      receipt({ requestId: 'another' }),
      receipt({ run: { runId: 'another', generation: 1 } }),
      receipt({ run: { runId: 'run-one', generation: 2 } }),
      receipt({ revision: '01' }),
      receipt({ revision: '18446744073709551616' }),
      receipt({ phase: 'failed', failure: null }),
      receipt({ phase: 'running', failure: 'outcome-unknown' }),
      { ...receipt(), revision: 2 },
      { ...receipt(), secret: 'private-field' },
      { ...receipt(), phase: 'invented' },
    ]
    for (const bad of badValues) {
      const transport = { start: vi.fn(async () => bad), status: vi.fn() }
      const attempt = createLaunchAttempt(request(), 'backend-one', transport)
      await expect(attempt.start()).rejects.toThrow('INVALID_LAUNCH_RESPONSE')
      expect(attempt.latest()).toBeUndefined()
      expect(transport.start).toHaveBeenCalledTimes(1)
    }
  })

  it('D11_Retry_BackendRestartDoesNotRebind_07', async () => {
    const transport = {
      start: vi.fn(async () => receipt()),
      status: vi.fn(async () => receipt({ instanceId: 'different-backend' })),
    }
    const attempt = createLaunchAttempt(request(), 'backend-one', transport)
    const started = await attempt.start()
    await expect(attempt.recover()).rejects.toThrow('BACKEND_INSTANCE_CHANGED')
    expect(attempt.latest()).toEqual(started)
    expect(transport.start).toHaveBeenCalledTimes(1)
  })

  it('D11_Retry_QueryBeforeStartIsRejected_08', async () => {
    const transport = { start: vi.fn(), status: vi.fn() }
    const attempt = createLaunchAttempt(request(), 'backend-one', transport)
    await expect(attempt.recover()).rejects.toThrow('LAUNCH_NOT_STARTED')
    expect(transport.start).not.toHaveBeenCalled()
    expect(transport.status).not.toHaveBeenCalled()
  })

  it('D11_Retry_OlderQueryCannotRegressState_09', async () => {
    const transport = {
      start: vi.fn(async () => receipt()),
      status: vi.fn(async () => receipt({ revision: '0', phase: 'reserved' })),
    }
    const attempt = createLaunchAttempt(request(), 'backend-one', transport)
    const started = await attempt.start()
    expect(await attempt.recover()).toEqual(started)
    expect(attempt.latest()).toEqual(started)
  })

  it('D11_Retry_ConflictingEqualVersionIsRejected_10', async () => {
    const transport = {
      start: vi.fn(async () => receipt()),
      status: vi.fn(async () => receipt({ phase: 'exited' })),
    }
    const attempt = createLaunchAttempt(request(), 'backend-one', transport)
    const started = await attempt.start()
    await expect(attempt.recover()).rejects.toThrow('INVALID_LAUNCH_RESPONSE')
    expect(attempt.latest()).toEqual(started)
  })

  it('D11_Retry_NewerRevisionCannotReversePhase_11', async () => {
    const transport = {
      start: vi.fn(async () => receipt()),
      status: vi.fn(async () => receipt({ revision: '9', phase: 'reserved' })),
    }
    const attempt = createLaunchAttempt(request(), 'backend-one', transport)
    await attempt.start()
    await expect(attempt.recover()).rejects.toThrow('INVALID_LAUNCH_RESPONSE')
    expect(attempt.latest()).toEqual(receipt())
  })

  it('D11_Retry_FailureReceiptDoesNotRetry_12', async () => {
    const failed = receipt({ phase: 'failed', failure: 'process-start-failed' })
    const transport = { start: vi.fn(async () => failed), status: vi.fn() }
    const attempt = createLaunchAttempt(request(), 'backend-one', transport)
    expect(await attempt.start()).toEqual(failed)
    expect(await attempt.start()).toEqual(failed)
    expect(transport.start).toHaveBeenCalledTimes(1)
    expect(Object.isFrozen(attempt.latest())).toBe(true)
    expect(Object.isFrozen(attempt.latest()?.run)).toBe(true)
  })
})
