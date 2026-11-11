#![allow(non_snake_case)]
use super::*;
use serde_json::json;

#[test]
fn NativeDelete_NoAckOrChangedPreviewNeverClaims_001() {
    let mut confirm: ConfirmRequest = serde_json::from_value(json!({"operationId":"00000000-0000-4000-8000-000000000001","previewId":"00000000-0000-4000-8000-000000000002","acknowledgePermanentDelete":false,"acknowledgeNativeSubtree":true})).unwrap();
    assert!(confirm.validate().is_err());
    confirm.acknowledge_permanent_delete = true;
    confirm.validate().unwrap();
    let gate = crate::version_history::maintenance::AdmissionGate::new();
    let starting = gate
        .begin_start(crate::version_history::maintenance::RuntimeKind::Native)
        .unwrap();
    assert!(gate.begin_metadata().is_err());
    drop(starting);
    assert!(gate.begin_metadata().is_err());
}

fn request() -> PreviewRequest {
    serde_json::from_value(json!({"operationId":"00000000-0000-4000-8000-000000000003","sessionKey":"fixture","source":{"scopeId":"scope","instanceId":"instance","cli":"codex","sourceRootKey":"root","identityEpoch":"1","profileId":"profile","profileRevision":"1","basis":"configured-profile","target":{"kind":"profile","profileId":"profile","expectedProfileRevision":"1","projectIds":["project"]}}})).unwrap()
}
fn caller() -> super::CallerIdentity {
    super::CallerIdentity {
        instance_id: "instance".into(),
        window_label: "main".into(),
        webview_epoch: crate::cli::types::WireU64::parse("1").unwrap(),
    }
}
#[test]
fn NativeDelete_ChangedPreviewIsConsumedAndDuplicateNeverClaims_002() {
    let operations = Operations::default();
    let caller = caller();
    let request = request();
    let preview = operations
        .register(
            &caller,
            &request,
            "00000000-0000-4000-8000-000000000004",
            "before".into(),
        )
        .unwrap();
    let confirm = ConfirmRequest {
        operation_id: request.operation_id.clone(),
        preview_id: preview.preview_id.unwrap(),
        acknowledge_permanent_delete: true,
        acknowledge_native_subtree: true,
    };
    for observation in ["changed", "before"] {
        let receipt = operations
            .begin(&caller, &confirm, observation)
            .unwrap()
            .unwrap();
        assert_eq!(receipt.state, "blocked");
        assert_eq!(receipt.reason, Some("DELETE_PREVIEW_CHANGED"));
        let entry = operations.get(&caller, &request.operation_id).unwrap();
        assert!(entry.confirmed.is_none());
        assert!(!entry.active.load(Ordering::Acquire));
        assert_eq!(entry.receipt.unwrap().state, "blocked");
    }
    let operations = Operations::default();
    let preview = operations
        .register(
            &caller,
            &request,
            "00000000-0000-4000-8000-000000000004",
            "before".into(),
        )
        .unwrap();
    let confirm = ConfirmRequest {
        preview_id: preview.preview_id.unwrap(),
        ..confirm
    };
    assert!(operations
        .begin(&caller, &confirm, "before")
        .unwrap()
        .is_none());
    assert_eq!(
        operations
            .begin(&caller, &confirm, "before")
            .unwrap()
            .unwrap()
            .state,
        "unknown"
    );
    operations.finish(
        &caller,
        &request.operation_id,
        request.receipt("unknown", Some(false), Some("DELETE_OUTCOME_UNKNOWN")),
    );
    let mut next = request.clone();
    next.operation_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        operations
            .register(
                &caller,
                &next,
                "00000000-0000-4000-8000-000000000004",
                "before".into()
            )
            .err()
            .unwrap()
            .code,
        "SOURCE_BUSY"
    );
    next.source.profile_id = "other-profile".into();
    next.session_key = "other-profile-key".into();
    assert_eq!(
        operations
            .register(
                &caller,
                &next,
                "00000000-0000-4000-8000-000000000004",
                "before".into()
            )
            .err()
            .unwrap()
            .code,
        "SOURCE_BUSY"
    );
    let mut stranger = caller.clone();
    stranger.window_label = "other".into();
    assert!(operations.get(&stranger, &request.operation_id).is_err());
}

#[test]
fn NativeDelete_ExpiredPreviewIsBlockedBeforeClaimAndCanReviewAgain_011() {
    let operations = Operations::default();
    let caller = caller();
    let request = request();
    let preview = operations
        .register(
            &caller,
            &request,
            "00000000-0000-4000-8000-000000000004",
            "before".into(),
        )
        .unwrap();
    operations.0.lock().unwrap()[0].created = Instant::now() - Duration::from_secs(121);
    let confirm = ConfirmRequest {
        operation_id: request.operation_id.clone(),
        preview_id: preview.preview_id.unwrap(),
        acknowledge_permanent_delete: true,
        acknowledge_native_subtree: true,
    };
    let receipt = operations
        .begin(&caller, &confirm, "before")
        .unwrap()
        .unwrap();
    assert_eq!(receipt.state, "blocked");
    assert_eq!(receipt.reason, Some("DELETE_PREVIEW_CHANGED"));
    let entry = operations.get(&caller, &request.operation_id).unwrap();
    assert!(entry.confirmed.is_none());
    assert!(!entry.active.load(Ordering::Acquire));
    assert_eq!(entry.receipt.unwrap().state, "blocked");
    assert_eq!(
        operations
            .begin(&caller, &confirm, "before")
            .unwrap()
            .unwrap()
            .state,
        "blocked"
    );
    let fresh = PreviewRequest {
        operation_id: uuid::Uuid::new_v4().to_string(),
        ..request
    };
    assert_eq!(
        operations
            .register(
                &caller,
                &fresh,
                "00000000-0000-4000-8000-000000000004",
                "before".into()
            )
            .unwrap()
            .state,
        "ready"
    );
}

#[test]
fn NativeDelete_UnknownPhysicalTargetLocksAcrossProfileAndUuidSpelling_006() {
    let operations = Operations::default();
    let caller = caller();
    let request = request();
    let preview = operations
        .register(
            &caller,
            &request,
            "00000000-0000-4000-8000-aabbccddeeff",
            "first".into(),
        )
        .unwrap();
    let confirm = ConfirmRequest {
        operation_id: request.operation_id.clone(),
        preview_id: preview.preview_id.unwrap(),
        acknowledge_permanent_delete: true,
        acknowledge_native_subtree: true,
    };
    operations.begin(&caller, &confirm, "first").unwrap();
    operations.finish(
        &caller,
        &request.operation_id,
        request.receipt("unknown", Some(false), Some("DELETE_OUTCOME_UNKNOWN")),
    );
    let mut other = request.clone();
    other.operation_id = uuid::Uuid::new_v4().to_string();
    other.source.profile_id = "other-profile".into();
    other.session_key = "another-key".into();
    let mut other_caller = caller.clone();
    other_caller.window_label = "another-caller".into();
    assert_eq!(
        operations
            .register(
                &other_caller,
                &other,
                "00000000-0000-4000-8000-AABBCCDDEEFF",
                "second".into()
            )
            .err()
            .unwrap()
            .code,
        "SOURCE_BUSY"
    );
    other.source.source_root_key = "different-physical-root".into();
    assert!(operations
        .register(
            &other_caller,
            &other,
            "00000000-0000-4000-8000-AABBCCDDEEFF",
            "second".into()
        )
        .is_ok());
}

#[test]
fn NativeDelete_LateInvalidationCannotReclassifyClaimedOperation_012() {
    let operations = Operations::default();
    let caller = caller();
    let request = request();
    let preview = operations
        .register(
            &caller,
            &request,
            "00000000-0000-4000-8000-000000000004",
            "before".into(),
        )
        .unwrap();
    let confirm = ConfirmRequest {
        operation_id: request.operation_id.clone(),
        preview_id: preview.preview_id.unwrap(),
        acknowledge_permanent_delete: true,
        acknowledge_native_subtree: true,
    };
    assert!(operations
        .begin(&caller, &confirm, "before")
        .unwrap()
        .is_none());
    assert_eq!(
        operations
            .invalidate(&caller, &request.operation_id)
            .unwrap()
            .state,
        "unknown"
    );
    let entry = operations.get(&caller, &request.operation_id).unwrap();
    assert!(entry.active.load(Ordering::Acquire));
    assert!(entry.confirmed.is_some());
    assert_eq!(
        entry.preview.preview_id.as_deref(),
        Some(confirm.preview_id.as_str())
    );
    assert!(entry.receipt.is_none());
    let unknown = request.receipt("unknown", None, Some("DELETE_OUTCOME_UNKNOWN"));
    operations.finish(&caller, &request.operation_id, unknown);
    assert_eq!(
        operations
            .invalidate(&caller, &request.operation_id)
            .unwrap()
            .state,
        "unknown"
    );
    assert_eq!(
        operations
            .begin(&caller, &confirm, "before")
            .unwrap()
            .unwrap()
            .state,
        "unknown"
    );
}

#[test]
fn NativeDelete_AbsenceNeedsAckAndAckNeedsReapedOwner_007() {
    let operations = Operations::default();
    let caller = caller();
    let request = request();
    let preview = operations
        .register(
            &caller,
            &request,
            "00000000-0000-4000-8000-000000000004",
            "before".into(),
        )
        .unwrap();
    let confirm = ConfirmRequest {
        operation_id: request.operation_id.clone(),
        preview_id: preview.preview_id.unwrap(),
        acknowledge_permanent_delete: true,
        acknowledge_native_subtree: true,
    };
    operations.begin(&caller, &confirm, "before").unwrap();
    let entry = operations.get(&caller, &request.operation_id).unwrap();
    assert_eq!(reconcile(&entry, Some(false)).state, "unknown");
    *entry.outcome.lock().unwrap() = Some(Ok(()));
    assert_eq!(reconcile(&entry, Some(false)).state, "unknown");
    entry.active.store(false, Ordering::Release);
    assert_eq!(reconcile(&entry, Some(false)).state, "acknowledged");
    *entry.outcome.lock().unwrap() = Some(Err(error("DELETE_OUTCOME_UNKNOWN")));
    assert_eq!(reconcile(&entry, Some(false)).state, "unknown");
    assert_eq!(reconcile(&entry, Some(true)).state, "partial");
}
#[test]
fn NativeDelete_ProtocolNeverResumesAndOriginFailureNeverDeletes_003() {
    for accept in [true, false] {
        let mut sent = Vec::new();
        let mut replies = std::collections::VecDeque::from([
            json!({"id":1,"result":{}}),
            json!({"id":2,"result":{"thread":{}}}),
            json!({"id":3,"result":{}}),
        ]);
        let result = exchange(
            "fixture",
            &mut |v| {
                sent.push(v);
                Ok(())
            },
            &mut |_| Ok(replies.pop_front().unwrap()),
            &|_| {
                if accept {
                    Ok(())
                } else {
                    Err(error("DELETE_ROOT_NOT_ARCHIVED"))
                }
            },
            &|_| Ok(()),
        );
        assert_eq!(result.is_ok(), accept);
        assert_eq!(
            sent[0]["params"]["capabilities"]["explicitGatewayOauth"],
            json!(true),
            "metadata deletion must never enable automatic gateway login"
        );
        assert_eq!(
            sent.iter()
                .filter(|v| v["method"] == "thread/delete")
                .count(),
            usize::from(accept)
        );
        assert!(!sent
            .iter()
            .any(|v| v["method"] == "thread/resume" || v["method"] == "turn/start"));
    }
}
#[test]
fn NativeDelete_PreDispatchProtocolFailureRetainsStage_008() {
    for phase in [1, 2, 3] {
        for malformed in [false, true] {
            let mut sent = Vec::new();
            let failure = exchange(
                "fixture",
                &mut |v| {
                    sent.push(v);
                    Ok(())
                },
                &mut |id| {
                    if id == phase {
                        if malformed {
                            Ok(json!({"id":id,"error":{"code":-32000,"message":"private fixture error"}}))
                        } else {
                            Err(error("DELETE_OUTCOME_UNKNOWN"))
                        }
                    } else {
                        Ok(json!({"id":id,"result":{"thread":{}}}))
                    }
                },
                &|_| Ok(()),
                &|_| Ok(()),
            )
            .unwrap_err();
            if phase < 3 {
                assert_eq!(
                    failure.code,
                    if phase == 1 {
                        "DELETE_INITIALIZE_FAILED"
                    } else {
                        "DELETE_READ_FAILED"
                    }
                );
                assert!(!sent.iter().any(|v| v["method"] == "thread/delete"));
            } else {
                assert!(matches!(
                    failure.code.as_str(),
                    "DELETE_OUTCOME_UNKNOWN" | "DELETE_NOT_ACKNOWLEDGED"
                ));
                assert_eq!(
                    sent.iter()
                        .filter(|v| v["method"] == "thread/delete")
                        .count(),
                    1
                );
            }
        }
    }
}
#[test]
fn NativeDelete_ReapedPreDispatchFailureRemainsBlocked_009() {
    for stage in [
        "DELETE_INITIALIZE_FAILED",
        "DELETE_READ_FAILED",
        "DELETE_PREPARE_FAILED",
    ] {
        let operations = Operations::default();
        let request = request();
        let caller = caller();
        let preview = operations
            .register(
                &caller,
                &request,
                "00000000-0000-4000-8000-000000000004",
                "before".into(),
            )
            .unwrap();
        let confirm = ConfirmRequest {
            operation_id: request.operation_id.clone(),
            preview_id: preview.preview_id.unwrap(),
            acknowledge_permanent_delete: true,
            acknowledge_native_subtree: true,
        };
        operations.begin(&caller, &confirm, "before").unwrap();
        let entry = operations.get(&caller, &request.operation_id).unwrap();
        *entry.outcome.lock().unwrap() = Some(Err(error(stage)));
        assert_eq!(reconcile(&entry, Some(true)).state, "unknown");
        entry.active.store(false, Ordering::Release);
        for present in [None, Some(true), Some(false)] {
            let receipt = reconcile(&entry, present);
            assert_eq!(receipt.state, "blocked");
            assert_eq!(receipt.reason, Some(stage));
        }
    }
}
#[test]
fn NativeDelete_PreWriteRevalidationFailureIsNotDispatch_010() {
    let mut input = Vec::new();
    let mut started = false;
    let failure = send_packet(
        &mut input,
        &json!({"method":"thread/delete","id":3}),
        &|| Err(error("SCOPE_STALE")),
        &mut started,
        Instant::now() + Duration::from_secs(30),
    )
    .unwrap_err();
    assert_eq!(failure.code, "SCOPE_STALE");
    assert!(!started);
    assert!(input.is_empty());
    struct FailingWriter;
    impl std::io::Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("fixture write failed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(
        send_packet(
            &mut FailingWriter,
            &json!({"method":"thread/delete","id":3}),
            &|| Ok(()),
            &mut started,
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap_err()
        .code,
        "DELETE_OUTCOME_UNKNOWN"
    );
    assert!(
        started,
        "write error cannot prove that no bytes reached the peer"
    );
}
#[test]
#[cfg(windows)]
fn NativeDelete_TwoPhaseSyntheticChildReapsAndDeletesOnce_004() {
    use crate::cli::{
        environment::EnvMap,
        launch_service::LaunchService,
        native_projection::{service::ProjectionService, wire::ScopeTarget},
        profiles::{EnvValue, Override, Profile},
        storage::{Patch, WorkspaceRepository},
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("codex-home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(root.join("archived_sessions")).unwrap();
    std::fs::create_dir_all(root.join("sessions")).unwrap();
    std::fs::create_dir(&project).unwrap();
    let id = "00000000-0000-4000-8000-000000000004";
    let rollout = root
        .join("archived_sessions")
        .join(format!("rollout-2026-11-06T00-00-00-{id}.jsonl"));
    let transcript = format!(
        "{}\n",
        json!({"type":"session_meta","payload":{"id":id,"cwd":project.to_str().unwrap(),"timestamp":"2026-11-06T00:00:00Z"}})
    );
    std::fs::write(&rollout, &transcript).unwrap();
    let marker = temp.path().join("delete-calls");
    let script = format!(
        r#"const fs=require('node:fs');const readline=require('node:readline');const file={file};const marker={marker};const thread={{id:{id},cwd:{cwd},path:file}};
fs.writeFileSync(marker+'.trace','started\n');readline.createInterface({{input:process.stdin}}).on('line',line=>{{const m=JSON.parse(line);fs.appendFileSync(marker+'.trace',m.method+'\n');if(!m.id)return;if(m.method==='initialize')process.stdout.write(JSON.stringify({{id:m.id,result:{{codexHome:{home}}}}})+'\n');else if(m.method==='thread/read')process.stdout.write(JSON.stringify({{id:m.id,result:{{thread}}}})+'\n');else if(m.method==='thread/delete'){{fs.appendFileSync(marker,'delete\n');fs.unlinkSync(file);process.stdout.write(JSON.stringify({{id:m.id,result:{{}}}})+'\n',()=>process.exit(0));}}else process.exit(9);}});"#,
        home = json!(root.to_str().unwrap()),
        file = json!(rollout.to_str().unwrap()),
        marker = json!(marker.to_str().unwrap()),
        id = json!(id),
        cwd = json!(project.to_str().unwrap())
    );
    let script=script.replace("const m=JSON.parse(line);","const m=JSON.parse(line);if(m.method==='thread/read'){process.stdout.write(JSON.stringify({method:'configWarning',params:{message:'fixture'}})+'\\n');process.stdout.write(JSON.stringify({method:'account/updated',params:{type:'fixture'}})+'\\n');process.stdout.write(JSON.stringify({method:'remoteControl/status/changed',params:{status:'disabled',serverName:'fixture',installationId:'fixture',environmentId:null}})+'\\n');}");
    std::fs::write(project.join("app-server"), &script).unwrap();
    let output = std::process::Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let node = String::from_utf8(output.stdout).unwrap().trim().to_string();
    let syntax = std::process::Command::new(&node)
        .arg("--check")
        .arg(project.join("app-server"))
        .output()
        .unwrap();
    assert!(
        syntax.status.success(),
        "synthetic script syntax: {}",
        String::from_utf8_lossy(&syntax.stderr)
    );
    let repo = WorkspaceRepository::open(temp.path().join("desk/workspace.json")).unwrap();
    let registered = crate::cli::workspace::register_project(&repo, &project).unwrap();
    let mut profile = Profile::new("fixture", CliKind::Codex);
    profile.program_path = Override::Set(node);
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
    let mut inherited = EnvMap::new();
    // Windows Node 的随机数初始化需要系统根路径；合成环境其余变量保持为空。
    inherited.insert("SystemRoot".into(), std::env::var_os("SystemRoot").unwrap());
    let launch = Arc::new(LaunchService::new(repo.clone(), Some(inherited), None));
    let caller = launch.registry().activate_window("main").unwrap();
    let service = ProjectionService::new(launch.clone());
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
    let request = PreviewRequest {
        operation_id: uuid::Uuid::new_v4().to_string(),
        session_key: json!(["local", "codex", source.source_root_key, id]).to_string(),
        source,
    };
    let preview = service.preview_delete(&caller, request.clone()).unwrap();
    assert_eq!(preview.state, "ready");
    assert!(!marker.exists());
    assert!(!marker.with_extension("trace").exists());
    assert_eq!(preview.native_session_id.as_deref(), Some(id));
    let confirm = ConfirmRequest {
        operation_id: request.operation_id.clone(),
        preview_id: preview.preview_id.unwrap(),
        acknowledge_permanent_delete: false,
        acknowledge_native_subtree: true,
    };
    assert!(service.confirm_delete(&caller, confirm.clone()).is_err());
    assert!(!marker.exists());
    assert!(!marker.with_extension("trace").exists());
    let active_request = PreviewRequest {
        operation_id: uuid::Uuid::new_v4().to_string(),
        ..request.clone()
    };
    let active_preview = service
        .preview_delete(&caller, active_request.clone())
        .unwrap();
    let ticket = repo
        .admission()
        .begin_start(crate::version_history::maintenance::RuntimeKind::Native)
        .unwrap();
    assert_eq!(
        service
            .preview_delete(
                &caller,
                PreviewRequest {
                    operation_id: uuid::Uuid::new_v4().to_string(),
                    ..request.clone()
                }
            )
            .unwrap()
            .reason,
        Some("SOURCE_BUSY")
    );
    assert_eq!(
        service
            .confirm_delete(
                &caller,
                ConfirmRequest {
                    operation_id: active_request.operation_id,
                    preview_id: active_preview.preview_id.unwrap(),
                    acknowledge_permanent_delete: true,
                    acknowledge_native_subtree: true
                }
            )
            .unwrap()
            .reason,
        Some("SOURCE_BUSY")
    );
    assert!(!marker.with_extension("trace").exists());
    ticket.no_child_created();
    let confirm = ConfirmRequest {
        acknowledge_permanent_delete: true,
        ..confirm
    };
    std::fs::write(
        root.join("config.toml"),
        "# observed configuration changed\n",
    )
    .unwrap();
    let blocked = service.confirm_delete(&caller, confirm.clone()).unwrap();
    assert_eq!(blocked.state, "blocked");
    assert_eq!(blocked.reason, Some("DELETE_PREVIEW_CHANGED"));
    assert_eq!(
        service
            .inspect_delete(
                &caller,
                InspectRequest {
                    operation_id: request.operation_id.clone()
                }
            )
            .unwrap()
            .state,
        "blocked"
    );
    assert!(!marker.exists());
    assert!(!marker.with_extension("trace").exists());
    std::fs::remove_file(root.join("config.toml")).unwrap();
    assert_eq!(
        service
            .confirm_delete(&caller, confirm.clone())
            .unwrap()
            .state,
        "blocked"
    );
    assert!(!marker.with_extension("trace").exists());
    let request = PreviewRequest {
        operation_id: uuid::Uuid::new_v4().to_string(),
        ..request
    };
    let preview = service.preview_delete(&caller, request.clone()).unwrap();
    let confirm = ConfirmRequest {
        operation_id: request.operation_id.clone(),
        preview_id: preview.preview_id.unwrap(),
        acknowledge_permanent_delete: true,
        acknowledge_native_subtree: true,
    };
    let receipt = service.confirm_delete(&caller, confirm.clone()).unwrap();
    assert_eq!(
        receipt.state,
        "acknowledged",
        "synthetic marker present: {} root present: {} trace: {:?}",
        marker.exists(),
        rollout.exists(),
        std::fs::read_to_string(marker.with_extension("trace"))
    );
    assert!(!rollout.exists());
    assert_eq!(
        service.confirm_delete(&caller, confirm).unwrap().state,
        "acknowledged"
    );
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "delete\n");
    assert_eq!(
        service
            .inspect_delete(
                &caller,
                InspectRequest {
                    operation_id: request.operation_id.clone()
                }
            )
            .unwrap()
            .state,
        "acknowledged"
    );
    assert!(repo.admission().check_metadata_idle().is_ok());

    // 两个根在同一目录、同一配置预览后逐项真执行合成助手；第二张票不能被首删污染。
    let batch_marker = temp.path().join("batch-delete-calls");
    let mut pending = Vec::new();
    for native in [
        "00000000-0000-4000-8000-000000000005",
        "00000000-0000-4000-8000-000000000006",
    ] {
        let path = root
            .join("archived_sessions")
            .join(format!("rollout-{native}.jsonl"));
        std::fs::write(&path, transcript.replace(id, native)).unwrap();
        let request = PreviewRequest {
            operation_id: uuid::Uuid::new_v4().to_string(),
            session_key: json!(["local", "codex", request.source.source_root_key, native])
                .to_string(),
            source: request.source.clone(),
        };
        pending.push((native, path, request));
    }
    let tickets: Vec<_> = pending
        .iter()
        .map(|(_, _, request)| service.preview_delete(&caller, request.clone()).unwrap())
        .collect();
    for ((native, path, request), preview) in pending.iter().zip(tickets) {
        assert_eq!(preview.state, "ready");
        let variant = script
            .replace(id, native)
            .replace(
                &json!(rollout.to_str().unwrap())
                    .to_string()
                    .replace(id, native),
                &json!(path.to_str().unwrap()).to_string(),
            )
            .replace(
                &json!(marker.to_str().unwrap()).to_string(),
                &json!(batch_marker.to_str().unwrap()).to_string(),
            );
        std::fs::write(project.join("app-server"), variant).unwrap();
        let receipt = service
            .confirm_delete(
                &caller,
                ConfirmRequest {
                    operation_id: request.operation_id.clone(),
                    preview_id: preview.preview_id.unwrap(),
                    acknowledge_permanent_delete: true,
                    acknowledge_native_subtree: true,
                },
            )
            .unwrap();
        assert_eq!(
            receipt.state, "acknowledged",
            "same-source batch root: {native}"
        );
        assert!(!path.exists());
    }
    assert_eq!(
        std::fs::read_to_string(batch_marker)
            .unwrap()
            .lines()
            .count(),
        2
    );
    // 原生子树删除包含另一已选根时，只能记首项 ACK，第二项缺失不得合成 ACK。
    let parent_id = "00000000-0000-4000-8000-000000000007";
    let child_id = "00000000-0000-4000-8000-000000000008";
    let parent_path = root.join("archived_sessions/overlap-parent.jsonl");
    let child_path = root.join("archived_sessions/overlap-child.jsonl");
    std::fs::write(&parent_path, transcript.replace(id, parent_id)).unwrap();
    std::fs::write(&child_path, transcript.replace(id, child_id)).unwrap();
    let overlap: Vec<_> = [parent_id, child_id]
        .into_iter()
        .map(|native| {
            let request = PreviewRequest {
                operation_id: uuid::Uuid::new_v4().to_string(),
                session_key: json!(["local", "codex", request.source.source_root_key, native])
                    .to_string(),
                source: request.source.clone(),
            };
            let preview = service.preview_delete(&caller, request.clone()).unwrap();
            assert_eq!(preview.state, "ready");
            ConfirmRequest {
                operation_id: request.operation_id,
                preview_id: preview.preview_id.unwrap(),
                acknowledge_permanent_delete: true,
                acknowledge_native_subtree: true,
            }
        })
        .collect();
    let overlap_marker = temp.path().join("overlap-delete-calls");
    let variant = script
        .replace(id, parent_id)
        .replace(
            &json!(rollout.to_str().unwrap())
                .to_string()
                .replace(id, parent_id),
            &json!(parent_path.to_str().unwrap()).to_string(),
        )
        .replace(
            &json!(marker.to_str().unwrap()).to_string(),
            &json!(overlap_marker.to_str().unwrap()).to_string(),
        )
        .replace(
            "fs.unlinkSync(file);",
            &format!(
                "fs.unlinkSync(file);fs.unlinkSync({});",
                json!(child_path.to_str().unwrap())
            ),
        );
    std::fs::write(project.join("app-server"), variant).unwrap();
    assert_eq!(
        service
            .confirm_delete(&caller, overlap[0].clone())
            .unwrap()
            .state,
        "acknowledged"
    );
    assert!(!parent_path.exists() && !child_path.exists());
    let missing = service.confirm_delete(&caller, overlap[1].clone()).unwrap();
    assert_eq!(missing.state, "blocked");
    assert_eq!(
        std::fs::read_to_string(overlap_marker)
            .unwrap()
            .lines()
            .count(),
        1
    );
    std::fs::write(project.join("app-server"), &script).unwrap();

    let active_rollout = root.join("sessions").join(rollout.file_name().unwrap());
    std::fs::write(&active_rollout, &transcript).unwrap();
    assert_eq!(
        service
            .preview_delete(
                &caller,
                PreviewRequest {
                    operation_id: uuid::Uuid::new_v4().to_string(),
                    ..request.clone()
                }
            )
            .unwrap()
            .reason,
        Some("DELETE_ROOT_NOT_ARCHIVED")
    );
    std::fs::rename(active_rollout, &rollout).unwrap();
    let other = temp.path().join("foreign-home");
    std::fs::create_dir(&other).unwrap();
    let foreign_script = script.replace(
        &format!("codexHome:{}", json!(root.to_str().unwrap())),
        &format!("codexHome:{}", json!(other.to_str().unwrap())),
    );
    assert_ne!(foreign_script, script);
    let descendant_script = script.replace(
        "()=>process.exit(0)",
        "()=>{setTimeout(()=>{try{const p=require('node:path');const r=p.dirname(p.dirname(file));fs.renameSync(p.join(r,'sessions'),p.join(r,'escaped-container'));fs.writeFileSync(marker+'.late','escaped');}catch{fs.writeFileSync(marker+'.late','held');}process.exit(0);},500);}",
    );
    let launcher=format!("require('node:child_process').spawn(process.execPath,['-e',{}],{{stdio:'inherit',detached:true}}).unref();setTimeout(()=>process.exit(0),2000);",json!(descendant_script));
    for (variant, expected, expected_calls, present) in [
        (foreign_script, "blocked", 1, true),
        (script.replace("const m=JSON.parse(line);","const m=JSON.parse(line);if(m.method==='thread/read')process.stdout.write(JSON.stringify({method:'turn/started',params:{}})+'\\n');"),"blocked",1,true),
        (
            script.replace("fs.unlinkSync(file);", ""),
            "partial",
            2,
            true,
        ),
        (launcher, "acknowledged", 3, false),
        (
            script.replace(
                "fs.unlinkSync(file);",
                "fs.unlinkSync(file);process.exit(0);",
            ),
            "unknown",
            4,
            false,
        ),
    ] {
        if !rollout.exists() {
            std::fs::write(&rollout, &transcript).unwrap();
        }
        std::fs::write(project.join("app-server"), variant).unwrap();
        let request = PreviewRequest {
            operation_id: uuid::Uuid::new_v4().to_string(),
            ..request.clone()
        };
        let preview = service.preview_delete(&caller, request.clone()).unwrap();
        assert_eq!(preview.state, "ready");
        let confirm = ConfirmRequest {
            operation_id: request.operation_id.clone(),
            preview_id: preview.preview_id.unwrap(),
            acknowledge_permanent_delete: true,
            acknowledge_native_subtree: true,
        };
        let receipt = service.confirm_delete(&caller, confirm.clone()).unwrap();
        assert_eq!(
            receipt.state,
            expected,
            "synthetic trace {:?}",
            std::fs::read_to_string(marker.with_extension("trace"))
        );
        assert_eq!(rollout.exists(), present);
        assert_eq!(receipt.root_present, Some(present));
        assert_eq!(
            service.confirm_delete(&caller, confirm).unwrap().state,
            expected
        );
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap().lines().count(),
            expected_calls
        );
        let inspected = service
            .inspect_delete(
                &caller,
                InspectRequest {
                    operation_id: request.operation_id,
                },
            )
            .unwrap();
        if expected == "acknowledged" {
            std::thread::sleep(std::time::Duration::from_millis(800));
            assert_eq!(
                std::fs::read_to_string(marker.with_extension("late")).unwrap(),
                "held",
                "launcher descendant escaped a released container lease"
            );
        }
        if expected == "unknown" {
            assert_eq!(inspected.state, if present {"partial"} else {"unknown"});
        }
        assert!(repo.admission().check_metadata_idle().is_ok());
    }
}

#[test]
#[cfg(windows)]
fn NativeDelete_ContainerLeaseRejectsReparseAndPinsUntilReap_005() {
    use crate::cli::native_projection::scoped_fs::Root;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let other = temp.path().join("other");
    std::fs::create_dir_all(root.join("sessions")).unwrap();
    std::fs::create_dir(root.join("archived_sessions")).unwrap();
    std::fs::create_dir(&other).unwrap();
    let authority = Root::open(&root).unwrap();
    let held = authority.pin_delete_containers().unwrap();
    assert!(std::fs::rename(root.join("sessions"), root.join("moved")).is_err());
    drop(held);
    std::fs::rename(root.join("sessions"), root.join("moved")).unwrap();
    let mut command = std::process::Command::new("cmd.exe");
    crate::platform::configure_command(&mut command);
    let status = command
        .args(["/D", "/C", "mklink", "/J"])
        .arg(root.join("sessions"))
        .arg(&other)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(authority.pin_delete_containers().is_err());
}

#[test]
fn NativeDelete_ExpiredBatchTicketNeverRenewsOrClaims_083() {
    let operations = Operations::default();
    let caller = caller();
    let request = request();
    let preview = operations
        .register(
            &caller,
            &request,
            "70000000-0000-4000-8000-000000000004",
            "same".into(),
        )
        .unwrap();
    operations.0.lock().unwrap()[0].created =
        std::time::Instant::now() - std::time::Duration::from_secs(121);
    let confirm = ConfirmRequest {
        operation_id: request.operation_id.clone(),
        preview_id: preview.preview_id.unwrap(),
        acknowledge_permanent_delete: true,
        acknowledge_native_subtree: true,
    };
    assert_eq!(
        operations
            .begin(&caller, &confirm, "same")
            .unwrap()
            .unwrap()
            .state,
        "blocked"
    );
    assert_eq!(
        operations
            .begin(&caller, &confirm, "same")
            .unwrap()
            .unwrap()
            .state,
        "blocked"
    );
    assert!(operations
        .get(&caller, &request.operation_id)
        .unwrap()
        .confirmed
        .is_none());
}

#[test]
fn NativeDelete_SlowPreWriteValidationCannotDispatchAfterDeadline_084() {
    let deadline = Instant::now() + Duration::from_millis(500);
    let checked = std::sync::atomic::AtomicBool::new(false);
    let mut wire = Vec::new();
    let mut started = false;
    let failure = exchange(
        "70000000-0000-4000-8000-000000000004",
        &mut |packet| send_packet(&mut wire, &packet, &|| Ok(()), &mut started, deadline),
        &mut |id| Ok(json!({"id":id,"result":if id==2 { json!({"thread":{}}) } else {json!({})}})),
        &|_| {
            checked.store(true, std::sync::atomic::Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(550));
            Ok(())
        },
        &|_| Ok(()),
    )
    .unwrap_err();
    assert!(!started);
    assert!(checked.load(std::sync::atomic::Ordering::Relaxed));
    let packets: Vec<serde_json::Value> = String::from_utf8(wire)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(!packets
        .iter()
        .any(|packet| packet["method"] == "thread/delete"));
    assert_eq!(pre_dispatch_failure(failure).code, "DELETE_PREPARE_FAILED");
}
