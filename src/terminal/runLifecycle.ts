import type { CliKind, U64String } from '@/types/cli'
import type { OutputState, ProcessState, RunState } from '@/types/terminal'

const MAX_U64 = (1n << 64n) - 1n

export type RunLifecycleEvent =
  | { type: 'process-exited'; runId: string; generation: number }
  | { type: 'output-end'; runId: string; generation: number; streamEpoch: U64String; finalOffset: U64String }
  | { type: 'degraded'; runId: string; generation: number; streamEpoch: U64String }
  | { type: 'incomplete'; runId: string; generation: number; streamEpoch: U64String }

export interface RunLifecycleSnapshot extends RunState {
  streamEpoch?: U64String
  finalOffset?: U64String
  parsedThrough: U64String
  sentThrough: U64String
}

export interface RunLifecycle {
  snapshot(): RunLifecycleSnapshot
  accept(event: RunLifecycleEvent): boolean
  processRunning(): void
  processExited(): void
  outputStarted(streamEpoch: U64String): void
  sentThrough(offset: U64String): void
  outputEnd(finalOffset: U64String): void
  parsedThrough(offset: U64String): void
  degraded(): void
  incomplete(): void
}

function parseU64(value: string): bigint {
  if (!/^(0|[1-9][0-9]*)$/.test(value)) throw new Error('INVALID_U64')
  const parsed = BigInt(value)
  if (parsed > MAX_U64) throw new Error('INVALID_U64')
  return parsed
}

export function createRunLifecycle(input: {
  runId: string
  generation: number
  cli: CliKind
}): RunLifecycle {
  let process: ProcessState = 'starting'
  let output: OutputState = 'open'
  let streamEpoch: U64String | undefined
  let sent = 0n
  let parsed = 0n
  let finalOffset: bigint | undefined

  const refresh = () => {
    if (output === 'degraded' || output === 'incomplete') return
    if (finalOffset !== undefined && parsed === finalOffset) output = 'drained'
    else if (finalOffset !== undefined || process === 'exited') output = 'draining'
  }

  const requireEpoch = (): U64String => {
    if (!streamEpoch) throw new Error('OUTPUT_STREAM_NOT_READY')
    return streamEpoch
  }

  const lifecycle: RunLifecycle = {
    snapshot() {
      return {
        runId: input.runId,
        generation: input.generation,
        cli: input.cli,
        process,
        output,
        activity: 'unknown',
        observation: 'off',
        ...(streamEpoch ? { streamEpoch } : {}),
        ...(finalOffset !== undefined ? { finalOffset: finalOffset.toString() as U64String } : {}),
        parsedThrough: parsed.toString() as U64String,
        sentThrough: sent.toString() as U64String,
      }
    },

    accept(event) {
      if (event.runId !== input.runId || event.generation !== input.generation) return false
      if ('streamEpoch' in event && streamEpoch !== event.streamEpoch) return false
      switch (event.type) {
        case 'process-exited':
          lifecycle.processExited()
          return true
        case 'output-end':
          lifecycle.outputEnd(event.finalOffset)
          return true
        case 'degraded':
          lifecycle.degraded()
          return true
        case 'incomplete':
          lifecycle.incomplete()
          return true
      }
    },

    processRunning() {
      if (process !== 'starting' && process !== 'running') throw new Error('RUN_STATE_CONFLICT')
      process = 'running'
    },

    processExited() {
      if (process === 'failed') throw new Error('RUN_STATE_CONFLICT')
      process = 'exited'
      refresh()
    },

    outputStarted(epoch) {
      if (parseU64(epoch) === 0n) throw new Error('INVALID_STREAM_EPOCH')
      if (streamEpoch && streamEpoch !== epoch) throw new Error('STALE_OUTPUT_STREAM')
      streamEpoch = epoch
    },

    sentThrough(value) {
      requireEpoch()
      const next = parseU64(value)
      if (next < sent) throw new Error('OUTPUT_OFFSET_BACKWARD')
      if (finalOffset !== undefined && next !== sent) throw new Error('OUTPUT_ALREADY_ENDED')
      sent = next
    },

    outputEnd(value) {
      requireEpoch()
      const next = parseU64(value)
      if (next > sent) throw new Error('OUTPUT_END_BEYOND_SENT')
      if (finalOffset !== undefined && finalOffset !== next) throw new Error('OUTPUT_END_CONFLICT')
      finalOffset = next
      refresh()
    },

    parsedThrough(value) {
      requireEpoch()
      const next = parseU64(value)
      if (next < parsed) throw new Error('OUTPUT_ACK_BACKWARD')
      if (next > sent) throw new Error('OUTPUT_ACK_BEYOND_SENT')
      parsed = next
      refresh()
    },

    degraded() {
      requireEpoch()
      output = 'degraded'
    },

    incomplete() {
      requireEpoch()
      output = 'incomplete'
    },
  }

  return lifecycle
}
