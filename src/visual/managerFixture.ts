import { reactive } from 'vue'
import wire from '../../tests/fixtures/version-manager-wire.json'
import { parseManagerStatus } from '../manager/contracts'
import type { ManagerPhase } from '../types/versionManager'
import { invoke as blocked } from './tauriStub'

export const MANAGER_FIXTURE_PHASES: readonly ManagerPhase[] = ['preparing', 'installing', 'installed-unconfirmed',
  'historical-active', 'returning', 'restored', 'pre-context-aborted', 'recovery-required']
const fixtures = {
  preparing: wire.preparing, installing: wire.installing, 'installed-unconfirmed': wire.installedUnconfirmed,
  'historical-active': wire.historicalActive, returning: wire.returning, restored: wire.restored,
  'pre-context-aborted': wire.preContextAborted, 'recovery-required': wire.recoveryRequired,
}

/** Explicit visual-only in-memory transport. It can never reach native IPC. */
export function installManagerFixture() {
  if (location.pathname !== '/__visual__/version-manager/') throw new Error('VISUAL_FIXTURE_DISABLED')
  if (Object.prototype.hasOwnProperty.call(window, '__CC_DESK_VERSION_MANAGER__')) throw new Error('VISUAL_DOCUMENT_ALREADY_PRESENT')
  if (['__TAURI_INTERNALS__', '__CC_DESK_DOCUMENT__'].some(name => Object.prototype.hasOwnProperty.call(window, name))) {
    throw new Error('VISUAL_NATIVE_CONTEXT_PRESENT')
  }
  const query = new URLSearchParams(location.search)
  const phase = query.get('phase') ?? 'installed-unconfirmed'
  const outcome = query.get('outcome') ?? 'success'
  const actions = query.get('actions') ?? 'backend'
  if (!MANAGER_FIXTURE_PHASES.includes(phase as ManagerPhase) || !['success', 'unknown'].includes(outcome)
    || !['backend', 'refresh-only'].includes(actions)) throw new Error('VISUAL_MANAGER_SCENARIO_INVALID')
  let status = parseManagerStatus({ ...fixtures[phase as ManagerPhase], ...(actions === 'refresh-only' ? { allowedActions: ['refresh'] } : {}) })
  const calls = reactive({ inspects: 0, confirms: 0, returns: 0 })
  let finishReturnOnInspect = false
  const bridge = Object.freeze({
    async invoke(command: string, payload: unknown) {
      const matches = (expected: unknown) => JSON.stringify(payload) === JSON.stringify(expected)
      if (command === 'inspect_version_switch' && matches({})) {
        ++calls.inspects
        if (finishReturnOnInspect) {
          finishReturnOnInspect = false
          status = parseManagerStatus({ ...fixtures.restored, generation: String(BigInt(status.generation) + BigInt(1)) })
        }
        return status
      }
      const action = command === 'confirm_historical_version' ? 'confirm-historical-version'
        : command === 'restore_previous_version' ? 'return-to-previous' : null
      if (!action || !status.allowedActions.includes(action) || !matches({ expectedGeneration: status.generation })) return blocked()
      if (action === 'confirm-historical-version') ++calls.confirms
      else ++calls.returns
      if (outcome === 'unknown') throw { code: 'VISUAL_MANAGER_OUTCOME_UNKNOWN', retryable: false }
      status = parseManagerStatus({ ...fixtures[action === 'confirm-historical-version' ? 'historical-active' : 'returning'],
        generation: String(BigInt(status.generation) + BigInt(1)) })
      finishReturnOnInspect = action === 'return-to-previous'
      return status
    },
  })
  Object.defineProperty(window, '__CC_DESK_VERSION_MANAGER__', { value: bridge, configurable: true })
  return { calls, dispose() { if (window.__CC_DESK_VERSION_MANAGER__ === bridge) delete window.__CC_DESK_VERSION_MANAGER__ } }
}
