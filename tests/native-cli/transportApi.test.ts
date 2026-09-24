import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import * as api from '@/api/tauri'
import type { OutputAck } from '@/types/terminal'

const bridgeKey = '__CC_DESK_DOCUMENT__'

function bridge(invoke: (...args: any[]) => Promise<unknown>) {
  Object.defineProperty(window, bridgeKey, {
    configurable: true,
    value: { invoke, instanceId: 'backend-d14-api' },
  })
}

beforeEach(() => mockIPC(() => { throw new Error('unguarded invoke') }))
afterEach(() => {
  clearMocks()
  delete (window as any)[bridgeKey]
})

it('D14_Api_AckUsesAuthenticatedDocumentBridgeWithoutChannel_001', async () => {
  const ack: OutputAck = {
    runId: 'run-a',
    generation: 2,
    streamEpoch: '9',
    throughOffset: '4',
  }
  const invoke = vi.fn(async () => undefined)
  bridge(invoke)

  await api.cliAckOutput(ack)

  expect(invoke.mock.calls).toEqual([['cli_ack_output', ack]])
})

it('D14_Api_AckNeverFallsBackWithoutDocumentAuthority_002', async () => {
  const ack: OutputAck = {
    runId: 'run-a',
    generation: 2,
    streamEpoch: '9',
    throughOffset: '4',
  }
  await expect(api.cliAckOutput(ack)).rejects.toMatchObject({
    code: 'DOCUMENT_BRIDGE_UNAVAILABLE',
  })
})


it('D15_Api_StopUsesAuthenticatedDocumentBridge_003', async () => {
  const invoke = vi.fn(async () => undefined)
  bridge(invoke)

  await api.cliStop({ runId: 'run-a', generation: 2 })

  expect(invoke.mock.calls).toEqual([['cli_stop', { runId: 'run-a', generation: 2 }]])
})

it('D15_Api_StopNeverFallsBackWithoutDocumentAuthority_004', async () => {
  await expect(api.cliStop({ runId: 'run-a', generation: 2 })).rejects.toMatchObject({
    code: 'DOCUMENT_BRIDGE_UNAVAILABLE',
  })
})
