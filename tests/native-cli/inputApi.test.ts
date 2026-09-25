import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import * as api from '@/api/tauri'
import type { InputWriteReceipt } from '@/types/terminal'

const bridgeKey = '__CC_DESK_DOCUMENT__'

function bridge(invoke: (...args: any[]) => Promise<unknown>) {
  Object.defineProperty(window, bridgeKey, {
    configurable: true,
    value: { invoke, instanceId: 'backend-d17-api' },
  })
}

beforeEach(() => mockIPC(() => { throw new Error('unguarded invoke') }))
afterEach(() => {
  clearMocks()
  delete (window as any)[bridgeKey]
})

it('D17_Api_InputStagesIn64KiBChunksAndCommitsOnce_001', async () => {
  const receipt: InputWriteReceipt = {
    runId: 'run-a',
    generation: 2,
    inputSeq: '9',
    modeEpoch: '3',
    state: 'host-written',
    confirmedBytes: String(api.NATIVE_INPUT_UPLOAD_CHUNK_BYTES + 7),
  }
  const invoke = vi.fn(async (command: string) => {
    if (command === 'cli_input_commit') return receipt
    return undefined
  })
  bridge(invoke)

  const bytes = Uint8Array.from(
    { length: api.NATIVE_INPUT_UPLOAD_CHUNK_BYTES + 7 },
    (_, index) => index % 251,
  )
  const result = await api.cliWriteInput({
    runId: 'run-a',
    generation: 2,
    inputSeq: '9',
    modeEpoch: '3',
    bytes,
  })

  expect(result).toEqual(receipt)
  expect(invoke.mock.calls.map(call => call[0])).toEqual([
    'cli_input_begin',
    'cli_input_chunk',
    'cli_input_chunk',
    'cli_input_commit',
  ])
  expect(invoke.mock.calls[0][1]).toEqual({
    runId: 'run-a',
    generation: 2,
    inputSeq: '9',
    modeEpoch: '3',
    totalBytes: String(bytes.byteLength),
  })
  expect(invoke.mock.calls[1][1].offset).toBe('0')
  expect(invoke.mock.calls[1][1].bytes).toHaveLength(api.NATIVE_INPUT_UPLOAD_CHUNK_BYTES)
  expect(invoke.mock.calls[2][1]).toMatchObject({
    runId: 'run-a',
    generation: 2,
    inputSeq: '9',
    offset: String(api.NATIVE_INPUT_UPLOAD_CHUNK_BYTES),
  })
  expect(invoke.mock.calls[2][1].bytes).toHaveLength(7)
})

it('D17_Api_StagingFailureAbortsWithoutRetryOrCommit_002', async () => {
  const failure = { code: 'WIRE_FAILED' }
  const invoke = vi.fn(async (command: string) => {
    if (command === 'cli_input_chunk') throw failure
    return undefined
  })
  bridge(invoke)

  await expect(api.cliWriteInput({
    runId: 'run-a',
    generation: 1,
    inputSeq: '1',
    modeEpoch: '1',
    bytes: Uint8Array.from([1, 2, 3]),
  })).rejects.toBe(failure)

  expect(invoke.mock.calls.map(call => call[0])).toEqual([
    'cli_input_begin',
    'cli_input_chunk',
    'cli_input_abort',
  ])
})

it('D17_Api_PartialReceiptIsReturnedWithoutAutomaticReplay_003', async () => {
  const receipt: InputWriteReceipt = {
    runId: 'run-a',
    generation: 1,
    inputSeq: '2',
    modeEpoch: '4',
    state: 'partial-or-unknown',
    confirmedBytes: '2',
  }
  const invoke = vi.fn(async (command: string) => {
    if (command === 'cli_input_commit') return receipt
    return undefined
  })
  bridge(invoke)

  const result = await api.cliWriteInput({
    runId: 'run-a',
    generation: 1,
    inputSeq: '2',
    modeEpoch: '4',
    bytes: Uint8Array.from([1, 2, 3, 4]),
  })

  expect(result).toEqual(receipt)
  expect(invoke.mock.calls.filter(call => call[0] === 'cli_input_commit')).toHaveLength(1)
  expect(invoke.mock.calls.filter(call => call[0] === 'cli_input_abort')).toHaveLength(0)
})

it('D17_Api_ProtocolUsesAuthenticatedDirectWriterCommand_004', async () => {
  const invoke = vi.fn(async () => ({
    state: 'host-written',
    confirmedBytes: '4',
  }))
  bridge(invoke)

  const result = await api.cliWriteProtocol(
    { runId: 'run-a', generation: 2 },
    Uint8Array.from([0, 127, 128, 255]),
  )

  expect(result).toEqual({ state: 'host-written', confirmedBytes: '4' })
  expect(invoke.mock.calls).toEqual([[
    'cli_input_protocol',
    {
      runId: 'run-a',
      generation: 2,
      bytes: [0, 127, 128, 255],
    },
  ]])
})

it('D17_Api_InputAndProtocolNeverFallBackWithoutDocumentAuthority_005', async () => {
  await expect(api.cliWriteInput({
    runId: 'run-a',
    generation: 1,
    inputSeq: '1',
    modeEpoch: '1',
    bytes: Uint8Array.from([1]),
  })).rejects.toMatchObject({ code: 'DOCUMENT_BRIDGE_UNAVAILABLE' })

  await expect(api.cliWriteProtocol(
    { runId: 'run-a', generation: 1 },
    Uint8Array.from([1]),
  )).rejects.toMatchObject({ code: 'DOCUMENT_BRIDGE_UNAVAILABLE' })
})
