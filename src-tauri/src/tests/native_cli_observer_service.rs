// Uses the actual D11 launch fixture, supervisor and native PTY, not a fake spawn.
use super::*;
use crate::observer_host::ObserverHost;
use crate::observer_registry::{ObserverBinding, ObserverRegistry, ObserverRun, ObserverSource};
use std::ffi::OsStr;

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
            Some(fixture.service.inherited_environment()),
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
