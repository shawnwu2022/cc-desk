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
    synthetic_archive(false, true, false, false);
}
#[test]
#[cfg(windows)]
fn NativeArchive_LostAckReadRestoreArchiveRecovers_009() {
    synthetic_archive(true, true, false, false);
}
#[cfg(windows)]
fn synthetic_archive(lost_ack: bool, move_before_ack: bool, unrelated_owner: bool, linger: bool) {
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
rl.createInterface({{input:process.stdin}}).on('line',line=>{{const m=JSON.parse(line);if(!m.id)return;let result;if(m.method==='initialize')result={{codexHome:{home}}};else if(m.method==='thread/read')result={{thread:{{id:{id},cwd:{cwd},path}}}};else if(m.method==='thread/archive'){{fs.appendFileSync({marker},'archive\n');if({move_before_ack}){{fs.renameSync(path,archived);path=archived;}}if({lost_ack}&&fs.readFileSync({marker},'utf8').trim().split('\n').length===1)process.exit(0);cp.spawn(process.execPath,[{descendant}],{{stdio:['ignore','inherit',fs.openSync({child_error},'w')],windowsHide:true,detached:true}}).unref();const until=Date.now()+5000;while(!fs.existsSync({started})&&Date.now()<until)Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,10);if(!fs.existsSync({started}))process.exit(8);result={{}};}}else process.exit(9);process.stdout.write(JSON.stringify({{id:m.id,result}})+'\n');}}).on('close',()=>{{if({linger})setInterval(()=>{{}},1000);else process.exit(0);}});"#,
        linger = linger,
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
    let service = Arc::new(ProjectionService::new(launch.clone()));
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
    let unrelated_ticket = unrelated_owner.then(|| {
        repo.admission()
            .begin_start(crate::version_history::maintenance::RuntimeKind::Legacy)
            .unwrap()
            .preparing()
    });
    let started_at = Instant::now();
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
        // 等待身份与已认证 owner 都不能借已归档快路径清除 Unknown。
        for authenticate in [false, true] {
            let identity =
                crate::version_history::codex_archive_admission::CodexOwnerIdentity::new(None);
            if authenticate {
                identity.observe(uuid::Uuid::new_v4());
            }
            let observed = repo
                .admission()
                .begin_start(crate::version_history::maintenance::RuntimeKind::Codex(
                    identity,
                ))
                .unwrap()
                .child_created();
            assert!(service
                .reconcile_before_unarchive(&caller, &unarchive)
                .is_err());
            if move_before_ack {
                assert_eq!(
                    service
                        .archive(
                            &caller,
                            ArchiveRequest {
                                operation_id: "observed-owner-must-not-reconcile".into(),
                                ..request.clone()
                            }
                        )
                        .err()
                        .unwrap()
                        .code,
                    "ARCHIVE_BUSY"
                );
            }
            assert!(
                service
                    .reconcile_before_unarchive(&caller, &unarchive)
                    .is_err(),
                "rejected archive must retain Unknown"
            );
            observed.reaped();
        }
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
        #[allow(dead_code, clippy::duplicate_mod)]
        #[path = "../conpty_runtime.rs"]
        mod bundled_runtime;
        bundled_runtime::initialize().unwrap();
        std::fs::write(
            project.join("unarchive"),
            format!(
                "require('node:fs').renameSync({}, {});",
                json!(archived.to_str().unwrap()),
                json!(file.to_str().unwrap())
            ),
        )
        .unwrap();
        launch
            .run_command(&caller, &unarchive.to_launch_request())
            .unwrap();
        assert!(
            repo.admission().check_codex_archive_idle().is_ok(),
            "successful unarchive must permit immediate rearchive"
        );
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
    if linger {
        assert!(
            started_at.elapsed() < Duration::from_secs(8),
            "verified archive must not wait for the full RPC timeout"
        );
    }
    drop(unrelated_ticket);
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
    synthetic_archive(true, false, false, false);
}

#[test]
#[cfg(windows)]
fn NativeArchive_UnrelatedLegacyOwnerDoesNotBlock_012() {
    synthetic_archive(false, true, true, false);
}
#[test]
#[cfg(windows)]
fn NativeArchive_CompletedHelperHasBoundedShutdown_013() {
    synthetic_archive(false, true, false, true);
}

#[test]
fn NativeArchive_AdmissionKeepsCodexUnknownAndDeleteGuards_014() {
    use crate::cli::types::CliKind;
    use crate::version_history::maintenance::{AdmissionGate, RuntimeKind};
    for cli in [
        CliKind::Claude,
        CliKind::Opencode,
        CliKind::Codex,
        CliKind::Shell,
    ] {
        let gate = AdmissionGate::new();
        let mut preparing = gate.begin_start(RuntimeKind::Native).unwrap().preparing();
        assert!(gate.check_codex_archive_idle().is_err());
        preparing.identify_cli(cli);
        let unrelated = matches!(cli, CliKind::Claude | CliKind::Opencode);
        assert_eq!(gate.check_codex_archive_idle().is_ok(), unrelated);
        let owned = preparing.begin_creation().child_created();
        assert_eq!(gate.check_codex_archive_idle().is_ok(), unrelated);
        assert!(
            gate.begin_metadata().is_err(),
            "permanent delete still requires all owners to end"
        );
        if unrelated {
            let (frozen, helper) = gate.begin_codex_archive().unwrap();
            assert!(gate.begin_start(RuntimeKind::Native).is_err());
            assert!(gate.begin_mutation().is_err());
            drop(helper);
            frozen.release_review().unwrap();
            assert!(
                gate.check_metadata_idle().is_err(),
                "unrelated owner must remain retained"
            );
        }
        drop(owned);
        assert!(
            gate.begin_codex_archive().is_err(),
            "unreaped ownership stays unknown"
        );
    }
    let gate = AdmissionGate::new();
    let mutation = gate.begin_mutation().unwrap();
    assert!(gate.begin_codex_archive().is_err());
    drop(mutation);
    assert!(
        gate.begin_codex_archive().is_err(),
        "unknown writes stay blocked"
    );
}

#[test]
fn NativeArchive_UnrelatedPreparationAndExitKeepFreeze_015() {
    use crate::cli::types::CliKind;
    use crate::version_history::maintenance::{AdmissionGate, RuntimeKind};
    for cli in [CliKind::Claude, CliKind::Opencode] {
        let gate = AdmissionGate::new();
        let mut preparing = gate.begin_start(RuntimeKind::Native).unwrap().preparing();
        preparing.identify_cli(cli);
        let (frozen, helper) = gate.begin_codex_archive().unwrap();
        let owned = preparing.begin_creation().child_created();
        owned.reaped();
        assert!(gate.begin_mutation().is_err());
        assert!(gate.check_codex_archive_idle().is_err());
        drop(helper);
        frozen.release_review().unwrap();
        assert!(gate.check_metadata_idle().is_ok());
    }
}

#[test]
fn NativeArchive_RelationPreflightAllowsOnlyDisjointOwners_024() {
    use std::{
        cell::RefCell,
        collections::{BTreeSet, HashMap},
    };
    let target = uuid::Uuid::new_v4();
    let owner = uuid::Uuid::new_v4();
    let ancestor = uuid::Uuid::new_v4();
    for mode in [
        "unrelated",
        "descendant",
        "ancestor",
        "sibling",
        "missing",
        "wrong-id",
        "native-error",
    ] {
        let t = |id: uuid::Uuid, parent: Option<uuid::Uuid>| json!({"id":id.to_string(),"parentThreadId":parent.map(|id|id.to_string())});
        let mut records = HashMap::from([
            (target, t(target, None)),
            (owner, t(owner, None)),
            (ancestor, t(ancestor, None)),
        ]);
        match mode {
            "descendant" => {
                records.insert(owner, t(owner, Some(target)));
            }
            "ancestor" => {
                records.insert(target, t(target, Some(owner)));
            }
            "sibling" => {
                records.insert(target, t(target, Some(ancestor)));
                records.insert(owner, t(owner, Some(ancestor)));
            }
            "missing" => {
                records.insert(owner, json!({"id":owner.to_string()}));
            }
            "wrong-id" => {
                records.insert(owner, t(target, None));
            }
            _ => {}
        }
        let sent = RefCell::new(Vec::<serde_json::Value>::new());
        let result = exchange_scoped(
            &target.to_string(),
            &BTreeSet::from([owner]),
            &mut |frame| {
                sent.borrow_mut().push(frame);
                Ok(())
            },
            &mut |id| {
                if id == 1 {
                    return Ok(json!({"id":id,"result":{}}));
                }
                if id == 3 {
                    return Ok(if mode == "native-error" {
                        json!({"id":id,"error":{"code":-32600,"message":"writer conflict"}})
                    } else {
                        json!({"id":id,"result":{}})
                    });
                }
                let requested = sent.borrow().last().unwrap()["params"]["threadId"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                let record = records
                    .get(&uuid::Uuid::parse_str(&requested).unwrap())
                    .unwrap();
                Ok(json!({"id":id,"result":{"thread":record}}))
            },
            &|_| Ok(()),
            &|_, _| Ok(()),
        );
        let frames = sent.into_inner();
        let writes = frames
            .iter()
            .filter(|frame| frame["method"] == "thread/archive")
            .count();
        assert!(frames.iter().all(|frame| matches!(
            frame["method"].as_str(),
            Some("initialize" | "initialized" | "thread/read" | "thread/archive")
        )));
        match mode {
            "unrelated" | "sibling" => {
                result.unwrap();
                assert_eq!(writes, 1);
            }
            "native-error" => {
                assert_eq!(result.unwrap_err().code, "ARCHIVE_OUTCOME_UNKNOWN");
                assert_eq!(writes, 1);
            }
            "descendant" | "ancestor" => {
                assert_eq!(result.unwrap_err().code, "ARCHIVE_BUSY");
                assert_eq!(writes, 0);
            }
            _ => {
                assert_eq!(result.unwrap_err().code, "ARCHIVE_RELATION_UNAVAILABLE");
                assert_eq!(writes, 0);
            }
        }
    }
}

#[test]
fn NativeArchive_SendCommitDistinguishesRefusalFromUnknown_025() {
    for mode in ["before", "attempted", "after"] {
        let mut sent = Vec::new();
        let mut replies = replies();
        let result = exchange_scoped(
            "fixture",
            &Default::default(),
            &mut |frame| {
                if mode == "before" && frame["method"] == "thread/archive" {
                    return Err(SendFailure::NotSent(crate::cli::profiles::error(
                        "ARCHIVE_BUSY",
                    )));
                }
                let mutation = frame["method"] == "thread/archive";
                let after = frame["id"] == 4;
                sent.push(frame);
                if mode == "attempted" && mutation {
                    return Err(SendFailure::Attempted(crate::cli::profiles::error(
                        "ARCHIVE_BUSY",
                    )));
                }
                if mode == "after" && after {
                    return Err(SendFailure::NotSent(crate::cli::profiles::error(
                        "ARCHIVE_BUSY",
                    )));
                }
                Ok(())
            },
            &mut |_| Ok(replies.pop_front().unwrap()),
            &|_| Ok(()),
            &|_, _| Ok(()),
        );
        assert_eq!(
            result.unwrap_err().code,
            if mode == "before" {
                "ARCHIVE_BUSY"
            } else {
                "ARCHIVE_OUTCOME_UNKNOWN"
            }
        );
        assert_eq!(
            sent.iter()
                .filter(|frame| frame["method"] == "thread/archive")
                .count(),
            usize::from(mode != "before")
        );
    }
}
#[test]
fn NativeArchive_BlockedInputDoesNotHoldAdmissionAndTimesOutUnknown_028() {
    use crate::version_history::{
        codex_archive_admission::CodexOwnerIdentity,
        maintenance::{AdmissionGate, RuntimeKind},
    };
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    struct Blocked {
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
    }
    impl std::io::Write for Blocked {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            let _ = self.entered.try_send(());
            let _ = self.release.recv();
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "test closed",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    for change in [false, true] {
        let gate = AdmissionGate::new();
        let identity = CodexOwnerIdentity::new(None);
        identity.observe(uuid::Uuid::new_v4());
        let owned = gate
            .begin_start(RuntimeKind::Codex(identity.clone()))
            .unwrap()
            .child_created();
        let (frozen, helper, owners) = gate.begin_codex_archive_for(uuid::Uuid::new_v4()).unwrap();
        let (entered, entered_rx) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let input = input_writer(Blocked {
            entered,
            release: released,
        })
        .unwrap();
        let (completed, completion) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            owners.with_current(|| ()).unwrap();
            let result = send_bytes(
                &input,
                b"archive\n".to_vec(),
                Instant::now() + Duration::from_millis(300),
                &|| owners.current(),
            )
            .map_err(SendFailure::Attempted)
            .map_err(SendFailure::archive);
            let _ = completed.send(result);
        });
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let (probed, probe_result) = mpsc::sync_channel(1);
        let probe_gate = gate.clone();
        let probe = std::thread::spawn(move || {
            let refused = probe_gate.begin_start(RuntimeKind::Native).is_err();
            if change {
                identity.observe(uuid::Uuid::new_v4());
            }
            let _ = probed.send(refused);
        });
        // 在释放 blocked Write 之前观察门禁、hook 和超时，先清理后断言避免失败挂起。
        let responsive = probe_result.recv_timeout(Duration::from_secs(1));
        let bounded = completion.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        probe.join().unwrap();
        worker.join().unwrap();
        assert!(responsive.unwrap());
        assert_eq!(
            bounded.unwrap().unwrap_err().code,
            "ARCHIVE_OUTCOME_UNKNOWN"
        );
        drop(helper);
        frozen.release_review().unwrap();
        owned.reaped();
    }
}

// 显式隔离基准：真实授权/磁盘/Job，只有官方 RPC 对端用本次临时 Node 进程替代。
#[test]
#[cfg(windows)]
#[ignore = "explicit isolated archive stage benchmark"]
fn NativeArchivePerf_IsolatedStageMatrix_029() {
    use crate::cli::{
        environment::EnvMap,
        launch_service::LaunchService,
        native_projection::{
            service::ProjectionService,
            wire::{ProjectionState, ReadRequest, ResourceKind, ScopeTarget},
        },
        profiles::{EnvValue, Override, Profile},
        storage::{Patch, WorkspaceRepository},
        types::{CliKind, WireU64},
    };
    use std::{
        sync::Arc,
        time::{Instant, SystemTime, UNIX_EPOCH},
    };
    assert_eq!(
        std::env::var("CC_DESK_ARCHIVE_PERF").as_deref(),
        Ok("isolated-stages")
    );
    let node = std::process::Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .unwrap();
    assert!(node.status.success());
    let node = String::from_utf8(node.stdout).unwrap().trim().to_owned();
    for (count, project_count) in [(1usize, 1usize), (128, 1), (1024, 1), (1024, 8)] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("codex-home");
        let directory = root.join("sessions/2026/11/13");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::create_dir(root.join("archived_sessions")).unwrap();
        let repo = WorkspaceRepository::open(temp.path().join("desk/workspace.json")).unwrap();
        let mut project_ids = Vec::new();
        let mut projects = Vec::new();
        for index in 0..project_count {
            let path = temp.path().join(format!("project-{index}"));
            std::fs::create_dir(&path).unwrap();
            project_ids.push(
                crate::cli::workspace::register_project(&repo, &path)
                    .unwrap()
                    .project_id,
            );
            projects.push(path);
        }
        let id = "00000000-0000-4000-8000-000000000000";
        let target_name = format!("rollout-2026-11-13T00-00-00-{id}.jsonl");
        let file = directory.join(&target_name);
        let archived = root.join("archived_sessions").join(&target_name);
        for index in 0..count {
            let native_id = format!("00000000-0000-4000-8000-{index:012}");
            let text = format!(
                "{}\n{}\n{}\n",
                json!({"type":"session_meta","timestamp":"2026-11-13T00:00:00Z","payload":{"id":native_id,"cwd":projects[index % project_count],"source":"cli"}}),
                json!({"type":"event_msg","timestamp":"2026-11-13T00:00:01Z","payload":{"type":"user_message","message":"Synthetic archive performance fixture"}}),
                json!({"type":"event_msg","timestamp":"2026-11-13T00:00:02Z","payload":{"type":"agent_message","message":"Synthetic fixture result"}})
            );
            std::fs::write(
                directory.join(format!("rollout-2026-11-13T00-00-00-{native_id}.jsonl")),
                text,
            )
            .unwrap();
        }
        let trace = temp.path().join("rpc-trace.jsonl");
        let script = format!(
            r#"const fs=require('node:fs'),rl=require('node:readline');const trace={trace};function mark(phase){{fs.appendFileSync(trace,JSON.stringify({{phase,at:Date.now()}})+'\n');}}mark('helper-start');let path={file};rl.createInterface({{input:process.stdin}}).on('line',line=>{{const m=JSON.parse(line);if(!m.id)return;mark(m.method);let result;if(m.method==='initialize')result={{codexHome:{home}}};else if(m.method==='thread/read')result={{thread:{{id:{id},cwd:{cwd},path}}}};else if(m.method==='thread/archive'){{fs.renameSync(path,{archived});path={archived};result={{}};}}else process.exit(9);process.stdout.write(JSON.stringify({{id:m.id,result}})+'\n');}}).on('close',()=>process.exit(0));"#,
            trace = json!(trace),
            file = json!(file),
            home = json!(root),
            id = json!(id),
            cwd = json!(projects[0]),
            archived = json!(archived)
        );
        std::fs::write(projects[0].join("app-server"), script).unwrap();
        let mut profile = Profile::new("archive-perf", CliKind::Codex);
        profile.program_path = Override::Set(node.clone());
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
        let service = ProjectionService::new(launch.clone());
        let start = Instant::now();
        let source = service
            .scope(
                &caller,
                &ScopeTarget::Profile {
                    profile_id: "archive-perf".into(),
                    expected_profile_revision: document.profiles["archive-perf"].revision,
                    project_ids,
                },
            )
            .unwrap();
        let scope_ms = start.elapsed().as_secs_f64() * 1000.0;
        let observe = |epoch: u64| {
            let started = Instant::now();
            let mut offset = 0usize;
            loop {
                let response = service
                    .read(
                        &caller,
                        &ReadRequest {
                            source: source.clone(),
                            resource_kind: ResourceKind::History,
                            request_epoch: WireU64::parse(&epoch.to_string()).unwrap(),
                            query: None,
                            session_id: None,
                            limit: 200,
                            offset: offset.try_into().unwrap(),
                        },
                    )
                    .unwrap();
                assert_eq!(response.state, ProjectionState::Ready);
                offset += response.items.len();
                if !response.has_more {
                    break;
                }
            }
            assert_eq!(offset, count);
            started.elapsed().as_secs_f64() * 1000.0
        };
        let initial_read_ms = observe(1);
        let started = Instant::now();
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let receipt = service
            .archive(
                &caller,
                ArchiveRequest {
                    operation_id: format!("perf-{count}-{project_count}"),
                    source: source.clone(),
                    session_key: json!(["local", "codex", source.source_root_key, id]).to_string(),
                },
            )
            .unwrap();
        let archive_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert!(receipt.root_archived && archived.exists() && !file.exists());
        assert!(repo.admission().check_metadata_idle().is_ok());
        let events: Vec<serde_json::Value> = std::fs::read_to_string(&trace)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let helper = events.first().unwrap()["at"].as_u64().unwrap();
        let last = events.last().unwrap()["at"].as_u64().unwrap();
        let readback_ms = observe(2);
        println!(
            "ARCHIVE_PERF {}",
            json!({"sessions":count,"projects":project_count,"scope_ms":scope_ms,"initial_read_ms":initial_read_ms,"pre_rpc_ms":helper.saturating_sub(wall),"rpc_window_ms":last.saturating_sub(helper),"archive_ms":archive_ms,"post_rpc_ms":archive_ms-(last.saturating_sub(wall) as f64),"readback_ms":readback_ms,"total_action_ms":archive_ms+readback_ms})
        );
        drop(service);
        drop(launch);
        drop(repo);
    }
}

#[test]
fn NativeArchive_PendingIdentityChangePreservesDispatchBoundary_034() {
    use crate::version_history::{
        codex_archive_admission::CodexOwnerIdentity,
        maintenance::{AdmissionGate, RuntimeKind},
    };
    for after_dispatch in [false, true] {
        let gate = AdmissionGate::new();
        let identity = CodexOwnerIdentity::new(None);
        let owned = gate
            .begin_start(RuntimeKind::Codex(identity.clone()))
            .unwrap()
            .child_created();
        let (frozen, helper, owners) = gate.begin_codex_archive_for(uuid::Uuid::new_v4()).unwrap();
        let mut sent = Vec::new();
        let mut responses = replies();
        let result = exchange_scoped(
            "fixture",
            owners.sessions(),
            &mut |frame| {
                let archive = frame["method"] == "thread/archive";
                if archive && !after_dispatch {
                    identity.observe(uuid::Uuid::new_v4());
                }
                owners.with_current(|| ()).map_err(SendFailure::NotSent)?;
                sent.push(frame);
                if archive && after_dispatch {
                    identity.observe(uuid::Uuid::new_v4());
                    owners.current().map_err(SendFailure::Attempted)?;
                }
                Ok(())
            },
            &mut |_| Ok(responses.pop_front().unwrap()),
            &|_| Ok(()),
            &|_, _| Ok(()),
        );
        assert_eq!(
            result.unwrap_err().code,
            if after_dispatch {
                "ARCHIVE_OUTCOME_UNKNOWN"
            } else {
                "ARCHIVE_BUSY"
            }
        );
        assert_eq!(
            sent.iter()
                .filter(|frame| frame["method"] == "thread/archive")
                .count(),
            usize::from(after_dispatch)
        );
        drop(helper);
        frozen.release_review().unwrap();
        owned.reaped();
    }
}
