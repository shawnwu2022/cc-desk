#![allow(non_snake_case)]
use super::super::wire::{ProjectionState, ResourceKind};
use super::*;
use crate::cli::{
    native_archive::ArchiveRequest,
    profiles::EnvValue,
    storage::{Patch, WorkspaceRepository},
    types::WireU64,
};

// 续页复用同次观察，不再次扫描或推进 Unknown；新 offset-zero 观察仍按原门禁核对。
#[test]
fn NativeArchive_HistoryContinuationDoesNotRepeatReconciliation_030() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("codex-home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(root.join("archived_sessions")).unwrap();
    std::fs::create_dir(&project).unwrap();
    let repo = WorkspaceRepository::open(temp.path().join("desk/workspace.json")).unwrap();
    let registration = crate::cli::workspace::register_project(&repo, &project).unwrap();
    std::fs::create_dir(root.join("sessions")).unwrap();
    for index in 1..=12 {
        if index == 9 {
            continue;
        }
        let native_id = format!("00000000-0000-4000-8000-{index:012}");
        std::fs::write(root.join(if index == 11 { "sessions" } else { "archived_sessions" }).join(format!("{native_id}.jsonl")),format!("{}\n{}\n",
            serde_json::json!({"type":"session_meta","payload":{"id":native_id,"cwd":if index == 10 { temp.path().join("foreign") } else { project.clone() },"source":"cli"}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"Synthetic pagination fixture"}}))).unwrap();
    }
    let mut profile = Profile::new("fixture", CliKind::Codex);
    profile.env.insert(
        "CODEX_HOME".into(),
        Override::Set(EnvValue::Literal {
            value: root.to_str().unwrap().into(),
            non_secret: true,
        }),
    );
    let document = repo
        .apply(repo.read().unwrap().revision, Patch::Create { profile })
        .unwrap();
    let launch = Arc::new(LaunchService::new(repo.clone(), Some(EnvMap::new()), None));
    let caller = launch.registry().activate_window("main").unwrap();
    let service = ProjectionService::new(launch.clone());
    let source = service
        .scope(
            &caller,
            &ScopeTarget::Profile {
                profile_id: "fixture".into(),
                expected_profile_revision: document.profiles["fixture"].revision,
                project_ids: vec![registration.project_id],
            },
        )
        .unwrap();
    let mut read = ReadRequest {
        source: source.clone(),
        resource_kind: ResourceKind::History,
        request_epoch: WireU64::parse("1").unwrap(),
        query: None,
        session_id: None,
        limit: 1,
        offset: 0,
    };
    let first = service.read(&caller, &read).unwrap();
    assert!(first.has_more);
    for index in 1..=12 {
        let id = format!("00000000-0000-4000-8000-{index:012}");
        let request = ArchiveRequest {
            operation_id: format!("uncertain-fixture-{index}"),
            source: source.clone(),
            session_key: serde_json::json!(["local", "codex", source.source_root_key, id])
                .to_string(),
        };
        let physical = serde_json::to_string(&(source.source_root_key.clone(), id)).unwrap();
        assert!(service
            .archive_operations
            .begin(&caller, &request, &physical)
            .unwrap()
            .is_none());
        if index != 12 {
            service.archive_operations.finish(
                &caller,
                &request,
                Err(error("ARCHIVE_OUTCOME_UNKNOWN")),
            );
        }
    }
    read.offset = 1;
    assert_eq!(
        service.read(&caller, &read).unwrap().state,
        ProjectionState::Ready
    );
    assert_eq!(
        service
            .archive_operations
            .unknown_session_keys(&source.source_root_key)
            .unwrap()
            .len(),
        12
    );
    let before = repo.read_count_for_test();
    read.offset = 0;
    read.request_epoch = WireU64::parse("2").unwrap();
    assert_eq!(
        service.read(&caller, &read).unwrap().state,
        ProjectionState::Ready
    );
    let unresolved = service
        .archive_operations
        .unknown_session_keys(&source.source_root_key)
        .unwrap();
    assert_eq!(unresolved.len(), 4);
    for index in 9..=12 {
        let id = format!("00000000-0000-4000-8000-{index:012}");
        assert!(unresolved.iter().any(|key| key.contains(&id)));
    }
    println!(
        "ARCHIVE_UNKNOWN_WORKSPACE_READS {}",
        repo.read_count_for_test() - before
    );
    assert!(
        repo.read_count_for_test() - before < 20,
        "Unknown reconciliation must share one authorized scan, reads={}",
        repo.read_count_for_test() - before
    );
    drop(service);
    drop(launch);
    drop(repo);
}
