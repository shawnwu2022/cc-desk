import type { U64String } from './cli'

export interface OutputFrame {
  runId: string
  generation: number
  streamEpoch: U64String
  offset: U64String
  bytes: number[]
}

export interface OutputAck {
  runId: string
  generation: number
  streamEpoch: U64String
  throughOffset: U64String
}
