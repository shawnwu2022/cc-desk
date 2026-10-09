export type HostInputRoute = 'user-queue' | 'protocol-bypass' | 'legacy-direct'
export type HostInputSource = 'explicit-user' | 'xterm-binary' | 'xterm-data-ambiguous'

export interface ClassifiedHostInput {
  route: HostInputRoute
  source: HostInputSource
  bytes: Uint8Array
}

const encoder = new TextEncoder()

function utf8(value: string): Uint8Array {
  return encoder.encode(value)
}

/**
 * xterm 5.5 public onData does not expose whether a data event came from
 * genuine user input or a terminal-generated response. Keep it ambiguous:
 * never infer "protocol" from escape-sequence contents.
 */
export function classifyXtermData(data: string): ClassifiedHostInput {
  return {
    route: 'legacy-direct',
    source: 'xterm-data-ambiguous',
    bytes: utf8(data),
  }
}

/**
 * xterm documents onBinary as raw 8-bit terminal data (currently legacy mouse
 * reports). Preserve byte semantics and keep it off the ordered user-intent
 * lane so an unresolved clipboard read cannot block a terminal response.
 */
export function classifyXtermBinary(data: string): ClassifiedHostInput {
  const bytes = new Uint8Array(data.length)
  for (let index = 0; index < data.length; index += 1) {
    bytes[index] = data.charCodeAt(index) & 0xff
  }
  return {
    route: 'protocol-bypass',
    source: 'xterm-binary',
    bytes,
  }
}

/**
 * Only callers with an explicit user-action provenance may enter the ordered
 * input queue. D18 owns DOM/shortcut/IME arbitration and will supply those
 * provenances when the native workspace UI adopts this adapter.
 */
export function classifyExplicitUserText(data: string): ClassifiedHostInput {
  return {
    route: 'user-queue',
    source: 'explicit-user',
    bytes: utf8(data),
  }
}
