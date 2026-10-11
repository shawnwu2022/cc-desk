use super::*;
use crate::cli::types::{CliKind, LaunchAction};

fn caller(registry: &RunRegistry<usize>) -> CallerIdentity {
    registry.activate_window("main").unwrap()
}

fn request(id: &str, tab: &str, run: &str, generation: u32) -> LaunchRequest {
    LaunchRequest {
        request_id: id.into(),
        tab_id: tab.into(),
        run_id: run.into(),
        generation,
        profile_id: "profile-test".into(),
        expected_profile_revision: WireU64::parse("1").unwrap(),
        cli: CliKind::Codex,
        launch_cwd: "cwd".into(),
        action: LaunchAction::Raw {
            argv: vec!["private-argv".into()],
        },
        extra_args: vec![],
        cols: 80,
        rows: 24,
    }
}

fn running(
    registry: &RunRegistry<usize>,
    caller: &CallerIdentity,
    request: &LaunchRequest,
) -> LaunchStatus {
    match registry.reserve(caller, request).unwrap() {
        Reservation::New(mut ticket) => {
            assert!(ticket.begin());
            ticket.complete(1usize)
        }
        Reservation::Existing(status) => status,
    }
}

fn retired(
    registry: &RunRegistry<usize>,
    caller: &CallerIdentity,
    request: &LaunchRequest,
) -> LaunchStatus {
    let status = running(registry, caller, request);
    registry.mark_exited(&status.run).unwrap();
    registry.retire(&status.run).unwrap();
    registry
        .complete_retirement(&status.run, &status.request_id)
        .unwrap();
    status
}

// 容量满时按最老优先淘汰非活跃记录，新 launch 注册成功。
#[test]
fn RegistryCapacity_EvictsOldestRetiredForNewLaunch_001() {
    let registry = RunRegistry::new(2);
    let caller = caller(&registry);
    retired(
        &registry,
        &caller,
        &request("request-1", "tab-1", "run-1", 1),
    );
    running(
        &registry,
        &caller,
        &request("request-2", "tab-2", "run-2", 1),
    );

    let third = running(
        &registry,
        &caller,
        &request("request-3", "tab-1", "run-3", 2),
    );
    assert_eq!(third.run.run_id, "run-3");
    assert_eq!(
        registry.status(&caller, "request-1").unwrap_err().code,
        "LAUNCH_NOT_FOUND"
    );
    assert!(registry.status(&caller, "request-2").is_ok());
    assert!(registry.status(&caller, "request-3").is_ok());
}

// 活跃记录即使最老也绝不淘汰；满员且无可淘汰记录时仍然拒绝。
#[test]
fn RegistryCapacity_NeverEvictsActiveRun_002() {
    let registry = RunRegistry::new(2);
    let caller = caller(&registry);
    running(
        &registry,
        &caller,
        &request("request-1", "tab-1", "run-1", 1),
    );
    retired(
        &registry,
        &caller,
        &request("request-2", "tab-2", "run-2", 1),
    );

    running(
        &registry,
        &caller,
        &request("request-3", "tab-3", "run-3", 1),
    );
    assert!(registry.status(&caller, "request-1").is_ok());
    assert_eq!(
        registry.status(&caller, "request-2").unwrap_err().code,
        "LAUNCH_NOT_FOUND"
    );
    let full = match registry.reserve(&caller, &request("request-4", "tab-4", "run-4", 1)) {
        Err(failure) => failure,
        Ok(_) => panic!("full registry must reject without an evictable record"),
    };
    assert_eq!(full.code, "REGISTRY_CAPACITY");
}

// 失败/取消墓碑同样可淘汰；淘汰后同 tab 的代次下限保持单调。
#[test]
fn RegistryCapacity_EvictedTombstoneKeepsGenerationFloor_003() {
    let registry = RunRegistry::new(3);
    let caller = caller(&registry);
    retired(
        &registry,
        &caller,
        &request("request-1", "tab-1", "run-1", 5),
    );
    retired(
        &registry,
        &caller,
        &request("request-2", "tab-2", "run-2", 1),
    );
    running(
        &registry,
        &caller,
        &request("request-3", "tab-3", "run-3", 1),
    );

    let stale = match registry.reserve(&caller, &request("request-4", "tab-1", "run-4", 5)) {
        Err(failure) => failure,
        Ok(_) => panic!("evicted tombstone generation floor must reject stale reuse"),
    };
    assert_eq!(stale.code, "STALE_GENERATION");
    // 只淘汰了一个名额，request-2 不应被顺带清除。
    assert!(registry.status(&caller, "request-2").is_ok());
    let fresh = running(
        &registry,
        &caller,
        &request("request-4", "tab-1", "run-4", 6),
    );
    assert_eq!(fresh.run.generation, 6);
}

// 未退休文档的代次下限不能遗忘；预算满后拒绝新 tab，但保留已有 tab 重启资格。
#[test]
fn RegistryCapacity_GenerationFloorsAreBoundedWithoutReplay_004() {
    let registry = RunRegistry::new(2);
    let caller = caller(&registry);
    for index in 0..4 {
        retired(
            &registry,
            &caller,
            &request(
                &format!("request-{index}"),
                &format!("tab-{index}"),
                &format!("run-{index}"),
                1,
            ),
        );
    }
    assert!(
        matches!(registry.reserve(&caller, &request("overflow", "tab-new", "run-new", 1)), Err(failure) if failure.code == "REGISTRY_CAPACITY")
    );
    assert_eq!(registry.state.lock().tab_generations.len(), 4);
    assert!(
        matches!(registry.reserve(&caller, &request("stale", "tab-0", "run-stale", 1)), Err(failure) if failure.code == "STALE_GENERATION")
    );
    let fresh = running(
        &registry,
        &caller,
        &request("fresh", "tab-0", "run-fresh", 2),
    );
    assert_eq!(fresh.run.generation, 2);
    assert_eq!(registry.state.lock().tab_generations.len(), 4);
}

// 新 document 永久撤销旧 epoch，才能释放旧代次下限；旧运行仍由后台保有。
#[test]
fn RegistryCapacity_NewDocumentReleasesOnlyOldFloors_005() {
    let registry = RunRegistry::new(2);
    let old = caller(&registry);
    let retained = running(&registry, &old, &request("old", "same-tab", "old-run", 5));
    let current = caller(&registry);
    assert!(registry.state.lock().tab_generations.is_empty());
    assert_eq!(registry.check_caller(&old).unwrap_err().code, "FORBIDDEN");
    assert!(registry.retained_resource(&retained.run).is_ok());
    let fresh = running(
        &registry,
        &current,
        &request("new", "same-tab", "new-run", 1),
    );
    assert_eq!(fresh.run.generation, 1);
    assert!(registry.retained_resource(&retained.run).is_ok());
}

// 进程已 reap 但 supervisor 尚在 drain 时，容量压力不能释放其精确 receipt。
#[test]
fn RegistryCapacity_RetiredRunRemainsPinnedUntilSupervisorConverges_006() {
    let registry = RunRegistry::new(1);
    let caller = caller(&registry);
    let first = running(&registry, &caller, &request("first", "tab", "run", 1));
    registry.mark_exited(&first.run).unwrap();
    registry.retire(&first.run).unwrap();

    assert!(matches!(
        registry.reserve(&caller, &request("next", "tab", "new-run", 2)),
        Err(failure) if failure.code == "REGISTRY_CAPACITY"
    ));
    registry.check_run(&caller, &first.run).unwrap();
    assert!(registry.status(&caller, "first").is_ok());
    registry
        .complete_retirement(&first.run, &first.request_id)
        .unwrap();
    let next = running(&registry, &caller, &request("next", "tab", "new-run", 2));
    assert_eq!(next.run.generation, 2);
}

// 同 document 完整收口后复用 RunKey，迟到旧 requestId 仍不能授权控制新 receipt。
#[test]
fn RegistryCapacity_StopAdmissionBindsRequestAfterRunKeyReuse_007() {
    let registry = RunRegistry::new(1);
    let caller = caller(&registry);
    let first = retired(&registry, &caller, &request("first", "tab-old", "run", 1));
    let next = running(&registry, &caller, &request("next", "tab-new", "run", 1));
    assert_eq!(first.run, next.run);
    assert_eq!(
        registry
            .admit_run(&caller, &first.run, "first")
            .unwrap_err()
            .code,
        "REQUEST_CONFLICT"
    );
    assert_eq!(
        registry.admit_run(&caller, &next.run, "next").unwrap(),
        next
    );
    assert_eq!(
        registry
            .complete_retirement(&first.run, "first")
            .unwrap_err()
            .code,
        "REQUEST_CONFLICT"
    );
    assert!(registry.resource(&caller, &next.run).is_ok());
}

// cancel 先到时原始 Ticket 仍须读取取消回执，容量回收不能提前删除它。
#[test]
fn RegistryCapacity_CancelledTicketRemainsPinnedUntilOwnerObserves_008() {
    let registry = RunRegistry::new(1);
    let caller = caller(&registry);
    let first = request("first", "tab", "run", 1);
    let mut ticket = match registry.reserve(&caller, &first).unwrap() {
        Reservation::New(ticket) => ticket,
        Reservation::Existing(_) => panic!("first request must reserve"),
    };
    assert_eq!(
        registry.cancel(&caller, &first).unwrap().phase,
        LaunchPhase::Cancelled
    );
    assert!(matches!(
        registry.reserve(&caller, &request("next", "tab", "new-run", 2)),
        Err(failure) if failure.code == "REGISTRY_CAPACITY"
    ));
    assert!(!ticket.begin());
    let next = running(&registry, &caller, &request("next", "tab", "new-run", 2));
    assert_eq!(next.run.generation, 2);
    assert_eq!(ticket.status().phase, LaunchPhase::Cancelled);
    drop(ticket);
}
