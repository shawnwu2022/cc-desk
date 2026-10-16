import { describe, expect, it } from 'vitest'
import { readObserverInstallation } from '@/api/observer'

const owner = { requestId: 'request', runId: 'run', generation: 1 }
const result = { requestId: 'request', run: { runId: 'run', generation: 1 }, state: 'connecting', reasonCode: 'OBSERVER_CONNECTING' }
describe('Observer installation transport', () => {
  it('Observer_ExactInstallationAndSafeReasons_001', async () => {
    for (const reasonCode of ['OBSERVER_DISABLED','OBSERVER_UNSUPPORTED','OBSERVER_CONNECTING','OBSERVER_SERVICE_UNAVAILABLE','OBSERVER_PLUGIN_UNAVAILABLE','OBSERVER_PREPARE_FAILED']) {
      const state = reasonCode === 'OBSERVER_CONNECTING' ? 'connecting' : ['OBSERVER_DISABLED','OBSERVER_UNSUPPORTED'].includes(reasonCode) ? 'off' : 'unavailable'
      const actual = await readObserverInstallation({ invoke: async () => ({...result,state,reasonCode}) }, owner)
      expect(actual).toMatchObject({state,reasonCode})
    }
  })
  it('Observer_RejectsForeignAndUnboundedInstallation_002', async () => {
    for (const patch of [{requestId:'foreign'}, {run:{runId:'run',generation:2}}, {reasonCode:'raw private error'}, {environment:{SECRET:'never'}}, {state:'active'}, {state:'off'}]) {
      await expect(readObserverInstallation({invoke:async()=>({...result,...patch})},owner)).rejects.toThrow('INVALID_OBSERVER_STATUS')
    }
  })
})
