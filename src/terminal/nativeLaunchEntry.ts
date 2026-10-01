import { createCliLaunchAttempt } from '@/api/tauri'
import type { LaunchAttempt, LaunchStatus } from '@/api/cliLaunchAttempt'
import type { LaunchAction, LaunchRequest, NativeCliKind } from '@/types/cli'
import type { OutputFrame } from '@/types/terminal'
import type { CliProfile } from '@/types/profile'
import { validateLaunchRequest } from '@/utils/nativeIdentity'
import type { Channel } from '@tauri-apps/api/core'

export interface NativeLaunchEntryInput {
  requestId: string
  tabId: string
  runId: string
  generation: number
  cli: NativeCliKind
  launchCwd: string
  action: LaunchAction
  extraArgs: string[]
  cols: number
  rows: number
}

export interface NativeLaunchEntryOptions {
  selectedProfile(cli: NativeCliKind): CliProfile | null
  createAttempt?: (
    request: LaunchRequest,
    channel: Channel<OutputFrame>,
  ) => LaunchAttempt
}

export interface NativeLaunchEntry {
  start(input: NativeLaunchEntryInput, channel: Channel<OutputFrame>): Promise<LaunchStatus>
  recover(requestId: string): Promise<LaunchStatus>
  latest(requestId: string): LaunchStatus | undefined
}

interface OwnedAttempt {
  fingerprint: string
  channel: Channel<OutputFrame>
  attempt: LaunchAttempt | null
  startPromise: Promise<LaunchStatus>
}

function requestFingerprint(request: LaunchRequest): string {
  return JSON.stringify(request)
}

function asPromise<T>(fn: () => Promise<T>): Promise<T> {
  try {
    return fn()
  } catch (error) {
    return Promise.reject(error)
  }
}

export function createNativeLaunchEntry(options: NativeLaunchEntryOptions): NativeLaunchEntry {
  const attempts = new Map<string, OwnedAttempt>()
  const makeAttempt = options.createAttempt ?? createCliLaunchAttempt

  function build(input: NativeLaunchEntryInput): LaunchRequest {
    if (input.cli !== 'claude' && input.cli !== 'codex') {
      throw new Error('NATIVE_CLI_REQUIRED')
    }

    const selected = options.selectedProfile(input.cli)
    if (!selected) throw new Error('CLI_PROFILE_REQUIRED')
    if (selected.cli !== input.cli) throw new Error('PROFILE_CLI_MISMATCH')

    return validateLaunchRequest({
      ...input,
      // Validation copies each action/argv field, including reactive store values.
      profileId: selected.id,
      expectedProfileRevision: selected.revision,
    })
  }

  return Object.freeze({
    start(input: NativeLaunchEntryInput, channel: Channel<OutputFrame>): Promise<LaunchStatus> {
      let request: LaunchRequest
      try {
        request = build(input)
      } catch (error) {
        return Promise.reject(error)
      }

      const fingerprint = requestFingerprint(request)
      const existing = attempts.get(request.requestId)
      if (existing) {
        if (existing.fingerprint !== fingerprint || existing.channel !== channel) {
          return Promise.reject(new Error('LAUNCH_REQUEST_ID_CONFLICT'))
        }
        return existing.startPromise
      }

      let resolveStart!: (value: LaunchStatus | PromiseLike<LaunchStatus>) => void
      let rejectStart!: (reason?: unknown) => void
      const startPromise = new Promise<LaunchStatus>((resolve, reject) => {
        resolveStart = resolve
        rejectStart = reject
      })
      const owned: OwnedAttempt = {
        fingerprint,
        channel,
        attempt: null,
        startPromise,
      }
      // Reserve before constructing or starting the attempt. Synchronous re-entry
      // for the same request must observe this exact promise instead of spawning.
      attempts.set(request.requestId, owned)

      let attempt: LaunchAttempt
      try {
        attempt = makeAttempt(request, channel)
        owned.attempt = attempt
      } catch (error) {
        if (attempts.get(request.requestId) === owned) attempts.delete(request.requestId)
        rejectStart(error)
        return startPromise
      }

      asPromise(() => attempt.start()).then(resolveStart, rejectStart)
      return startPromise
    },

    recover(requestId: string): Promise<LaunchStatus> {
      const owned = attempts.get(requestId)
      if (!owned) return Promise.reject(new Error('LAUNCH_ATTEMPT_NOT_FOUND'))
      if (!owned.attempt) return Promise.reject(new Error('LAUNCH_ATTEMPT_NOT_READY'))
      return asPromise(() => owned.attempt!.recover())
    },

    latest(requestId: string): LaunchStatus | undefined {
      return attempts.get(requestId)?.attempt?.latest()
    },
  })
}
