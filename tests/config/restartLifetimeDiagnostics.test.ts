import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const token = readFileSync('src-tauri/src/tests/version_history_payload/token.rs', 'utf8')

// 源码边界契约不替代实际 Windows 进程、Job API 与恢复权限验收。
describe('Restart lifetime failure diagnostics', () => {
  it('WorkerTerminalDiagnostic_FailureOnly_001 retains the original rejection after restart-only observation', () => {
    const failure = token.match(/if\s+code\s*!=\s*0\s*\|\|\s*accounting\.ActiveProcesses\s*!=\s*0\s*\|\|\s*drain\.is_err\(\)\s*\{/)
    expect(failure).not.toBeNull()
    const branch = token.slice(failure!.index)
    expect(branch).toContain('HISTORY_CONFINED_WORKER_TERMINAL')
    expect(branch).toMatch(/if matches!\(&scope, WorkerScope::RestartLifetime \{ \.\. \}\)\s*\{\s*observe_worker_terminal_failure\(/)
    const observation = branch.indexOf('observe_worker_terminal_failure(')
    const rejection = branch.indexOf('return Err(drain')
    expect(observation).toBeGreaterThan(0)
    expect(rejection).toBeGreaterThan(observation)
    expect(branch.slice(observation, rejection)).not.toContain('Ok(')
    expect(branch.slice(rejection)).toMatch(/return Err\(drain\s*\.err\(\)\s*\.unwrap_or_else\(\|\| blocked\("worker failed or owned job not empty"\)\)\);/)
  })

  it('WorkerTerminalDiagnostic_ReadOnly_002 only emits fixed fields and makes nonblocking queries', () => {
    const start = token.indexOf('fn observe_worker_terminal_failure(')
    expect(start).toBeGreaterThan(0)
    const adapter = token.slice(start, token.indexOf('pub(super) fn verify_worker(', start))
    expect(adapter).toContain('WaitForSingleObject(raw(process), 0)')
    expect(adapter).toContain('JobObjectBasicProcessIdList')
    expect(adapter).toContain('[usize; 8]')
    const emitted = adapter.slice(adapter.indexOf('eprintln!('))
    const keys = [...emitted.matchAll(/"([A-Za-z][A-Za-z0-9]*)"\s*:/g)].map(match => match[1])
    expect(keys).toEqual(['schema', 'sample', 'elapsedMs', 'activeProcesses', 'workerTerminal', 'memberListComplete', 'workerListed', 'otherMemberCount', 'snapshotAtomic'])
    expect(adapter).not.toMatch(/TerminateProcess|TerminateJobObject|AssignProcessToJobObject|SetInformationJobObject|OpenProcess\(|drop\(/)
  })
})
