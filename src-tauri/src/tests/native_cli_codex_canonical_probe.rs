//! Explicit isolated diagnostic. No task, user profile, account or raw output receipt.
#![cfg(feature = "packaged-canonical-diagnostic")]
use super::*;
#[test]
#[ignore = "explicit installed Codex path and empty-home diagnostic only"]
fn CodexCanonical_EmptyHomeOriginalNative_001() {
    let exe = std::env::var("CC_DESK_CANONICAL_CODEX_EXE").expect("explicit Codex executable");
    assert!(Path::new(&exe).is_absolute());
    use sha2::{Digest, Sha256};
    let expected =
        std::env::var("CC_DESK_CANONICAL_CODEX_SHA256").expect("explicit reviewed binary digest");
    assert_eq!(expected.len(), 64);
    assert!(expected.bytes().all(|b| b.is_ascii_hexdigit()));
    let actual = format!("{:x}", Sha256::digest(std::fs::read(&exe).unwrap()));
    assert_eq!(
        actual,
        expected.to_ascii_lowercase(),
        "reviewed same-version binary required"
    );
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let mut fixture = Fixture::with_runtime_environment("hold", supervisor, transports, true);
    let mut profile = Profile::new("canonical-codex-empty", CliKind::Codex);
    profile.program_path = Override::Set(exe);
    // Fixed non-secret environment contrasts only; never inherit the user's environment.
    let environment_case =
        std::env::var("CC_DESK_CANONICAL_ENV_CASE").unwrap_or_else(|_| "minimal".into());
    assert!(matches!(
        environment_case.as_str(),
        "minimal" | "hosted-gui"
    ));
    if environment_case == "hosted-gui" {
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        assert_eq!(std::env::var("RUNNER_OS").as_deref(), Ok("Windows"));
        // Only fresh hosted OS directories and PATH; no arbitrary environment or credentials.
        for key in [
            "PATH",
            "USERPROFILE",
            "HOME",
            "APPDATA",
            "LOCALAPPDATA",
            "TEMP",
            "TMP",
        ] {
            if let Ok(value) = std::env::var(key) {
                profile.env.insert(
                    key.into(),
                    Override::Set(crate::cli::profiles::EnvValue::Literal {
                        value,
                        non_secret: true,
                    }),
                );
            }
        }
        profile.env.insert(
            "CI".into(),
            Override::Set(crate::cli::profiles::EnvValue::Literal {
                value: "true".into(),
                non_secret: true,
            }),
        );
    }
    if environment_case == "hosted-gui" {
        let home = fixture.root.path().join("empty-codex-home");
        std::fs::create_dir(&home).unwrap();
        profile.env.insert(
            "CODEX_HOME".into(),
            Override::Set(crate::cli::profiles::EnvValue::Literal {
                value: home.to_str().unwrap().into(),
                non_secret: true,
            }),
        );
        let sqlite = fixture.root.path().join("empty-codex-sqlite");
        std::fs::create_dir(&sqlite).unwrap();
        profile.env.insert(
            "CODEX_SQLITE_HOME".into(),
            Override::Set(crate::cli::profiles::EnvValue::Literal {
                value: sqlite.to_str().unwrap().into(),
                non_secret: true,
            }),
        );
    }
    eprintln!("TEST_CODEX_ENV case={environment_case}");
    for (key, value) in [("TERM", "xterm-256color"), ("COLORTERM", "truecolor")] {
        profile.env.insert(
            key.into(),
            Override::Set(crate::cli::profiles::EnvValue::Literal {
                value: value.into(),
                non_secret: true,
            }),
        );
    }
    let repository = fixture.service.repository();
    let doc = repository
        .apply(
            repository.read().unwrap().revision,
            Patch::Create {
                profile: profile.clone(),
            },
        )
        .unwrap();
    fixture.request.profile_id = profile.id.clone();
    fixture.request.expected_profile_revision = doc.profiles[&profile.id].revision;
    fixture.request.action = LaunchAction::Raw {
        argv: vec![
            "--sandbox".into(),
            "read-only".into(),
            "--ask-for-approval".into(),
            "on-request".into(),
        ],
    };
    let status = fixture.start();
    let end = Instant::now() + Duration::from_secs(8);
    let mut last = "NO_OBSERVATION".to_string();
    let mut every_scope_available = true;
    while Instant::now() < end {
        let observed = fixture
            .service
            .access(&fixture.caller, &status.run)
            .and_then(|access| fixture.supervisor.admit_remote_run(access, &status.run))
            .and_then(|admission| admission.scope());
        let code = match observed {
            Ok(_) => "OK".to_string(),
            Err(e) => e.code.to_string(),
        };
        every_scope_available &= code == "OK";
        if code != last {
            eprintln!("TEST_CODEX_CANONICAL scope={code}");
            last = code;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let (read_bytes, read_calls, processed_bytes, processed_calls, counters_valid) =
        cc_desk_remote_host::packaged_diagnostic::processing_totals();
    eprintln!("TEST_CODEX_PROCESSING phase=before_shutdown read_bytes={read_bytes} read_calls={read_calls} processed_bytes={processed_bytes} processed_calls={processed_calls} counters_valid={counters_valid}");
    fixture.supervisor.shutdown();
    fixture.wait_registry_retired();
    let identity = crate::platform::admitted_child::test_identity_verified();
    let (read_bytes, read_calls, processed_bytes, processed_calls, counters_valid) =
        cc_desk_remote_host::packaged_diagnostic::processing_totals();
    let events = fixture.events.lock();
    let mut route_bytes = 0u64;
    let mut route_frames = 0u64;
    for event in events.iter() {
        if let Some(bytes) = event.get("bytes").and_then(Value::as_array) {
            route_bytes = route_bytes.checked_add(bytes.len() as u64).unwrap();
            route_frames += 1;
        }
    }
    drop(events);
    eprintln!(
        "TEST_CODEX_PROCESSING identity={identity} read_bytes={read_bytes} read_calls={read_calls} processed_bytes={processed_bytes} processed_calls={processed_calls} counters_valid={counters_valid} route_bytes={route_bytes} route_frames={route_frames}"
    );
    // This path has no Wry invoke/layout resize or GUI ACK. Registry retirement
    // does not attest all ConPTY IO threads or descendants.
    assert!(identity, "original handle identity unproven");
    assert!(
        counters_valid && read_bytes > 0 && processed_bytes > 0 && processed_calls > 0,
        "no valid nonzero reader/parser acceptance"
    );
    assert_eq!(
        read_bytes, processed_bytes,
        "not all original ingress bytes accepted"
    );
    eprintln!(
        "TEST_CODEX_CANONICAL observed={last} task_sent=false raw_output_logged=false full_process_tree_retired=unproven"
    );
    assert_eq!(
        last, "OK",
        "actual isolated startup did not retain canonical scope"
    );
    assert!(
        every_scope_available,
        "canonical scope was lost during the observation window"
    );
}
