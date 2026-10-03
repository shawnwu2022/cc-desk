// Source boundaries supplement the Windows process/document tests. These do
// not establish that a real retained manager or native recovery UI has run.
import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const runtime = readFileSync('src-tauri/src/version_history/manager_runtime.rs', 'utf8')
const native = readFileSync('src-tauri/src/version_history/windows/reentry.rs', 'utf8')
const host = () => {
  const start = runtime.indexOf('mod reentry_runtime {')
  expect(start).toBeGreaterThan(0)
  return runtime.slice(start)
}

describe('retained version manager read-only entry boundary', () => {
  it('VersionManager_ReentryNeverReplaysInitialChild_001', () => {
    const library = readFileSync('src-tauri/src/lib.rs', 'utf8')
    expect(library).toContain('manager_runtime::run_reentry(None)')
    expect(runtime).toMatch(/pub\(crate\) fn run_reentry\(/)
    expect(host()).toContain('ReenteredManager::open(request)')
    for (const authority of ['InitialManager', 'ManagerWorker', 'publish_ready', 'capture_ready', 'ready_published']) {
      expect(host()).not.toContain(authority)
    }
  })

  it('VersionManager_ReentryRegistersOnlyAuthenticatedInspection_002', () => {
    const commands = host().match(/tauri::generate_handler!\[([^\]]+)\]/)?.[1]
    expect(commands?.split(',').map(value => value.trim()).filter(Boolean)).toEqual(['inspect_version_switch'])
    expect(host()).toContain('manager_document::build_manager(')
    expect(host()).not.toContain('.plugin(')
    expect(host()).not.toContain('pin_initial_handoff')
  })

  it('VersionManager_ReentryRechecksDocumentAroundFreshRead_003', () => {
    const inspect = host().slice(host().indexOf('async fn inspect_version_switch('), host().indexOf('pub(super) fn run('))
    expect(inspect.indexOf('admit_request(')).toBeGreaterThan(0)
    expect(inspect.indexOf('admit_request(')).toBeLessThan(inspect.indexOf('spawn_blocking('))
    const read = inspect.indexOf('owner.lock().inspect()')
    expect(read).toBeGreaterThan(inspect.indexOf('document.check()?'))
    expect(inspect.slice(read)).toMatch(/document\.check\(\)\?;[\s\S]+\.await[\s\S]+document\.check\(\)\?/)
    expect(inspect.indexOf('try_acquire_owned()')).toBeLessThan(inspect.indexOf('spawn_blocking('))
    expect(inspect).toContain('let _permit = permit;')
    expect(host()).toContain('tokio::sync::Semaphore::new(1)')
  })

  it('VersionManager_ReentryCannotMutateDurableTransaction_004', () => {
    expect(native).toContain('RetainedManagerInspection::open(')
    expect(native).toContain('ManagerStatus::project_reentry(')
    for (const authority of ['InitialManager', 'ManagerChildAdmission', 'OwnedJob', 'LiveCoordinator', 'bind_existing(', '.append(', '.reconcile(', '.resume(', 'publish_ready(', 'CreateProcess']) {
      expect(native).not.toContain(authority)
    }
    const projection = readFileSync('src-tauri/src/version_history/manager_types.rs', 'utf8').split('pub(crate) fn project_reentry(')[1].split('fn validate_projection')[0]
    expect(projection).toContain('ManagerPhase::RecoveryRequired')
    expect(projection).toContain('vec![ManagerAction::Refresh]')
    expect(projection).toContain('ManagerBlockReason::RecoveryEvidenceUnavailable')
    expect(projection).not.toContain('ManagerAction::ReturnToPrevious')
    expect(projection).not.toContain('ManagerAction::ConfirmHistoricalVersion')
  })

  it('VersionManager_SharedAdmissionRejectsForeignOrUnboundedRequests_005', () => {
    const admission = runtime.split('fn admit_request<')[1].split('#[tauri::command]')[0]
    expect(admission.indexOf('binding.admit_native(')).toBeLessThan(admission.indexOf('serde_json::from_slice('))
    expect(admission).toContain('InvokeBody::Raw(bytes)')
    expect(admission).toContain('bytes.len() > 1024')
    expect(admission).toContain('transaction != expected_transaction')
    expect(admission).toContain('ManagerDocumentProof::admit(')
  })
})
