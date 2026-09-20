/** Bounded diagnostic metadata. Disabled in ordinary builds; never logs text. */
export interface PasteTicket {
  readonly ptyId: string
  readonly pasteId: string
  readonly started: number
}

export interface InputTrace {
  pasteId: string
  seq: number
  bytes: number
  clipboard: boolean
  ageMs: number
  // Diagnostic builds only: compared in Rust memory, never formatted or logged.
  expected?: string
}

export class PasteTrace {
  #enabled: boolean
  #started: number | undefined
  #seq = 0
  #contexts = new Map<string, PasteTicket>()
  #scope: { ticket: PasteTicket; expected: string } | undefined

  constructor(
    enabled = false,
    private readonly now: () => number = Date.now,
    private readonly uuid: () => string = () => crypto.randomUUID(),
  ) {
    this.#enabled = enabled
  }

  setEnabled(enabled: boolean): void {
    this.#enabled = enabled
    if (!enabled) this.#contexts.clear()
  }

  #allowed(): boolean {
    if (!this.#enabled || this.#seq >= 256) return false
    if (this.#started !== undefined && this.now() - this.#started >= 60_000) {
      this.#contexts.clear()
      return false
    }
    return true
  }

  begin(ptyId: string | undefined): PasteTicket | undefined {
    if (!ptyId || !this.#allowed()) return undefined
    // A fixed cap prevents unbounded maps if many tabs are opened during tracing.
    if (!this.#contexts.has(ptyId) && this.#contexts.size >= 32) return undefined
    this.#started ??= this.now()
    const ticket = { ptyId, pasteId: this.uuid(), started: this.now() }
    this.#contexts.set(ptyId, ticket)
    return ticket
  }

  observe<T>(ticket: PasteTicket | undefined, ptyId: string, expected: string, send: () => T): T {
    if (!ticket || ticket.ptyId !== ptyId || !this.#allowed()) return send()
    const previous = this.#scope
    this.#scope = { ticket, expected }
    try {
      // No await: the actual IPC dispatch sees the reference synchronously.
      // Preserve promise identity, errors and send count; never retry a paste.
      return send()
    } finally {
      this.#scope = previous
    }
  }

  input(ptyId: string, data: string): InputTrace | undefined {
    if (!this.#allowed()) return undefined
    const scope = this.#scope?.ticket.ptyId === ptyId ? this.#scope : undefined
    const ticket = scope?.ticket ?? this.#contexts.get(ptyId)
    if (!ticket) return undefined
    const result: InputTrace = {
      pasteId: ticket.pasteId,
      seq: ++this.#seq,
      bytes: new TextEncoder().encode(data).length,
      clipboard: !!scope,
      ageMs: Math.max(0, this.now() - ticket.started),
    }
    if (scope) result.expected = scope.expected
    return result
  }
}

export const pasteTrace = new PasteTrace()
