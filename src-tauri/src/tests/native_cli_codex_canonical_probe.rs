//! Explicit isolated diagnostic. No task, user profile, account or raw output receipt.
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
    fixture.supervisor.shutdown();
    fixture.wait_registry_retired();
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
