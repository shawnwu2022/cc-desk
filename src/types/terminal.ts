import type { CliKind, U64String } from './cli'

export interface RunKey {
  runId: string
  generation: number
}

export type ProcessState = 'starting' | 'running' | 'exited' | 'failed'
export type OutputState = 'open' | 'draining' | 'drained' | 'incomplete' | 'degraded'
export type ActivityState = 'unknown' | 'working' | 'waiting'
export type ObservationState = 'off' | 'connecting' | 'active' | 'unavailable'

export interface OutputFrame extends RunKey {
  streamEpoch: U64String
  offset: U64String
  bytes: number[]
}

export interface OutputAck extends RunKey {
  streamEpoch: U64String
  throughOffset: U64String
}

export interface RunState extends RunKey {
  cli: CliKind
  process: ProcessState
  output: OutputState
  activity: ActivityState
  observation: ObservationState
}


export type InputWriteState = 'host-written' | 'partial-or-unknown'

export interface NativeInputFrame extends RunKey {
  inputSeq: U64String
  modeEpoch: U64String
  bytes: Uint8Array
}

export interface InputWriteReceipt extends RunKey {
  inputSeq: U64String
  modeEpoch: U64String
  state: InputWriteState
  confirmedBytes: U64String
}

export interface ProtocolWriteReceipt {
  state: InputWriteState
  confirmedBytes: U64String
}
