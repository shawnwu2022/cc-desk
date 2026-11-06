#![allow(non_snake_case)]
use super::*;
use crate::cli::environment::build_environment;
use crate::cli::profiles::Profile;
use crate::cli::storage::Patch;
use std::path::PathBuf;

struct ProbeFixture {
    service: LaunchService,
    caller: CallerIdentity,
    profile: Profile,
    environment: EnvMap,
    request: LaunchRequest,
    database: PathBuf,
    root: tempfile::TempDir,
}

impl ProbeFixture {
    fn new(explicit: Option<&str>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data/opencode");
        std::fs::create_dir_all(&data).unwrap();
        let database = data.join(explicit.unwrap_or("opencode.db"));
        let repository = WorkspaceRepository::open(root.path().join("workspace.json")).unwrap();
        let mut profile = Profile::new("probe-fixture", CliKind::Opencode);
        profile.program_path = Override::Set(crate::platform::find_executable("node").unwrap());
        profile.observer = Override::Set(true);
        repository
            .apply(
                repository.read().unwrap().revision,
                Patch::Create {
                    profile: profile.clone(),
                },
            )
            .unwrap();
        let profile = repository.get_profile(&profile.id).unwrap();
        let mut inherited = EnvMap::from([(
            "XDG_DATA_HOME".into(),
            root.path().join("data").into_os_string(),
        )]);
        if explicit.is_some() {
            inherited.insert("OPENCODE_DB".into(), database.as_os_str().into());
        }
        if let Some(value) = std::env::var_os("SystemRoot") {
            inherited.insert("SystemRoot".into(), value);
        }
        let environment =
            build_environment(&inherited, &EnvMap::new(), &profile, None, None).unwrap();
        let host = crate::observer_host::ObserverHost::new(
            Arc::new(crate::observer_registry::ObserverRegistry::with_capacity(8)),
            PathBuf::new(),
            Arc::new(|| None),
        );
        let service =
            LaunchService::new(repository, Some(inherited), None).with_observer(Arc::new(host));
        let caller = service.registry().activate_window("main").unwrap();
        let request = LaunchRequest {
            request_id: "probe-request".into(),
            tab_id: "probe-tab".into(),
            run_id: "probe-run".into(),
            generation: 1,
            profile_id: profile.id.clone(),
            expected_profile_revision: profile.revision,
            cli: CliKind::Opencode,
            launch_cwd: root.path().to_str().unwrap().into(),
            action: LaunchAction::ResumeId {
                native_session_id: "ses_probe".into(),
            },
            extra_args: vec![],
            cols: 80,
            rows: 24,
        };
        let f = Self {
            service,
            caller,
            profile,
            environment,
            request,
            database,
            root,
        };
        f.output("debug", &f.database.to_string_lossy());
        f.output("db", &f.database.to_string_lossy());
        f
    }

    fn output(&self, command: &str, output: &str) {
        // Node 作为所选程序，真实执行 debug/db 两个入口，记录命令顺序。
        std::fs::write(self.root.path().join(command), format!(
            "const fs=require('node:fs');fs.appendFileSync('probe.calls',{}+'\\n');process.stdout.write({}+'\\n');",
            serde_json::to_string(command).unwrap(), serde_json::to_string(output).unwrap(),
        )).unwrap();
    }

    fn probe(&self) -> Result<String, SafeError> {
        self.service.probe_opencode_database(
            &self.caller,
            &self.profile,
            self.root.path(),
            &self.environment,
        )
    }

    fn snapshot(&self, request: &LaunchRequest) -> LaunchSnapshot {
        let profile = self
            .service
            .repository
            .get_profile(&request.profile_id)
            .unwrap();
        freeze_launch(
            request,
            &profile,
            &self.caller,
            &FreezeContext {
                inherited: self.service.inherited.as_ref().unwrap(),
                terminal: &EnvMap::new(),
                legacy: None,
                observer: None,
            },
        )
        .unwrap()
    }

    fn run_source(
        &self,
        request: &LaunchRequest,
    ) -> (
        super::super::source_provenance::SourceProvenance,
        Option<PathBuf>,
    ) {
        self.service
            .freeze_run_source(&self.caller, &self.snapshot(request), &EnvMap::new())
    }

    fn remember_v1(&self) {
        self.service.opencode_probe_argv.lock().insert(
            (self.profile.id.clone(), self.profile.revision.to_string()),
            ApiFlavor::V1,
        );
    }
}

#[test]
fn Opencode_ProbeRejectsInvalidCachedPathsAndRedecides_107() {
    for invalid in ["", "unknown command", "opencode.db", "opencode.db\nother"] {
        let f = ProbeFixture::new(None);
        f.output("db", invalid);
        f.remember_v1();
        assert_eq!(
            f.probe().unwrap().trim_end(),
            f.database.to_str().unwrap(),
            "invalid cached output: {invalid:?}"
        );
        assert_eq!(
            std::fs::read_to_string(f.root.path().join("probe.calls")).unwrap(),
            "db\ndebug\n"
        );
    }
    let f = ProbeFixture::new(None);
    f.output("db", &f.root.path().join("opencode.db").to_string_lossy());
    f.remember_v1();
    assert_eq!(f.probe().unwrap().trim_end(), f.database.to_str().unwrap());
}

#[test]
fn Opencode_DirectExplicitResumeEnablesCueWithoutNewSession_108() {
    let f = ProbeFixture::new(Some("custom.sqlite"));
    let (source, cue) = f.run_source(&f.request);
    assert_eq!(
        cue.as_deref(),
        Some(f.database.as_path()),
        "direct resume must not require an earlier New/prepare"
    );
    assert_eq!(
        source.selection().unwrap().root.sqlite_filename(),
        "custom.sqlite"
    );
}

#[test]
fn Opencode_ProfileRevisionCannotReuseOldPrepareFlavor_109() {
    let f = ProbeFixture::new(None);
    assert!(f.run_source(&f.request).1.is_some());
    f.output("debug", "unknown command");
    let doc = f.service.repository.read().unwrap();
    let doc = f
        .service
        .repository
        .apply(
            doc.revision,
            Patch::Update {
                id: f.profile.id.clone(),
                changes: serde_json::json!({"name":"changed program selection"})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
        )
        .unwrap();
    let mut request = f.request.clone();
    request.expected_profile_revision = doc.profiles[&f.profile.id].revision;
    assert!(
        f.run_source(&request).1.is_none(),
        "old v2 evidence must not enable a different profile revision running v1"
    );
    assert_ne!(request.expected_profile_revision, f.profile.revision);
}

#[test]
fn Opencode_SameProgramDowngradeDisablesCueDespiteCachedV2_110() {
    let f = ProbeFixture::new(Some("custom.sqlite"));
    assert!(f.run_source(&f.request).1.is_some());
    f.output("debug", "help\nunknown command");
    assert!(f.run_source(&f.request).1.is_none());
    f.output("db", "unknown command");
    f.output("debug", &f.database.to_string_lossy());
    assert!(f.run_source(&f.request).1.is_some());
}

#[test]
fn Opencode_ExplicitProbeFailurePreservesSourceWithoutCue_111() {
    let f = ProbeFixture::new(Some("custom.sqlite"));
    f.output(
        "debug",
        &f.root.path().join("opencode.db").to_string_lossy(),
    );
    f.output("db", "help");
    let (source, cue) = f.run_source(&f.request);
    assert!(cue.is_none());
    assert_eq!(
        source.selection().unwrap().root.sqlite_filename(),
        "custom.sqlite"
    );
    assert!(f.probe().is_err());
}

#[test]
fn Opencode_DisabledChannelAndChannelPathUseExactDatabase_112() {
    let mut f = ProbeFixture::new(None);
    f.service
        .inherited
        .as_mut()
        .unwrap()
        .insert("OPENCODE_DISABLE_CHANNEL_DB".into(), "1".into());
    assert_eq!(
        f.run_source(&f.request).1.as_deref(),
        Some(f.database.as_path())
    );
    f.service
        .inherited
        .as_mut()
        .unwrap()
        .remove(std::ffi::OsStr::new("OPENCODE_DISABLE_CHANNEL_DB"));
    let channel = f.database.with_file_name("opencode-beta.db");
    f.output("debug", &channel.to_string_lossy());
    let (source, cue) = f.run_source(&f.request);
    assert_eq!(cue.as_deref(), Some(channel.as_path()));
    assert_eq!(
        source.selection().unwrap().root.sqlite_filename(),
        "opencode-beta.db"
    );
}

#[test]
fn Opencode_ObserverOffUnknownArgsAndStaleSnapshotDoNotProbeCue_113() {
    let mut f = ProbeFixture::new(Some("custom.sqlite"));
    let snapshot = f.snapshot(&f.request);
    let doc = f.service.repository.read().unwrap();
    f.service
        .repository
        .apply(
            doc.revision,
            Patch::Update {
                id: f.profile.id.clone(),
                changes: serde_json::json!({"observer":{"mode":"set","value":false}})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
        )
        .unwrap();
    let (_, cue) = f
        .service
        .freeze_run_source(&f.caller, &snapshot, &EnvMap::new());
    assert!(cue.is_none());
    let mut request = f.request.clone();
    request.expected_profile_revision = f
        .service
        .repository
        .get_profile(&f.profile.id)
        .unwrap()
        .revision;
    assert!(f.run_source(&request).1.is_none());
    // raw 和未知参数各自独立禁止 cue 探测，不能被观察关闭的 gate 掩盖。
    let doc = f.service.repository.read().unwrap();
    let doc = f
        .service
        .repository
        .apply(
            doc.revision,
            Patch::Update {
                id: f.profile.id.clone(),
                changes: serde_json::json!({"observer":{"mode":"set","value":true}})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
        )
        .unwrap();
    request.expected_profile_revision = doc.profiles[&f.profile.id].revision;
    assert!(!f.root.path().join("probe.calls").exists());
    request.extra_args = vec!["--future-option".into()];
    assert!(f.run_source(&request).1.is_none());
    request.extra_args.clear();
    request.action = LaunchAction::Raw {
        argv: vec!["--help".into()],
    };
    assert!(f.run_source(&request).1.is_none());
    assert!(!f.root.path().join("probe.calls").exists());
    request.extra_args.clear();
    request.action = f.request.action.clone();
    f.service.observer = None;
    assert!(f.run_source(&request).1.is_none());
    assert!(!f.root.path().join("probe.calls").exists());
}

#[test]
fn Opencode_PureResumeDoesNotSpawnCueProbe_114() {
    for pure in ["1", "true"] {
        let mut f = ProbeFixture::new(Some("custom.sqlite"));
        f.service
            .inherited
            .as_mut()
            .unwrap()
            .insert("OPENCODE_PURE".into(), pure.into());
        let (source, cue) = f.run_source(&f.request);
        assert!(
            !f.root.path().join("probe.calls").exists(),
            "Pure={pure} must not spawn an observation metadata child"
        );
        assert!(cue.is_none(), "Pure={pure} must not authorize a watcher");
        assert_eq!(
            source.selection().unwrap().root.sqlite_filename(),
            "custom.sqlite"
        );
        assert_eq!(
            super::super::observer_status::installation_reason(
                &f.snapshot(&f.request),
                false,
                None
            ),
            "OBSERVER_DISABLED"
        );
    }
}

#[test]
fn Opencode_PureKeepsRequiredHistoryProbeAndNonPureCue_116() {
    let mut f = ProbeFixture::new(None);
    f.service
        .inherited
        .as_mut()
        .unwrap()
        .insert("OPENCODE_PURE".into(), "true".into());
    let (source, cue) = f.run_source(&f.request);
    assert!(cue.is_none());
    assert_eq!(
        source.selection().unwrap().root.sqlite_filename(),
        "opencode.db"
    );
    assert_eq!(
        std::fs::read_to_string(f.root.path().join("probe.calls")).unwrap(),
        "debug\n"
    );
    for pure in ["0", "false"] {
        let mut f = ProbeFixture::new(Some("custom.sqlite"));
        f.service
            .inherited
            .as_mut()
            .unwrap()
            .insert("OPENCODE_PURE".into(), pure.into());
        assert_eq!(
            f.run_source(&f.request).1.as_deref(),
            Some(f.database.as_path())
        );
    }
}
