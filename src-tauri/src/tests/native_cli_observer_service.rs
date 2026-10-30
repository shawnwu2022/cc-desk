// Uses the actual D11 launch fixture, supervisor and native PTY, not a fake spawn.
use super::*;
use crate::observer_host::ObserverHost;
use crate::observer_registry::{ObserverBinding, ObserverRegistry, ObserverRun, ObserverSource};
use std::ffi::OsStr;

// 自有 shim 记录全部启动；不同版本文本不得触发额外探测，实际插件经真实 HTTP 鉴权交付。
#[cfg(windows)]
#[test]
fn Opencode_SameRunWithoutVersionProbe_125() {
    opencode_same_run_fixture(
        &[(0, "1.18.0"), (1, "1.19.0"), (2, "future+shape"), (3, "")],
        Duration::ZERO,
    );
}

// 单次响应低于生产 700ms 期限；完整序列不能被更短的无关总表杀掉。
#[cfg(windows)]
#[test]
fn Opencode_SameRunBoundedResponses_132() {
    opencode_same_run_fixture(&[(3, "")], Duration::from_millis(550));
}

#[cfg(windows)]
fn opencode_same_run_fixture(cases: &[(usize, &str)], response_delay: Duration) {
    use crate::cli::profiles::{Dialect, Launcher};
    for (index, version) in cases {
        let mut f = Fixture::new(false);
        let script = f.root.path().join("owned-opencode.mjs");
        fs::write(&script, r#"
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {join} from 'node:path';
const root=process.env.CC_DESK_TEST_ROOT;
assert(root && fs.realpathSync(root)===fs.realpathSync(process.cwd()));
let phase='configuration', requests=0;
const startedAt=performance.now(), statuses=[], eventIDs=[], elapsedMs=[];
// 自有 fixture 的固定阶段/数字；不记录环境、argv、nonce 或请求正文。
process.once('exit',code=>fs.writeFileSync(join(root,'diagnostic.json'),JSON.stringify({code,phase,requests,statuses,eventIDs,elapsedMs,totalMs:Math.round(performance.now()-startedAt),nodeVersion:process.versions.node})));
const originalFetch=globalThis.fetch;
globalThis.fetch=async (...args)=>{ requests++; eventIDs.push(args[1]?.headers?.['x-cc-desk-event']); const start=performance.now();try { const response=await originalFetch(...args);statuses.push(response.status);return response } catch(error) { statuses.push(0);throw error } finally { elapsedMs.push(Math.round(performance.now()-start)) } };
fs.appendFileSync(join(root,'calls.log'),JSON.stringify(process.argv.slice(2))+'\n');
if(process.argv.includes('--version')) { process.stdout.write(process.env.C4_VERSION); process.exit(0); }
// 固定9个事件产生10次串行发送；同run最多两次重送，按12×700ms另加5s宿主调度余量。
// 5s沿用旧总保护数值，不代表已有独立阶段合同或产品 SLA。
// 这是 fixture 防挂保护，不改变生产单请求期限或成功断言。
const fixtureBudgetMs=5000+12*700;
const timeout=setTimeout(()=>process.exit(42),fixtureBudgetMs); timeout.unref();
const config=JSON.parse(process.env.OPENCODE_CONFIG_CONTENT ?? '{}');
assert.equal(config.future.keep,true);
assert.deepEqual(config.plugin[0],['user-kept',{enabled:false,nested:{keep:true}}]);
assert.equal(config.plugin[1],'second-kept');
assert.equal(config.plugin.length,3);
const [url,options]=config.plugin[2];
phase='import';
const mod=await import(url);
const id=options.sessionID ?? 'ses_same_run';
const metadata={id,directory:options.directory};
phase='initialization';
const plugin=await mod.CCDeskObserver({directory:options.directory,client:{session:{get:async()=>({data:metadata})}}},options);
const emit=(type,properties,key=type)=>{phase=key;return plugin.event({event:{id:key,type,properties}})};
await emit('session.created',{info:{...metadata,future:'ignored'}});
await emit('session.status',{sessionID:id,status:{type:'busy'}},'busy');
await emit('permission.asked',{sessionID:id,id:'permission'});
await emit('permission.replied',{sessionID:id,requestID:'permission'});
await emit('question.asked',{sessionID:id,id:'question'});
await emit('question.replied',{sessionID:id,requestID:'question'});
await emit('session.error',{sessionID:id});
await emit('session.status',{sessionID:id,status:{type:'idle'}},'idle');
await emit('session.status',{sessionID:id,status:{type:'retry'}},'retry');
plugin.dispose();
phase='done';
process.exit(0);
"#).unwrap();
        let program = f.root.path().join("owned-opencode.cmd");
        let node = crate::platform::find_executable("node").unwrap();
        fs::write(
            &program,
            format!("@\"{node}\" \"{}\" %*\r\n", script.display()),
        )
        .unwrap();
        let mut profile = Profile::new("opencode-same-run", CliKind::Opencode);
        profile.program_path = Override::Set(program.to_str().unwrap().into());
        profile.launcher = Launcher::Shim {
            runner: std::env::var("COMSPEC").unwrap(),
            dialect: Dialect::Cmd,
        };
        profile.observer = Override::Set(true);
        let doc = f.repository.read().unwrap();
        let doc = f
            .repository
            .apply(
                doc.revision,
                Patch::Create {
                    profile: profile.clone(),
                },
            )
            .unwrap();
        f.request.cli = CliKind::Opencode;
        f.request.profile_id = profile.id.clone();
        f.request.expected_profile_revision = doc.profiles[&profile.id].revision;
        f.request.action = if index % 2 == 0 {
            LaunchAction::New
        } else {
            LaunchAction::ResumeId {
                native_session_id: "ses_same_run".into(),
            }
        };
        let registry = Arc::new(ObserverRegistry::new());
        let received = Arc::new(Mutex::new(Vec::new()));
        let output = received.clone();
        let app = crate::observer_http::router(
            registry.clone(),
            Arc::new(move |_, payload| {
                output.lock().push(payload);
                Ok(())
            }),
        );
        let app = if response_delay.is_zero() {
            app
        } else {
            app.layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, next: axum::middleware::Next| async move {
                    let response = next.run(request).await;
                    tokio::time::sleep(response_delay).await;
                    response
                },
            ))
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            // 保持原测试的两个 HTTP worker，不借故改变调度或生产期限。
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let (shutdown, ended) = tokio::sync::oneshot::channel::<()>();
        let server = runtime.spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = ended.await;
                })
                .await
                .unwrap();
        });
        let host = Arc::new(ObserverHost::new(
            registry,
            f.root.path().join("unused"),
            Arc::new(move || Some(port)),
        ));
        let mut env = EnvMap::new();
        for key in [
            "PATH",
            "SystemRoot",
            "WINDIR",
            "ComSpec",
            "PATHEXT",
            "TEMP",
            "TMP",
        ] {
            if let Some(value) = std::env::var_os(key) {
                env.insert(key.into(), value);
            }
        }
        env.insert(
            "CC_DESK_TEST_ROOT".into(),
            f.request.launch_cwd.clone().into(),
        );
        env.insert("C4_VERSION".into(), (*version).into());
        env.insert("OPENCODE_DB".into(), ":memory:".into());
        env.insert("OPENCODE_CONFIG_CONTENT".into(), r#"{"plugin":[["user-kept",{"enabled":false,"nested":{"keep":true}}],"second-kept"],"future":{"keep":true}}"#.into());
        f.service = Arc::new(
            LaunchService::new(f.repository.clone(), Some(env), Some(f.consumer.clone()))
                .with_observer(host),
        );
        f.caller = f.service.registry().activate_window("main").unwrap();
        let status = f.start().unwrap();
        let child = f.consumer.runs.lock()[0].clone();
        let exit = child.process.pty.wait().unwrap();
        let _ = shutdown.send(());
        runtime.block_on(server).unwrap();
        let diagnostic =
            fs::read_to_string(Path::new(&f.request.launch_cwd).join("diagnostic.json"))
                .unwrap_or_else(|_| "missing".into());
        let snapshot = f
            .service
            .access(&f.caller, &status.run)
            .unwrap()
            .snapshot()
            .unwrap();
        assert!(
            snapshot
                .environment()
                .contains_key(OsStr::new("CC_DESK_OBSERVER_CAPABILITY")),
            "version text {version:?} silently removed the same-run plugin"
        );
        assert!(
            exit.success(),
            "owned plugin fixture did not complete for version text {version:?}: {exit:?}; diagnostic={}",
            diagnostic
        );
        let diagnostic: serde_json::Value = serde_json::from_str(&diagnostic).unwrap();
        assert_eq!(diagnostic["code"], 0);
        assert_eq!(diagnostic["phase"], "done");
        let statuses = diagnostic["statuses"].as_array().unwrap();
        let event_ids = diagnostic["eventIDs"].as_array().unwrap();
        assert_eq!(diagnostic["requests"], statuses.len());
        assert_eq!(event_ids.len(), statuses.len());
        assert!(
            statuses.len() <= 12,
            "transport retry budget exceeded: {diagnostic}"
        );
        assert_eq!(
            statuses.iter().filter(|status| **status == 200).count(),
            10,
            "ten authenticated responses required: {diagnostic}"
        );
        assert!(
            statuses.iter().all(|status| *status == 0 || *status == 200),
            "server rejected an event: {diagnostic}"
        );
        for (index, status) in statuses.iter().enumerate().take(statuses.len() - 1) {
            if *status == 0 {
                assert_eq!(
                    event_ids[index],
                    event_ids[index + 1],
                    "failed event was not retried with the same ID: {diagnostic}"
                );
            }
        }
        let calls = fs::read_to_string(Path::new(&f.request.launch_cwd).join("calls.log")).unwrap();
        assert_eq!(
            calls.lines().count(),
            1,
            "an auxiliary CLI invocation was made: {calls}"
        );
        assert!(!calls.contains("--version"));
        assert_eq!(f.start().unwrap(), status);
        assert_eq!(f.consumer.calls.load(Ordering::SeqCst), 1);
        let events = received.lock();
        assert_eq!(
            events.len(),
            10,
            "all ten distinct events must be delivered exactly once: {diagnostic}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_name == "SessionStart")
                .count(),
            1
        );
        assert!(events
            .iter()
            .all(|event| event.session_id.as_deref() == Some("ses_same_run")));
        let states = events
            .iter()
            .filter_map(|event| match &event.detail {
                crate::hook_events::HookEventDetail::Notification(detail) => {
                    detail.notification_type.as_deref()
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(states.contains(&"opencode_busy"));
        assert!(states.contains(&"opencode_permission"));
        assert!(states.contains(&"opencode_question"));
        assert_eq!(
            states
                .iter()
                .filter(|value| **value == "opencode_error")
                .count(),
            2
        );
        assert_eq!(states.last(), Some(&"opencode_retry"));
        assert!(!events.iter().any(|event| event.event_name == "Stop"));
    }
}

#[cfg(windows)]
#[test]
fn CodexIdentity_CmdShimOptionalHooksNeverBlockNewOrResume_046() {
    use crate::cli::profiles::{Dialect, Launcher};
    // 可选 hook 不可让原本可表示的 npm cmd 启动失败；保留严格 CALL 参数限制。
    for action in [
        LaunchAction::New,
        LaunchAction::ResumeId {
            native_session_id: "00000000-0000-4000-8000-000000000009".into(),
        },
    ] {
        let mut fixture = Fixture::new(false);
        let node = crate::platform::find_executable("node").unwrap();
        let probe = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/native-cli/owned-probe.mjs");
        let shim = fixture.root.path().join("codex.cmd");
        fs::write(
            &shim,
            format!("@\"{node}\" \"{}\" hold %*\r\n", probe.display()),
        )
        .unwrap();
        let document = fixture.repository.read().unwrap();
        let updated = fixture.repository.apply(document.revision, Patch::Update {
            id: fixture.request.profile_id.clone(),
            changes: serde_json::from_value(serde_json::json!({
                "programPath": {"mode":"set", "value":shim},
                "launcher": Launcher::Shim { runner: std::env::var("COMSPEC").unwrap(), dialect: Dialect::Cmd }
            })).unwrap(),
        }).unwrap();
        fixture.request.expected_profile_revision =
            updated.profiles[&fixture.request.profile_id].revision;
        fixture.request.action = action;
        let host = Arc::new(ObserverHost::new(
            Arc::new(ObserverRegistry::new()),
            fixture.root.path().join("unused-plugin"),
            Arc::new(|| Some(12345)),
        ));
        fixture.service = Arc::new(
            LaunchService::new(
                fixture.repository.clone(),
                Some(fixture.service.inherited_environment().unwrap()),
                Some(fixture.consumer.clone()),
            )
            .with_observer(host),
        );
        fixture.caller = fixture.service.registry().activate_window("main").unwrap();
        let status = fixture
            .start()
            .expect("optional Codex hooks must not block cmd launch");
        assert_eq!(status.phase, LaunchPhase::Running);
        fixture.ready();
        let snapshot = fixture
            .service
            .access(&fixture.caller, &status.run)
            .unwrap()
            .snapshot()
            .unwrap();
        assert!(!snapshot
            .environment()
            .contains_key(OsStr::new("CC_DESK_CODEX_CAPABILITY")));
        let invocation =
            crate::cli::invocation::build_invocation(&fixture.request, &snapshot).unwrap();
        assert!(!invocation
            .args()
            .iter()
            .any(|arg| arg.to_string_lossy().starts_with("hooks.")));
        assert_eq!(fixture.children(), 1);
    }
}

fn observed_fixture(enabled: bool) -> (Fixture, Arc<ObserverRegistry>) {
    let mut fixture = Fixture::new(false);
    let plugin = fixture.root.path().join("observer-plugin");
    crate::hook_config::ensure_plugin_files_at(&plugin).unwrap();
    let registry = Arc::new(ObserverRegistry::new());
    let host = Arc::new(ObserverHost::new(
        registry.clone(),
        plugin,
        Arc::new(|| Some(12345)),
    ));
    let mut profile = fixture
        .repository
        .get_profile(&fixture.request.profile_id)
        .unwrap();
    // Register a separate Claude profile; changing a profile's CLI is not permitted.
    profile.id = "observed-claude".into();
    profile.revision = WireU64::parse("0").unwrap();
    profile.cli = CliKind::Claude;
    profile.observer = Override::Set(enabled);
    let LaunchAction::Raw { argv } = &fixture.request.action else {
        unreachable!()
    };
    profile.default_args = Override::Set(argv.clone());
    let revision = fixture.repository.read().unwrap().revision;
    let stored = fixture
        .repository
        .apply(
            revision,
            Patch::Create {
                profile: profile.clone(),
            },
        )
        .unwrap();
    fixture.request.profile_id = profile.id.clone();
    fixture.request.expected_profile_revision = stored.profiles[&profile.id].revision;
    fixture.request.cli = CliKind::Claude;
    fixture.request.action = LaunchAction::New;
    fixture.service = Arc::new(
        LaunchService::new(
            fixture.repository.clone(),
            Some(fixture.service.inherited_environment().unwrap()),
            Some(fixture.consumer.clone()),
        )
        .with_observer(host),
    );
    fixture.caller = fixture.service.registry().activate_window("main").unwrap();
    (fixture, registry)
}

#[test]
fn D13_Service_RealRunOwnsCredentialReplayAndDocumentRevocation_001() {
    let (fixture, registry) = observed_fixture(true);
    // Node is a native argv/PTY probe, not a real Claude process. Keep its script
    // before Desk's plugin option so the fixture can execute as a structured run.
    let status = fixture.start().unwrap();
    fixture.ready();
    let access = fixture
        .service
        .access(&fixture.caller, &status.run)
        .unwrap();
    let snapshot = access.snapshot().unwrap();
    let env = snapshot.environment();
    let binding = ObserverBinding {
        run: ObserverRun {
            run_id: status.run.run_id.clone(),
            generation: status.run.generation,
        },
        capability: env[OsStr::new("CC_DESK_OBSERVER_CAPABILITY")]
            .to_str()
            .unwrap()
            .into(),
        source: ObserverSource::ClaudeHook,
    };
    assert!(registry
        .accept_event(
            &binding,
            "first",
            br#"{"hook_event_name":"SessionStart","session_id":"known"}"#
        )
        .is_ok());
    assert_eq!(fixture.start().unwrap(), status);
    assert_eq!(fixture.children(), 1);
    let again = access.snapshot().unwrap();
    assert_eq!(again.environment(), snapshot.environment());
    let document = fixture.repository.read().unwrap();
    fixture
        .repository
        .apply(
            document.revision,
            Patch::Delete {
                id: fixture.request.profile_id.clone(),
            },
        )
        .unwrap();
    assert!(registry.check_binding(&binding).is_ok());
    fixture
        .service
        .registry()
        .revoke_window(&fixture.caller)
        .unwrap();
    assert!(registry.check_binding(&binding).is_err());
    // Revoking observation/document authority must not terminate the actual child.
    assert!(fixture.consumer.runs.lock()[0]
        .process
        .pty
        .try_wait()
        .unwrap()
        .is_none());
}

#[test]
fn D13_Service_RawAndOffNeverInjectObserverOrRequireItsService_002() {
    for raw in [false, true] {
        let (mut fixture, _) = observed_fixture(raw);
        if raw {
            let profile = fixture
                .repository
                .get_profile(&fixture.request.profile_id)
                .unwrap();
            let Override::Set(argv) = profile.default_args else {
                unreachable!()
            };
            fixture.request.action = LaunchAction::Raw { argv };
        }
        let status = fixture.start().unwrap();
        fixture.ready();
        let query = crate::cli::observer_status::ObserverStatusRequest {
            request_id: status.request_id.clone(),
            run_id: status.run.run_id.clone(),
            generation: status.run.generation,
        };
        let projected = fixture
            .service
            .observer_status(&fixture.caller, &query)
            .unwrap();
        assert_eq!(projected.state, "off");
        assert_eq!(
            projected.reason_code,
            if raw {
                "OBSERVER_UNSUPPORTED"
            } else {
                "OBSERVER_DISABLED"
            }
        );
        let snapshot = fixture
            .service
            .access(&fixture.caller, &status.run)
            .unwrap()
            .snapshot()
            .unwrap();
        assert!(!snapshot
            .environment()
            .contains_key(OsStr::new("CC_DESK_OBSERVER_CAPABILITY")));
        assert!(
            !crate::cli::invocation::build_invocation(&fixture.request, &snapshot)
                .unwrap()
                .args()
                .contains(&"--plugin-dir".into())
        );
    }
}

#[test]
fn ObserverStatus_FrozenInstallationAndExactReceipt_003() {
    let (fixture, _) = observed_fixture(true);
    let status = fixture.start().unwrap();
    fixture.ready();
    let query = crate::cli::observer_status::ObserverStatusRequest {
        request_id: status.request_id.clone(),
        run_id: status.run.run_id.clone(),
        generation: status.run.generation,
    };
    let projected = fixture
        .service
        .observer_status(&fixture.caller, &query)
        .unwrap();
    assert_eq!(projected.reason_code, "OBSERVER_CONNECTING");
    let mut stale = query;
    stale.request_id = "other-request".into();
    assert!(fixture
        .service
        .observer_status(&fixture.caller, &stale)
        .is_err());
}

#[test]
fn ObserverStatus_ServiceAndPluginInitializationFailures_004() {
    for (service, plugin, expected) in [
        (false, true, "OBSERVER_SERVICE_UNAVAILABLE"),
        (true, false, "OBSERVER_PLUGIN_UNAVAILABLE"),
    ] {
        let (mut fixture, _) = observed_fixture(true);
        let path = fixture.root.path().join("status-plugin");
        if plugin {
            crate::hook_config::ensure_plugin_files_at(&path).unwrap();
        }
        let host = Arc::new(ObserverHost::new(
            Arc::new(ObserverRegistry::new()),
            path,
            Arc::new(move || service.then_some(12345)),
        ));
        fixture.service = Arc::new(
            LaunchService::new(
                fixture.repository.clone(),
                Some(fixture.service.inherited_environment().unwrap()),
                Some(fixture.consumer.clone()),
            )
            .with_observer(host),
        );
        fixture.caller = fixture.service.registry().activate_window("main").unwrap();
        let status = fixture.start().unwrap();
        fixture.ready();
        let query = crate::cli::observer_status::ObserverStatusRequest {
            request_id: status.request_id.clone(),
            run_id: status.run.run_id.clone(),
            generation: status.run.generation,
        };
        let projected = fixture
            .service
            .observer_status(&fixture.caller, &query)
            .unwrap();
        assert_eq!(projected.reason_code, expected);
        assert_eq!(fixture.children(), 1);
    }
}
