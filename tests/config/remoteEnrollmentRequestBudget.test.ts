import { readFileSync } from 'node:fs'
import { expect, it } from 'vitest'

it('RemoteShare_FourSupportedControlTargetsFitOnlyEnrollmentBudget_001', () => {
  const runtime = readFileSync('src-tauri/src/cli/native_runtime.rs', 'utf8')
  const command = readFileSync('src-tauri/src/remote_native_observer_enrollment_ipc.rs', 'utf8')
  const target = {
    tunnel: 'a'.repeat(32), phoneId: 'a'.repeat(32),
    phoneRelayPublicKeySec1: '04' + 'a'.repeat(128), phoneEndpointSpki: 'a'.repeat(182),
    endpointBinding: 'a'.repeat(64),
  }
  const runs = Array.from({ length: 4 }, (_, i) => ({
    runId: 'run-' + ('a'.repeat(31)) + i, generation: 1, label: 'OpenCode 10', role: 'Controller',
  }))
  const bytes = Buffer.byteLength(JSON.stringify({ action: 'startSharing', ticket: 'a'.repeat(36), target, runs }))
  expect(bytes).toBeGreaterThan(1024)
  const budget = Number(command.match(/const ENROLLMENT_REQUEST_LIMIT: usize = (\d+);/)?.[1] ?? 1024)
  expect(bytes).toBeLessThanOrEqual(budget)
  expect(budget).toBe(2048)
  expect(command.includes('admit_host_document_with_limit::<Action, _>')).toBe(true)
  expect(runtime.includes('self.admit_host_document_with_limit(webview, headers, body, 1024)')).toBe(true)
})
