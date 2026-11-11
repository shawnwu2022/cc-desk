#![allow(non_snake_case)]
use super::*;
use serde_json::json;

fn replies() -> std::collections::VecDeque<serde_json::Value> {
    [json!({"id":1,"result":{"codexHome":"fixture-home"}}),
     json!({"id":2,"result":{"thread":{"id":"fixture","path":"sessions/fixture.jsonl"}}}),
     json!({"id":3,"result":{}}),
     json!({"id":4,"result":{"thread":{"id":"fixture","path":"archived_sessions/fixture.jsonl"}}})].into()
}
#[test]
fn NativeArchive_OfficialProtocolVerifiesRootArchive_001() {
    let mut sent = Vec::new();
    let mut replies = replies();
    exchange(
        "fixture",
        &mut |frame| {
            sent.push(frame);
            Ok(())
        },
        &mut |_| Ok(replies.pop_front().unwrap()),
        &|value| {
            assert_eq!(value["codexHome"], "fixture-home");
            Ok(())
        },
        &|thread, after| {
            assert_eq!(thread["id"], "fixture");
            assert_eq!(
                thread["path"],
                if after {
                    "archived_sessions/fixture.jsonl"
                } else {
                    "sessions/fixture.jsonl"
                }
            );
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        sent.iter()
            .filter_map(|x| x["method"].as_str())
            .collect::<Vec<_>>(),
        [
            "initialize",
            "initialized",
            "thread/read",
            "thread/archive",
            "thread/read"
        ]
    );
    assert_eq!(sent[3]["params"], json!({"threadId":"fixture"}));
}
#[test]
fn NativeArchive_InvalidStartupNeverSendsArchive_002() {
    let mut sent = Vec::new();
    let mut replies = replies();
    let result = exchange(
        "fixture",
        &mut |frame| {
            sent.push(frame);
            Ok(())
        },
        &mut |_| Ok(replies.pop_front().unwrap()),
        &|_| Err(super::super::profiles::error("SCOPE_STALE")),
        &|_, _| Ok(()),
    );
    assert_eq!(result.unwrap_err().code, "SCOPE_STALE");
    assert_eq!(sent.len(), 1);
}
#[test]
fn NativeArchive_SourceMismatchNeverSendsArchive_003() {
    let mut sent = Vec::new();
    let mut replies = replies();
    let result = exchange(
        "fixture",
        &mut |frame| {
            sent.push(frame);
            Ok(())
        },
        &mut |_| Ok(replies.pop_front().unwrap()),
        &|_| Ok(()),
        &|_, _| Err(super::super::profiles::error("SESSION_ORIGIN_AMBIGUOUS")),
    );
    assert_eq!(result.unwrap_err().code, "SESSION_ORIGIN_AMBIGUOUS");
    assert!(!sent.iter().any(|x| x["method"] == "thread/archive"));
}
#[test]
fn NativeArchive_LostAckIsUnknownAndNeverReplayed_004() {
    let mut sent = Vec::new();
    let mut replies = replies();
    replies.truncate(2);
    let result = exchange(
        "fixture",
        &mut |frame| {
            sent.push(frame);
            Ok(())
        },
        &mut |_| {
            replies
                .pop_front()
                .ok_or_else(|| super::super::profiles::error("SOURCE_UNAVAILABLE"))
        },
        &|_| Ok(()),
        &|_, _| Ok(()),
    );
    assert_eq!(result.unwrap_err().code, "ARCHIVE_OUTCOME_UNKNOWN");
    assert_eq!(
        sent.iter()
            .filter(|x| x["method"] == "thread/archive")
            .count(),
        1
    );
}
#[test]
fn NativeArchive_AckWithoutArchivedReadbackNeverConfirms_005() {
    let mut replies = replies();
    let result = exchange(
        "fixture",
        &mut |_| Ok(()),
        &mut |_| Ok(replies.pop_front().unwrap()),
        &|_| Ok(()),
        &|_, after| {
            if after {
                Err(super::super::profiles::error("SOURCE_CHANGED"))
            } else {
                Ok(())
            }
        },
    );
    assert_eq!(result.unwrap_err().code, "ARCHIVE_VERIFICATION_FAILED");
}
#[test]
fn NativeArchive_ScopeAndOperationReplayAreBound_006() {
    use crate::cli::{snapshot::CallerIdentity, types::WireU64};
    let request: ArchiveRequest = serde_json::from_value(json!({"operationId":"archive-1","sessionKey":"session-key","source":{
        "scopeId":"scope", "instanceId":"instance", "cli":"codex", "sourceRootKey":"root", "identityEpoch":"1", "profileId":"profile", "profileRevision":"1", "basis":"configured-profile",
        "target":{"kind":"profile","profileId":"profile","expectedProfileRevision":"1","projectIds":["project"]}}})).unwrap();
    request.validate().unwrap();
    let caller = CallerIdentity {
        instance_id: "instance".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let operations = Operations::default();
    assert!(operations
        .begin(&caller, &request, "physical")
        .unwrap()
        .is_none());
    assert_eq!(
        operations
            .begin(&caller, &request, "physical")
            .unwrap()
            .unwrap()
            .unwrap_err()
            .code,
        "ARCHIVE_OUTCOME_UNKNOWN"
    );
    let mut changed = request.clone();
    changed.session_key = "other".into();
    assert_eq!(
        operations
            .begin(&caller, &changed, "physical")
            .unwrap_err()
            .code,
        "ARCHIVE_OPERATION_CONFLICT"
    );
    operations.finish(&caller, &request, Ok(()));
    assert!(operations
        .begin(&caller, &request, "physical")
        .unwrap()
        .unwrap()
        .is_ok());
    changed.source.cli = crate::cli::types::CliKind::Claude;
    assert_eq!(
        changed.validate().unwrap_err().code,
        "NATIVE_ARCHIVE_UNSUPPORTED"
    );
}

#[test]
fn NativeArchive_UnknownFenceSurvivesNewOperationAndDocument_007() {
    use crate::cli::{snapshot::CallerIdentity, types::WireU64};
    let request: ArchiveRequest = serde_json::from_value(json!({"operationId":"one","sessionKey":"key","source":{"scopeId":"s","instanceId":"i","cli":"codex","sourceRootKey":"r","identityEpoch":"1","profileId":"p","profileRevision":"1","basis":"configured-profile","target":{"kind":"profile","profileId":"p","expectedProfileRevision":"1","projectIds":["project"]}}})).unwrap();
    let caller = CallerIdentity {
        instance_id: "i".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let other = CallerIdentity {
        webview_epoch: WireU64::parse("2").unwrap(),
        ..caller.clone()
    };
    let operations = Operations::default();
    operations.begin(&caller, &request, "physical").unwrap();
    assert!(operations.reconcile_archived("physical").is_err());
    operations.finish(
        &caller,
        &request,
        Err(super::super::profiles::error("ARCHIVE_OUTCOME_UNKNOWN")),
    );
    operations.release_revoked(|_| false).unwrap();
    let fresh = ArchiveRequest {
        operation_id: "two".into(),
        ..request.clone()
    };
    assert_eq!(
        operations
            .begin(&other, &fresh, "physical")
            .err()
            .unwrap()
            .code,
        "ARCHIVE_OUTCOME_UNKNOWN"
    );
    assert!(operations
        .begin(&other, &fresh, "unrelated")
        .unwrap()
        .is_none());
    operations.reconcile_archived("physical").unwrap();
    assert!(operations
        .begin(
            &other,
            &ArchiveRequest {
                operation_id: "three".into(),
                ..request
            },
            "physical"
        )
        .unwrap()
        .is_none());
}

#[test]
#[cfg(windows)]
fn NativeArchive_SyntheticBackendPinsSourceAndWaitsForJob_008() {
    synthetic_archive(false, true);
}
#[test]
#[cfg(windows)]
fn NativeArchive_LostAckReadRestoreArchiveRecovers_009() {
    synthetic_archive(true, true);
}
#[cfg(windows)]
fn synthetic_archive(lost_ack: bool, move_before_ack: bool) {
    use crate::cli::{
        environment::EnvMap,
        launch_service::LaunchService,
        native_projection::{service::ProjectionService, wire::ScopeTarget},
        profiles::{EnvValue, Override, Profile},
        storage::{Patch, WorkspaceRepository},
        types::CliKind,
    };
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("codex-home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(root.join("sessions")).unwrap();
    std::fs::create_dir(&project).unwrap();
    let id = "00000000-0000-4000-8000-000000000008";
    let filename = format!("rollout-2026-11-11T00-00-00-{id}.jsonl");
    let file = root.join("sessions").join(&filename);
    let archived = root.join("archived_sessions").join(&filename);
    let transcript = format!(
        "{}\n",
        json!({"type":"session_meta","payload":{"id":id,"cwd":project.to_str().unwrap(),"timestamp":"2026-11-11T00:00:00Z"}})
    );
    std::fs::write(&file, &transcript).unwrap();
    let unrelated = root.join("unrelated.txt");
    std::fs::write(&unrelated, "unrelated fixture").unwrap();
    let marker = temp.path().join("archive-calls");
    let late = temp.path().join("descendant-exited");
    let descendant = temp.path().join("descendant.js");
    let started = temp.path().join("descendant-started");
    let child_error = temp.path().join("descendant-stderr");
    std::fs::write(&descendant, format!("require('node:fs').writeFileSync({}, 'started');setTimeout(()=>require('node:fs').writeFileSync({}, 'done'),500);", json!(started.to_str().unwrap()), json!(late.to_str().unwrap()))).unwrap();
    let script = format!(
        r#"const fs=require('node:fs'),rl=require('node:readline'),cp=require('node:child_process');let path={file};const archived={archived};
rl.createInterface({{input:process.stdin}}).on('line',line=>{{const m=JSON.parse(line);if(!m.id)return;let result;if(m.method==='initialize')result={{codexHome:{home}}};else if(m.method==='thread/read')result={{thread:{{id:{id},cwd:{cwd},path}}}};else if(m.method==='thread/archive'){{fs.appendFileSync({marker},'archive\n');if({move_before_ack}){{fs.renameSync(path,archived);path=archived;}}if({lost_ack}&&fs.readFileSync({marker},'utf8').trim().split('\n').length===1)process.exit(0);cp.spawn(process.execPath,[{descendant}],{{stdio:['ignore','inherit',fs.openSync({child_error},'w')],windowsHide:true,detached:true}}).unref();const until=Date.now()+5000;while(!fs.existsSync({started})&&Date.now()<until)Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,10);if(!fs.existsSync({started}))process.exit(8);result={{}};}}else process.exit(9);process.stdout.write(JSON.stringify({{id:m.id,result}})+'\n');}}).on('close',()=>process.exit(0));"#,
        lost_ack = lost_ack,
        move_before_ack = move_before_ack,
        file = json!(file.to_str().unwrap()),
        archived = json!(archived.to_str().unwrap()),
        home = json!(root.to_str().unwrap()),
        id = json!(id),
        cwd = json!(project.to_str().unwrap()),
        marker = json!(marker.to_str().unwrap()),
        descendant = json!(descendant.to_str().unwrap()),
        started = json!(started.to_str().unwrap()),
        child_error = json!(child_error.to_str().unwrap())
    );
    std::fs::write(project.join("app-server"), script).unwrap();
    let node = std::process::Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .unwrap();
    assert!(node.status.success());
    let repo = WorkspaceRepository::open(temp.path().join("desk/workspace.json")).unwrap();
    let registered = crate::cli::workspace::register_project(&repo, &project).unwrap();
    let mut profile = Profile::new("fixture", CliKind::Codex);
    profile.program_path = Override::Set(String::from_utf8(node.stdout).unwrap().trim().into());
    profile.default_args = Override::Set(vec![]);
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
    let mut environment = EnvMap::new();
    environment.insert("SystemRoot".into(), std::env::var_os("SystemRoot").unwrap());
    let launch = Arc::new(LaunchService::new(repo.clone(), Some(environment), None));
    let caller = launch.registry().activate_window("main").unwrap();
    let service = Arc::new(ProjectionService::new(launch));
    let source = service
        .scope(
            &caller,
            &ScopeTarget::Profile {
                profile_id: "fixture".into(),
                expected_profile_revision: document.profiles["fixture"].revision,
                project_ids: vec![registered.project_id],
            },
        )
        .unwrap();
    let request = ArchiveRequest {
        operation_id: "archive-fixture".into(),
        session_key: json!(["local", "codex", source.source_root_key, id]).to_string(),
        source,
    };
    let mut forged = request.clone();
    forged.source.profile_id = "other".into();
    assert!(service.archive(&caller, forged).is_err());
    assert!(!marker.exists());
    let worker_service = service.clone();
    let worker_caller = caller.clone();
    let worker_request = request.clone();
    let worker = std::thread::spawn(move || worker_service.archive(&worker_caller, worker_request));
    let until = Instant::now() + Duration::from_secs(10);
    while !marker.exists() && !worker.is_finished() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    if !marker.exists() {
        panic!(
            "synthetic helper failed before archive: {:?}",
            (
                worker.join().unwrap().err().map(|e| e.code),
                std::fs::read_to_string(&child_error)
            )
        );
    }
    let receipt = if lost_ack {
        assert_eq!(
            worker.join().unwrap().err().unwrap().code,
            "ARCHIVE_OUTCOME_UNKNOWN"
        );
        assert!(repo.admission().check_metadata_idle().is_ok());
        // 忙时普通 history 仍可读，但恢复的强制只读门不得绕过 pending owner。
        let active = repo
            .admission()
            .begin_start(crate::version_history::maintenance::RuntimeKind::Native)
            .unwrap()
            .preparing();
        let unarchive = crate::cli::types::UnarchiveRequest {
            request_id: "restore-fixture".into(),
            cli: CliKind::Codex,
            profile_id: "fixture".into(),
            expected_profile_revision: request.source.profile_revision,
            project_path: project.to_str().unwrap().into(),
            native_session_id: id.into(),
        };
        assert!(service
            .reconcile_before_unarchive(&caller, &unarchive)
            .is_err());
        service
            .read(
                &caller,
                &crate::cli::native_projection::wire::ReadRequest {
                    source: request.source.clone(),
                    resource_kind: crate::cli::native_projection::wire::ResourceKind::History,
                    request_epoch: crate::cli::types::WireU64::parse("1").unwrap(),
                    query: None,
                    session_id: None,
                    limit: 100,
                    offset: 0,
                },
            )
            .unwrap();
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "archive\n");
        drop(active);
        if !move_before_ack {
            assert_eq!(
                service
                    .reconcile_before_unarchive(&caller, &unarchive)
                    .err()
                    .unwrap()
                    .code,
                "ARCHIVE_OUTCOME_UNKNOWN"
            );
            assert_eq!(
                service
                    .archive(
                        &caller,
                        ArchiveRequest {
                            operation_id: "blocked-repeat".into(),
                            ..request.clone()
                        }
                    )
                    .err()
                    .unwrap()
                    .code,
                "ARCHIVE_OUTCOME_UNKNOWN"
            );
            assert_eq!(std::fs::read_to_string(&file).unwrap(), transcript);
            return;
        }
        service
            .reconcile_before_unarchive(&caller, &unarchive)
            .unwrap();
        std::fs::rename(&archived, &file).unwrap();
        service
            .archive(
                &caller,
                ArchiveRequest {
                    operation_id: "after-restore".into(),
                    ..request.clone()
                },
            )
            .unwrap()
    } else {
        assert!(repo.admission().check_metadata_idle().is_err());
        assert_eq!(
            service
                .archive(
                    &caller,
                    ArchiveRequest {
                        operation_id: "while-pending".into(),
                        ..request.clone()
                    }
                )
                .err()
                .unwrap()
                .code,
            "ARCHIVE_BUSY"
        );
        worker.join().unwrap().unwrap()
    };
    assert!(receipt.root_archived);
    assert_eq!(receipt.descendants, "unverified");
    assert!(
        late.exists(),
        "the admission must outlive the owned descendant: {:?}",
        std::fs::read_to_string(&child_error)
    );
    assert!(repo.admission().check_metadata_idle().is_ok());
    assert!(!file.exists());
    assert_eq!(std::fs::read_to_string(&archived).unwrap(), transcript);
    assert_eq!(
        std::fs::read_to_string(&unrelated).unwrap(),
        "unrelated fixture"
    );
    assert!(
        service
            .archive(&caller, request.clone())
            .unwrap()
            .root_archived
    );
    let mut changed = request;
    changed.session_key = "different".into();
    assert!(service.archive(&caller, changed).is_err());
    assert_eq!(
        std::fs::read_to_string(marker).unwrap(),
        if lost_ack {
            "archive\narchive\n"
        } else {
            "archive\n"
        }
    );
}

#[test]
#[cfg(windows)]
fn NativeArchive_ContainersRejectJunctionAndPreventReplacement_010() {
    use crate::cli::native_projection::scoped_fs::Root;
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("home");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&root_path).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::create_dir(root_path.join("sessions")).unwrap();
    let root = Root::open(&root_path).unwrap();
    let held = root.pin_archive_containers().unwrap();
    assert!(root_path.join("archived_sessions").is_dir());
    assert!(std::fs::rename(root_path.join("archived_sessions"), root_path.join("moved")).is_err());
    drop(held);
    std::fs::rename(root_path.join("archived_sessions"), root_path.join("moved")).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(root_path.join("archived_sessions"))
        .arg(&outside)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        root.pin_archive_containers().err().unwrap(),
        "ARCHIVE_CONTAINER_UNSAFE"
    );
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
}

#[test]
#[cfg(windows)]
fn NativeArchive_UnknownUnarchivedDoesNotBlockOrdinaryHistory_011() {
    synthetic_archive(true, false);
}
