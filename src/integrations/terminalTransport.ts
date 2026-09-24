export interface NativeRunRef {
  runId: string
  generation: number
}

export interface NativeOutputFrame extends NativeRunRef {
  streamEpoch: string
  offset: string
  bytes: number[]
}

export interface NativeOutputAck extends NativeRunRef {
  streamEpoch: string
  throughOffset: string
}

interface TerminalWriter {
  write(data: Uint8Array, callback?: () => void): void
}

export interface TerminalOutputConsumer {
  accept(frame: NativeOutputFrame): void
  dispose(): void
}

export function createTerminalOutputConsumer(
  run: NativeRunRef,
  term: TerminalWriter,
  sendAck: (ack: NativeOutputAck) => Promise<void>,
): TerminalOutputConsumer {
  let closed = false
  return {
    accept(frame) {
      if (closed) throw new Error('OUTPUT_CONSUMER_CLOSED')
      term.write(Uint8Array.from(frame.bytes), () => {
        if (closed) return
        void sendAck({
          ...run,
          streamEpoch: frame.streamEpoch,
          throughOffset: String(Number(frame.offset) + frame.bytes.length),
        })
      })
    },
    dispose() {
      closed = true
    },
  }
}
