#![allow(non_snake_case)]
use super::*;
use std::{fs, io::Write, os::windows::fs::OpenOptionsExt, path::PathBuf};
const ID: &str = "70000000-0000-4000-8000-000000000001";
const OP: &str = "70000000-0000-4000-8000-000000000002";
struct Fixture {
    scope: Scope,
    transcript: PathBuf,
    children: PathBuf,
    configuration: PathBuf,
    cwd: String,
    _root: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("ccdesk-recycle-test-")
            .tempdir()
            .unwrap();
        let configuration = root.path().join("configuration");
        let project = configuration.join("projects/authorized");
        let children = project.join(ID);
        fs::create_dir_all(&children).unwrap();
        let cwd = root.path().to_str().unwrap().to_owned();
        let transcript = project.join(format!("{ID}.jsonl"));
        fs::write(&transcript,format!("{}\n",serde_json::json!({"type":"user","sessionId":ID,"cwd":cwd,"uuid":ID,"timestamp":"2026-11-12T00:00:00Z","message":{"role":"user","content":"synthetic recoverable record"}}))).unwrap();
        fs::write(
            children.join("child.keep"),
            b"child remains at original path",
        )
        .unwrap();
        fs::write(
            project.join("sessions-index.json"),
            format!("{{\"version\":1,\"entries\":[{{\"sessionId\":\"{ID}\"}}]}}"),
        )
        .unwrap();
        let scope = Scope {
            root: Root::open(&configuration).unwrap(),
            project_paths: vec![root.path().to_owned()],
            check: Arc::new(|| Ok(())),
        };
        Self {
            scope,
            transcript,
            children,
            configuration,
            cwd,
            _root: root,
        }
    }
    fn target(&self) -> Target {
        prepare_target(
            &self.scope,
            &format!("projects/authorized/{ID}.jsonl"),
            ID,
            &self.cwd,
        )
        .unwrap()
    }
    fn payload(&self) -> PathBuf {
        self.configuration
            .join(".ccdesk-recovery-v1")
            .join(OP)
            .join("transcript.jsonl")
    }
    fn sdk_ids(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let helper = self._root.path().join("helper");
        fs::create_dir_all(&helper).unwrap();
        let sdk = include_bytes!("../../../node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs");
        assert_eq!(
            format!("{:x}", Sha256::digest(sdk)),
            "6b963b3eb871949a14392bb35ff970a55e91e2edbe218c61cd9eed60e46cbe45"
        );
        fs::write(helper.join("sdk.mjs"), sdk).unwrap();
        fs::write(helper.join("list.mjs"),"import {listSessions} from './sdk.mjs'; console.log(JSON.stringify((await listSessions()).map(s=>s.sessionId))); ").unwrap();
        let node = Command::new("node")
            .args(["-p", "process.execPath"])
            .output()
            .unwrap();
        assert!(node.status.success());
        let node = String::from_utf8(node.stdout).unwrap();
        let mut command = Command::new(node.trim());
        crate::platform::configure_command(&mut command);
        command
            .arg("--permission")
            .arg(format!("--allow-fs-read={}", helper.display()))
            .arg(format!("--allow-fs-read={}", self.configuration.display()))
            .arg(helper.join("list.mjs"));
        command
            .env_clear()
            .env("SystemRoot", std::env::var_os("SystemRoot").unwrap())
            .env("CLAUDE_CONFIG_DIR", &self.configuration)
            .current_dir(self._root.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let start = Instant::now();
        while child.try_wait().unwrap().is_none() {
            if start.elapsed() > Duration::from_secs(15) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("isolated SDK list timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
#[test]
fn NativeRecycle_MoveRestorePreservesOriginalAndChildren_001() {
    let f = Fixture::new();
    let original = fs::read(&f.transcript).unwrap();
    let index = f.transcript.parent().unwrap().join("sessions-index.json");
    let index_bytes = fs::read(&index).unwrap();
    let moved = remove(&f.scope, OP, f.target()).unwrap();
    assert_eq!(moved.records[0].state, "stored");
    assert!(!f.transcript.exists());
    assert_eq!(fs::read(f.payload()).unwrap(), original);
    assert_eq!(
        fs::read(f.children.join("child.keep")).unwrap(),
        b"child remains at original path"
    );
    assert_eq!(fs::read(index).unwrap(), index_bytes);
    assert_eq!(list(&f.scope).unwrap().records.len(), 1);
    assert_eq!(restore(&f.scope, OP).unwrap().records[0].state, "source");
    assert_eq!(fs::read(&f.transcript).unwrap(), original);
    assert_eq!(restore(&f.scope, OP).unwrap().records[0].state, "source");
}
#[test]
fn NativeRecycle_OriginalWriterBlocksAndLateChildrenSurvive_002() {
    let f = Fixture::new();
    let target = f.target();
    let writer = fs::OpenOptions::new()
        .append(true)
        .share_mode(7)
        .open(&f.transcript)
        .unwrap();
    assert_eq!(
        remove(&f.scope, OP, target).err().unwrap().code,
        "SOURCE_BUSY"
    );
    assert!(f.transcript.exists());
    drop(writer);
    let mut child = fs::OpenOptions::new()
        .append(true)
        .share_mode(7)
        .open(f.children.join("child.keep"))
        .unwrap();
    remove(&f.scope, OP, f.target()).unwrap();
    child.write_all(b" late writer").unwrap();
    drop(child);
    assert!(fs::read_to_string(f.children.join("child.keep"))
        .unwrap()
        .ends_with(" late writer"));
    fs::write(f.children.join("late.jsonl"), b"late child preserved").unwrap();
    restore(&f.scope, OP).unwrap();
    assert_eq!(
        fs::read(f.children.join("late.jsonl")).unwrap(),
        b"late child preserved"
    );
}
#[test]
fn NativeRecycle_RestoreConflictNeverOverwritesSuccessor_003() {
    let f = Fixture::new();
    let original = fs::read(&f.transcript).unwrap();
    remove(&f.scope, OP, f.target()).unwrap();
    fs::write(&f.transcript, b"external successor").unwrap();
    assert!(list(&f.scope).unwrap().records[0].original_occupied);
    assert_eq!(
        restore(&f.scope, OP).err().unwrap().code,
        "RECYCLE_RESTORE_CONFLICT"
    );
    assert_eq!(fs::read(&f.transcript).unwrap(), b"external successor");
    assert_eq!(fs::read(f.payload()).unwrap(), original);
    fs::rename(&f.transcript, f.transcript.with_extension("successor")).unwrap();
    assert_eq!(restore(&f.scope, OP).unwrap().records[0].state, "source");
}
struct Fault;
impl Drop for Fault {
    fn drop(&mut self) {
        platform::FAIL_AT.with(|v| *v.borrow_mut() = None);
    }
}
#[test]
fn NativeRecycle_JournalAndMovedFaultsAreReadOnlyReconciled_004() {
    for stage in ["journal", "moved"] {
        let f = Fixture::new();
        let target = f.target();
        let original = fs::read(&f.transcript).unwrap();
        platform::FAIL_AT.with(|v| *v.borrow_mut() = Some(stage.into()));
        let fault = Fault;
        assert_eq!(
            remove(&f.scope, OP, target).err().unwrap().code,
            "RECYCLE_OUTCOME_UNKNOWN"
        );
        drop(fault);
        // 新 scope 不依赖原操作对象；只读观察不重发移动。
        let reopened = Scope {
            root: Root::open(&f.configuration).unwrap(),
            project_paths: f.scope.project_paths.clone(),
            check: Arc::new(|| Ok(())),
        };
        assert_eq!(
            list(&reopened).unwrap().records[0].state,
            if stage == "journal" {
                "source"
            } else {
                "stored"
            }
        );
        restore(&reopened, OP).unwrap();
        assert_eq!(fs::read(&f.transcript).unwrap(), original);
    }
}
#[test]
fn NativeRecycle_RevokedScopeWrongProjectAndHardlinksAreRejected_005() {
    let mut f = Fixture::new();
    let target = f.target();
    f.scope.check = Arc::new(|| Err(error("SCOPE_REVOKED")));
    assert_eq!(
        remove(&f.scope, OP, target).err().unwrap().code,
        "SCOPE_REVOKED"
    );
    assert!(f.transcript.exists());
    f.scope.check = Arc::new(|| Ok(()));
    f.scope.project_paths.clear();
    assert_eq!(
        prepare_target(
            &f.scope,
            &format!("projects/authorized/{ID}.jsonl"),
            ID,
            &f.cwd
        )
        .err()
        .unwrap()
        .code,
        "FORBIDDEN"
    );
    f.scope.project_paths.push(f._root.path().to_owned());
    fs::hard_link(&f.transcript, f._root.path().join("alias.jsonl")).unwrap();
    assert!(prepare_target(
        &f.scope,
        &format!("projects/authorized/{ID}.jsonl"),
        ID,
        &f.cwd
    )
    .is_err());
    assert!(f.transcript.exists());
}
#[test]
fn NativeRecycle_OfficialSdkDiscoveryOmitsRecoveryAndRestores_006() {
    let f = Fixture::new();
    assert!(f.sdk_ids().contains(&ID.to_owned()));
    remove(&f.scope, OP, f.target()).unwrap();
    assert!(!f.sdk_ids().contains(&ID.to_owned()));
    assert!(f.payload().exists());
    restore(&f.scope, OP).unwrap();
    assert!(f.sdk_ids().contains(&ID.to_owned()));
}

#[test]
fn NativeRecycle_HalfWrittenJournalIsNeverPublished_009() {
    let f = Fixture::new();
    let target = f.target();
    let original = fs::read(&f.transcript).unwrap();
    platform::FAIL_AT.with(|v| *v.borrow_mut() = Some("journal-write".into()));
    let fault = Fault;
    assert_eq!(
        remove(&f.scope, OP, target).err().unwrap().code,
        "RECYCLE_OUTCOME_UNKNOWN"
    );
    drop(fault);
    assert!(list(&f.scope).unwrap().records.is_empty());
    assert_eq!(fs::read(&f.transcript).unwrap(), original);
    assert!(!f.payload().exists());
    assert!(!f.payload().with_file_name("record.json").exists());
    assert!(f.payload().with_file_name("intent.tmp").exists());
}

#[test]
fn NativeRecycle_DamagedJournalDoesNotHideValidRecovery_010() {
    let f = Fixture::new();
    remove(&f.scope, OP, f.target()).unwrap();
    let damaged = f
        .configuration
        .join(".ccdesk-recovery-v1/70000000-0000-4000-8000-000000000099");
    fs::create_dir(&damaged).unwrap();
    fs::write(damaged.join("record.json"), b"{partial").unwrap();
    let result = list(&f.scope).unwrap();
    assert_eq!(result.unavailable_count, 1);
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].state, "stored");
    restore(&f.scope, OP).unwrap();
    assert!(f.transcript.exists());
    assert_eq!(fs::read(damaged.join("record.json")).unwrap(), b"{partial");
}
#[test]
fn NativeRecycle_RestoreRaceNeverOverwritesNewOriginal_011() {
    let f = Fixture::new();
    let bytes = fs::read(&f.transcript).unwrap();
    remove(&f.scope, OP, f.target()).unwrap();
    let path = f.transcript.clone();
    let _probe = crate::version_history::windows::files::probe_before_rename(move || {
        fs::write(path, b"racing successor").unwrap()
    });
    assert_eq!(
        restore(&f.scope, OP).err().unwrap().code,
        "RECYCLE_OUTCOME_UNKNOWN"
    );
    assert_eq!(fs::read(&f.transcript).unwrap(), b"racing successor");
    assert_eq!(fs::read(f.payload()).unwrap(), bytes);
    let observed = list(&f.scope).unwrap();
    assert_eq!(observed.records[0].state, "stored");
    assert!(observed.records[0].original_occupied);
}

fn fixture_junction(link: &std::path::Path, target: &std::path::Path) {
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "synthetic junction creation failed"
    );
}
#[test]
fn NativeRecycle_ReviewLaterCwdDoesNotChangeFirstProject_012() {
    let f = Fixture::new();
    let subdir = f._root.path().join("subdirectory");
    fs::create_dir(&subdir).unwrap();
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&f.transcript)
        .unwrap();
    writeln!(file,"{}",serde_json::json!({"type":"user","sessionId":ID,"cwd":subdir,"message":{"role":"user","content":"later subdirectory"}})).unwrap();
    drop(file);
    let original = fs::read(&f.transcript).unwrap();
    remove(&f.scope, OP, f.target()).unwrap();
    restore(&f.scope, OP).unwrap();
    assert_eq!(fs::read(&f.transcript).unwrap(), original);
}
#[test]
fn NativeRecycle_ReviewReparseEntryDoesNotHideHealthyRecord_013() {
    let f = Fixture::new();
    remove(&f.scope, OP, f.target()).unwrap();
    let outside = f._root.path().join("synthetic-outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("outside.keep"), b"outside unchanged").unwrap();
    let recovery = f.configuration.join(".ccdesk-recovery-v1");
    let linked = recovery.join("70000000-0000-4000-8000-000000000098");
    fixture_junction(&linked, &outside);
    let directory =
        crate::version_history::windows::files::Directory::open_absolute(&recovery).unwrap();
    assert!(directory.read_children(128).is_err());
    drop(directory);
    let result = list(&f.scope).unwrap();
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].state, "stored");
    assert_eq!(result.unavailable_count, 1);
    restore(&f.scope, OP).unwrap();
    assert_eq!(
        fs::read(outside.join("outside.keep")).unwrap(),
        b"outside unchanged"
    );
}
#[test]
fn NativeRecycle_ReviewUnadmittedJournalAliasIsNotAuthority_014() {
    let f = Fixture::new();
    remove(&f.scope, OP, f.target()).unwrap();
    let external = tempfile::Builder::new()
        .prefix("ccdesk-recycle-alias-")
        .tempdir()
        .unwrap();
    let alias = external.path().join("unadmitted-alias");
    fixture_junction(&alias, f._root.path());
    let journal = f.payload().with_file_name("record.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
    value["cwd"] = serde_json::json!(alias);
    fs::write(&journal, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(list(&f.scope).unwrap().records.is_empty());
    assert!(!f.scope.permits(alias.to_str().unwrap()));
    assert!(f.payload().exists());
    assert!(!f.transcript.exists());
}

#[test]
fn NativeRecycle_ReviewFirstCwdStillRejectsOtherProject_015() {
    let f = Fixture::new();
    let original = fs::read(&f.transcript).unwrap();
    let other = f._root.path().join("not-the-admitted-project");
    fs::create_dir(&other).unwrap();
    let mut bytes = format!(
        "{}\n",
        serde_json::json!({"type":"user","sessionId":ID,"cwd":other})
    )
    .into_bytes();
    bytes.extend_from_slice(&original);
    fs::write(&f.transcript, &bytes).unwrap();
    assert!(prepare_target(
        &f.scope,
        &format!("projects/authorized/{ID}.jsonl"),
        ID,
        &f.cwd
    )
    .is_err());
    assert_eq!(fs::read(&f.transcript).unwrap(), bytes);
}
#[test]
fn NativeRecycle_ReviewRestoreAcknowledgementLossIsUnknown_016() {
    let f = Fixture::new();
    let original = fs::read(&f.transcript).unwrap();
    remove(&f.scope, OP, f.target()).unwrap();
    platform::FAIL_AT.with(|v| *v.borrow_mut() = Some("restored".into()));
    let fault = Fault;
    assert_eq!(
        restore(&f.scope, OP).err().unwrap().code,
        "RECYCLE_OUTCOME_UNKNOWN"
    );
    drop(fault);
    let result = list(&f.scope).unwrap();
    assert_eq!(result.records[0].state, "source");
    assert_eq!(fs::read(&f.transcript).unwrap(), original);
    assert!(!f.payload().exists());
}
