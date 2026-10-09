import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

describe('D28 native terminal async attempt isolation', () => {
  it('D28_Terminal_AsyncCompletionsAreGuardedByExactAttempt_01', () => {
    const terminal = readFileSync('src/components/NativeCliTerminal.vue', 'utf8')
    const guards = terminal.match(/if \(!attemptIsCurrent\(attempt\)\) return/g) ?? []

    expect(terminal).toContain('captureNativeAttempt')
    expect(terminal).toContain('matchesNativeAttempt')
    expect(guards.length).toBeGreaterThanOrEqual(4)
    const receiptHelper = terminal.slice(terminal.indexOf('function applyReceipt('), terminal.indexOf('async function recover('))
    expect(receiptHelper).toContain('if (!attemptIsCurrent(attempt) || !tabs.applyLaunchStatus(props.tabId, result)) return false')
    expect(terminal.match(/tabs\.applyLaunchStatus\(props\.tabId, result\)/g)).toHaveLength(1)
    expect(terminal).toContain('if (!applyReceipt(attempt, result)) return')
    expect(terminal).toContain("if (!applyReceipt(attempt, result)) throw new Error('NATIVE_STOP_UNCONFIRMED')")
    expect(terminal).toContain('markInputFailure(attempt)')
  })

  it('D28_Tabs_AttemptIdentityIncludesRequestRunAndGeneration_02', () => {
    const store = readFileSync('src/stores/nativeTabs.ts', 'utf8')

    expect(store).toContain('tab.requestId === attempt.requestId')
    expect(store).toContain('tab.runId === attempt.runId')
    expect(store).toContain('tab.generation === attempt.generation')
  })
})
