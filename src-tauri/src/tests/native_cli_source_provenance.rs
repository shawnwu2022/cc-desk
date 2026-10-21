#![allow(non_snake_case)]
use super::*;
use crate::cli::{
    environment::EnvMap,
    profiles::{Override, Profile},
    snapshot::{freeze_launch, CallerIdentity, FreezeContext},
    types::{LaunchRequest, WireU64},
};
fn frozen(root: &std::path::Path, cwd: &std::path::Path) -> LaunchSnapshot {
    let profile_id = "provenance";
    let mut profile = Profile::new(profile_id, CliKind::Codex);
    profile.program_path = Override::Set(std::env::current_exe().unwrap().to_str().unwrap().into());
    let revision = WireU64::parse("0").unwrap();
    let caller = CallerIdentity {
        instance_id: "fixture".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let request = LaunchRequest {
        request_id: "request".into(),
        tab_id: "tab".into(),
        run_id: "run".into(),
        generation: 1,
        profile_id: profile_id.into(),
        expected_profile_revision: revision,
        cli: CliKind::Codex,
        launch_cwd: cwd.to_str().unwrap().into(),
        action: LaunchAction::New,
        extra_args: vec![],
        cols: 80,
        rows: 24,
    };
    let mut environment = EnvMap::new();
    environment.insert("CODEX_HOME".into(), root.as_os_str().into());
    freeze_launch(
        &request,
        &profile,
        &caller,
        &FreezeContext {
            inherited: &environment,
            terminal: &EnvMap::new(),
            legacy: None,
            observer: None,
        },
    )
    .unwrap()
}

#[test]
fn SourceProvenance_OpencodeDefaultNeedsExactProbeForRestart_004() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let root = data.join("opencode");
    std::fs::create_dir_all(&root).unwrap();
    let mut profile = Profile::new("opencode-provenance", CliKind::Opencode);
    profile.program_path = Override::Set(std::env::current_exe().unwrap().to_str().unwrap().into());
    let revision = WireU64::parse("0").unwrap();
    let caller = CallerIdentity {
        instance_id: "fixture".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let request = LaunchRequest {
        request_id: "opencode-request".into(),
        tab_id: "tab".into(),
        run_id: "run".into(),
        generation: 1,
        profile_id: profile.id.clone(),
        expected_profile_revision: revision,
        cli: CliKind::Opencode,
        launch_cwd: temp.path().to_str().unwrap().into(),
        action: LaunchAction::New,
        extra_args: vec![],
        cols: 80,
        rows: 24,
    };
    let environment = EnvMap::from([("XDG_DATA_HOME".into(), data.as_os_str().into())]);
    let snapshot = freeze_launch(
        &request,
        &profile,
        &caller,
        &FreezeContext {
            inherited: &environment,
            terminal: &EnvMap::new(),
            legacy: None,
            observer: None,
        },
    )
    .unwrap();
    assert_eq!(
        SourceProvenance::freeze(&snapshot)
            .selection()
            .err()
            .unwrap()
            .code,
        "SCOPE_UNKNOWN"
    );
    let exact = root.join("opencode-beta.db");
    let source = SourceProvenance::freeze_with_database(
        &snapshot,
        Ok(Some(exact.to_str().unwrap().to_owned())),
    );
    assert_eq!(
        source.selection().unwrap().root.sqlite_filename(),
        "opencode-beta.db"
    );
    assert!(SourceProvenance::freeze_with_database(
        &snapshot,
        Ok(Some(
            temp.path()
                .join("unrelated.db")
                .to_str()
                .unwrap()
                .to_owned()
        )),
    )
    .selection()
    .is_err());
    assert!(
        SourceProvenance::freeze_with_database(&snapshot, Err(error("SCOPE_UNKNOWN")))
            .selection()
            .is_err()
    );
}
#[test]
fn SourceProvenance_ReplacedRootCannotBecomeOriginalSource_001() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("native");
    let old = temp.path().join("old-native");
    std::fs::create_dir(&root).unwrap();
    let snapshot = frozen(&root, temp.path());
    let provenance = SourceProvenance::freeze(&snapshot);
    assert!(provenance.selection().is_ok());
    match std::fs::rename(&root, &old) {
        Ok(()) => {
            std::fs::create_dir(&root).unwrap();
            assert!(provenance.selection().is_err());
        }
        Err(_error) => {
            #[cfg(not(windows))]
            panic!("root fixture rename failed: {_error}");
            #[cfg(windows)]
            assert!(provenance.selection().is_ok());
        }
    }
}
#[test]
fn SourceProvenance_MissingRootNeverCreatesDirectoryOrFailsSnapshot_002() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("not-yet-created");
    let snapshot = frozen(&root, temp.path());
    let provenance = SourceProvenance::freeze(&snapshot);
    assert!(provenance.selection().is_err());
    assert!(!root.exists());
}

#[cfg(any(windows, unix))]
#[test]
fn SourceProvenance_RetargetedAliasCannotReuseHeldRoot_003() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let alias = temp.path().join("alias");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    fn link(alias: &std::path::Path, target: &std::path::Path) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let status = std::process::Command::new("cmd.exe")
                .creation_flags(0x08000000)
                .args(["/D", "/C", "mklink", "/J"])
                .arg(alias)
                .arg(target)
                .output()
                .unwrap();
            assert!(status.status.success(), "junction fixture creation failed");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, alias).unwrap();
    }
    link(&alias, &first);
    let snapshot = frozen(&alias, temp.path());
    let provenance = SourceProvenance::freeze(&snapshot);
    let original = provenance.selection().unwrap().root.key().to_owned();
    #[cfg(windows)]
    std::fs::remove_dir(&alias).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(&alias).unwrap();
    link(&alias, &second);
    let replacement = Root::open(&alias).unwrap();
    assert_ne!(replacement.key(), original);
    assert!(provenance.selection().is_err());
}
