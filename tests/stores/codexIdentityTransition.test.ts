import { expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { captureNativeAttempt, useNativeTabsStore } from '@/stores/nativeTabs'
import * as tauri from '@/api/tauri'

// Characterize the candidate policy; this is not a field-event reconstruction.
it('CodexIdentity_SameRunSessionStartCanReplaceIdentity_001', () => {
  setActivePinia(createPinia())
  const store = useNativeTabsStore()
  const tab = store.create({ cli: 'codex', projectId: 'p', projectPath: '/fixture', profileId: 'cx', profileRevision: '14', action: { kind: 'new' } })
  store.applyLaunchStatus(tab.tabId, { instanceId: 'fixture', requestId: tab.requestId,
    run: { runId: tab.runId, generation: tab.generation }, revision: '2', phase: 'running', failure: null })
  const attempt = captureNativeAttempt(tab)
  const first = '01a18afd-4f1f-7df1-98b7-93534352dc84'
  const second = '01a18aff-66df-79f1-ba6b-cba3ff004b65'
  expect(store.applyObservationIdentity(tab.tabId, attempt, first, 'sessionStart')).toBe(true)
  expect(store.applyObservationIdentity(tab.tabId, attempt, second, 'stop')).toBe(false)
  expect(store.tab(tab.tabId)?.observedSessionId).toBe(first)
  expect(store.applyObservationIdentity(tab.tabId, attempt, second, 'sessionStart')).toBe(true)
  expect(store.tab(tab.tabId)?.observedSessionId).toBe(second)
  expect(store.applyObservationIdentity(tab.tabId, attempt, first, 'userPromptSubmit')).toBe(false)
  expect(store.applyObservationIdentity(tab.tabId, { ...attempt, generation: attempt.generation + 1 }, first, 'sessionStart')).toBe(false)
  store.applyLaunchStatus(tab.tabId, { instanceId: 'fixture', requestId: tab.requestId,
    run: { runId: tab.runId, generation: tab.generation }, revision: '3', phase: 'exited', failure: null })
  expect(store.applyObservationIdentity(tab.tabId, attempt, first, 'sessionStart')).toBe(false)
  expect(store.tab(tab.tabId)?.observedSessionId).toBe(second)
})

it('CodexIdentity_DiagnosticsDescribeActualStoreDecisionAndStayBounded_002', () => {
  const output = vi.spyOn(console, 'info').mockImplementation(() => {})
  const transport = vi.spyOn(tauri, 'logMessage').mockResolvedValue(undefined)
  try {
    setActivePinia(createPinia())
    const store = useNativeTabsStore()
    const tab = store.create({ cli: 'codex', projectId: 'p', projectPath: '/PRIVATE', profileId: 'cx', profileRevision: '1', action: { kind: 'new' } })
    store.applyLaunchStatus(tab.tabId, { instanceId: 'fixture', requestId: tab.requestId,
      run: { runId: tab.runId, generation: tab.generation }, revision: '2', phase: 'running', failure: null })
    const owner = captureNativeAttempt(tab)
    const a = '01a18afd-4f1f-7df1-98b7-93534352dc84', b = '01a18aff-66df-79f1-ba6b-cba3ff004b65'
    store.applyObservationIdentity(tab.tabId, owner, a, 'sessionStart')
    store.applyObservationIdentity(tab.tabId, owner, b, 'stop')
    store.applyObservationIdentity(tab.tabId, owner, b, 'sessionStart')
    const records = output.mock.calls.map(call => JSON.parse(String(call[1])))
    expect(records).toMatchObject([
      { layer: 'frontend-identity', outcome: 'adopted', reason: 'first-id', before: null, after: a, event: 'sessionStart', run: tab.runId, generation: 1 },
      { layer: 'frontend-identity', outcome: 'rejected', reason: 'identity-mismatch', before: a, after: a, event: 'stop' },
      { layer: 'frontend-identity', outcome: 'adopted', reason: 'changed-id', before: a, after: b, event: 'sessionStart' },
    ])
    expect(transport.mock.calls).toHaveLength(3)
    const fileRecords = transport.mock.calls.map(call => JSON.parse(call[1]))
    expect(fileRecords[0]).toEqual({ schema: 'codex-identity-v1', run: tab.runId, generation: 1,
      event: 'sessionStart', outcome: 'adopted', reason: 'first-id', before: null, after: a })
    expect(fileRecords.every(record => !('layer' in record))).toBe(true)
    const count = output.mock.calls.length
    store.applyObservationIdentity(tab.tabId, { ...owner, generation: 99 }, a, 'sessionStart')
    store.applyObservationIdentity(tab.tabId, owner, 'PRIVATE_TOKEN', 'sessionStart')
    store.applyObservationIdentity(tab.tabId, owner, a, 'PRIVATE_EVENT')
    expect(output.mock.calls).toHaveLength(count)
    for (let i = 0; i < 100; i++) store.applyObservationIdentity(tab.tabId, owner, i % 2 ? a : b, 'sessionStart')
    expect(output.mock.calls.length).toBeLessThanOrEqual(16)
    expect(JSON.stringify(output.mock.calls)).not.toContain('PRIVATE')
    const beforeRetire = output.mock.calls.length
    store.applyLaunchStatus(tab.tabId, { instanceId: 'fixture', requestId: tab.requestId,
      run: { runId: tab.runId, generation: 1 }, revision: '3', phase: 'exited', failure: null })
    store.applyObservationIdentity(tab.tabId, owner, a, 'sessionStart')
    expect(output.mock.calls).toHaveLength(beforeRetire)
    const claude = store.create({ cli: 'claude', projectId: 'p', projectPath: '/PRIVATE', profileId: 'cx', profileRevision: '1', action: { kind: 'new' } })
    store.applyLaunchStatus(claude.tabId, { instanceId: 'fixture', requestId: claude.requestId,
      run: { runId: claude.runId, generation: claude.generation }, revision: '2', phase: 'running', failure: null })
    store.applyObservationIdentity(claude.tabId, captureNativeAttempt(claude), 'PRIVATE_CLAUDE', 'sessionStart')
    expect(output.mock.calls).toHaveLength(beforeRetire)
  } finally { output.mockRestore(); transport.mockRestore() }
})

it('CodexIdentity_DiagnosticFailureDoesNotChangeAdoption_003', () => {
  const output = vi.spyOn(console, 'info').mockImplementation(() => { throw new Error('fixture logger unavailable') })
  try {
    setActivePinia(createPinia())
    const store = useNativeTabsStore()
    const tab = store.create({ cli: 'codex', projectId: 'p', projectPath: '/fixture', profileId: 'cx', profileRevision: '1', action: { kind: 'new' } })
    store.applyLaunchStatus(tab.tabId, { instanceId: 'fixture', requestId: tab.requestId,
      run: { runId: tab.runId, generation: 1 }, revision: '2', phase: 'running', failure: null })
    const id = '01a18afd-4f1f-7df1-98b7-93534352dc84'
    expect(store.applyObservationIdentity(tab.tabId, captureNativeAttempt(tab), id, 'sessionStart')).toBe(true)
    expect(store.tab(tab.tabId)?.observedSessionId).toBe(id)
  } finally { output.mockRestore() }
})

it('CodexIdentity_DiagnosticsHaveStoreWideLimit_004', () => {
  const output = vi.spyOn(console, 'info').mockImplementation(() => {})
  try {
    setActivePinia(createPinia())
    const store = useNativeTabsStore()
    for (let i = 0; i < 10; i++) {
      const tab = store.create({ cli: 'codex', projectId: 'p', projectPath: '/fixture', profileId: 'cx', profileRevision: '1', action: { kind: 'new' } })
      store.applyLaunchStatus(tab.tabId, { instanceId: 'fixture', requestId: tab.requestId,
        run: { runId: tab.runId, generation: 1 }, revision: '2', phase: 'running', failure: null })
      for (let j = 0; j < 20; j++) {
        expect(store.applyObservationIdentity(tab.tabId, captureNativeAttempt(tab),
          j % 2 ? '01a18afd-4f1f-7df1-98b7-93534352dc84' : '01a18aff-66df-79f1-ba6b-cba3ff004b65', 'sessionStart')).toBe(true)
      }
    }
    expect(output.mock.calls).toHaveLength(128)
  } finally { output.mockRestore() }
})
