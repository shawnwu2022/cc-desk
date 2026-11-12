#![allow(non_snake_case)]
use super::*;
use crate::cli::snapshot::CallerIdentity;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
fn request() -> Request {
    serde_json::from_value(serde_json::json!({"operationId":"70000000-0000-4000-8000-000000000001","archived":true,"acknowledgeRecoverable":true,"sessionKey":"fixture","source":{"scopeId":"scope","instanceId":"instance","cli":"opencode","sourceRootKey":"root","identityEpoch":"1","profileId":"profile","profileRevision":"1","basis":"configured-profile","target":{"kind":"profile","profileId":"profile","expectedProfileRevision":"1","projectIds":["project"]}}})).unwrap()
}
fn caller() -> CallerIdentity {
    CallerIdentity {
        instance_id: "instance".into(),
        window_label: "main".into(),
        webview_epoch: crate::cli::types::WireU64::parse("1").unwrap(),
    }
}
fn fixture() -> tempfile::TempDir {
    let root = tempfile::Builder::new()
        .prefix("ccdesk-opencode-archive-test-")
        .tempdir()
        .unwrap();
    let db = rusqlite::Connection::open(root.path().join("fixture.db")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE synthetic(id TEXT PRIMARY KEY,title TEXT); INSERT INTO synthetic VALUES ('target','before'),('other','preserve');").unwrap();
    drop(db);
    root
}
fn node_environment() -> crate::cli::environment::EnvMap {
    std::env::vars_os().collect()
}
fn run(
    prepared: &mut Prepared,
    native_id: &str,
    cwd: &str,
    archived: bool,
    current: &dyn Fn() -> Result<(), SafeError>,
) -> Result<(), SafeError> {
    let job = crate::cli::opencode_prepare::server_job::Job::new().unwrap();
    let mut child = prepared.command.spawn().unwrap();
    job.assign_and_resume(&child).unwrap();
    let result = prepared.process(&mut child, native_id, cwd, archived, current);
    if result.is_err() {
        let _ = job.terminate();
        let _ = child.kill();
    }
    child.wait().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !job.is_empty().unwrap() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(job.is_empty().unwrap());
    result
}
#[test]
fn OpenCodeArchive_RequestAndDedupe_001() {
    let mut r = request();
    r.validate().unwrap();
    r.acknowledge_recoverable = false;
    assert!(r.validate().is_err());
    r.acknowledge_recoverable = true;
    r.source.cli = CliKind::Claude;
    assert!(r.validate().is_err());
    r.source.cli = CliKind::Opencode;
    let ledger = Operations::default();
    let c = caller();
    assert!(ledger.begin(&c, &r).unwrap().is_none());
    assert_eq!(
        ledger.begin(&c, &r).unwrap().unwrap().unwrap_err().code,
        "ARCHIVE_OUTCOME_UNKNOWN"
    );
    let mut altered = r.clone();
    altered.archived = false;
    assert_eq!(
        ledger.begin(&c, &altered).unwrap_err().code,
        "ARCHIVE_OPERATION_CONFLICT"
    );
    ledger.finish(&c, &r, Err(error("ARCHIVE_OUTCOME_UNKNOWN")));
    assert_eq!(
        ledger.begin(&c, &r).unwrap().unwrap().unwrap_err().code,
        "ARCHIVE_OUTCOME_UNKNOWN"
    );
    let receipt = serde_json::to_value(r.receipt()).unwrap();
    assert!(receipt.get("databasePath").is_none());
}
#[test]
fn OpenCodeArchive_PinsNamesWhileAllowingSqliteWrites_002() {
    let root = fixture();
    let pins = PinnedSource::open(root.path(), "fixture.db").unwrap();
    for file in &pins.paths {
        assert!(fs::rename(file, file.with_extension("moved")).is_err());
    }
    let db = rusqlite::Connection::open(root.path().join("fixture.db")).unwrap();
    db.execute("UPDATE synthetic SET title='after' WHERE id='target'", [])
        .unwrap();
    assert_eq!(
        db.query_row("SELECT title FROM synthetic WHERE id='other'", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        "preserve"
    );
    pins.verify().unwrap();
    drop(db);
    pins.verify().unwrap();
    drop(pins);
    fs::rename(root.path().join("fixture.db"), root.path().join("moved.db")).unwrap();
}
#[test]
fn OpenCodeArchive_RejectsAbsentNonWalHardlinksAndUnsafePaths_003() {
    let root = tempfile::tempdir().unwrap();
    assert!(PinnedSource::open(root.path(), "missing.db").is_err());
    assert!(!root.path().join("missing.db").exists());
    let db = rusqlite::Connection::open(root.path().join("plain.db")).unwrap();
    db.execute_batch("CREATE TABLE synthetic(x)").unwrap();
    drop(db);
    assert!(PinnedSource::open(root.path(), "plain.db").is_err());
    assert!(!root.path().join("plain.db-wal").exists());
    let f = fixture();
    fs::hard_link(f.path().join("fixture.db"), f.path().join("alias.db")).unwrap();
    assert!(PinnedSource::open(f.path(), "fixture.db").is_err());
    for path in [
        r"\\server\share",
        r"\\?\UNC\server\share",
        r"C:\wild*",
        r"C:\a,b",
    ] {
        assert!(PinnedSource::open(Path::new(path), "fixture.db").is_err());
    }
    assert!(PinnedSource::open(root.path(), "../escape.db").is_err());
}
#[test]
fn OpenCodeArchive_ActualHelperRejectsUnknownSchema_004() {
    let root = fixture();
    let pins = PinnedSource::open(root.path(), "fixture.db").unwrap();
    let mut prepared = Prepared::new(&node_environment(), pins).unwrap();
    assert_eq!(
        run(
            &mut prepared,
            "ses_target",
            root.path().to_str().unwrap(),
            true,
            &|| Ok(())
        )
        .unwrap_err()
        .code,
        "ARCHIVE_SCHEMA_UNSUPPORTED"
    );
    let db = rusqlite::Connection::open(root.path().join("fixture.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT title FROM synthetic WHERE id='target'", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        "before"
    );
}
#[test]
fn OpenCodeArchive_RevocationBeforeReadyNeverMutates_005() {
    let root = fixture();
    let pins = PinnedSource::open(root.path(), "fixture.db").unwrap();
    let mut prepared = Prepared::new(&node_environment(), pins).unwrap();
    assert_eq!(
        run(
            &mut prepared,
            "ses_target",
            root.path().to_str().unwrap(),
            true,
            &|| Err(error("SCOPE_REVOKED"))
        )
        .unwrap_err()
        .code,
        "SCOPE_REVOKED"
    );
}
#[test]
#[ignore = "creates a fresh fake database using the explicitly selected local SDK modules"]
fn OpenCodeArchive_RealNativeRoundtripWithPinnedSidecars_006() {
    let modules = std::env::var_os("CC_DESK_OPENCODE_ARCHIVE_TEST_MODULES")
        .expect("explicit test modules required");
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let node = crate::cli::native_rename::claude::node_program(&node_environment()).unwrap();
    let mut fixture_command = Command::new(node);
    fixture_command.env_clear();
    crate::platform::configure_command(&mut fixture_command);
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            fixture_command.env(key, value);
        }
    }
    let output = fixture_command
        .arg(workspace.join("tests/scripts/opencodeRecoverySdk.real.mjs"))
        .arg(modules)
        .current_dir(&workspace)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let root = PathBuf::from(report["root"].as_str().unwrap());
    assert!(root.starts_with(workspace.join(".audit")));
    assert!(root
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("opencode-sdk-recovery-"));
    let db = rusqlite::Connection::open(root.join("fixture.db")).unwrap();
    let (id, cwd) = db
        .query_row(
            "SELECT id,directory FROM session_v2 WHERE title='late target'",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .unwrap();
    let other = db
        .query_row(
            "SELECT id FROM session_v2 WHERE title='unrelated'",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap();
    let original_jobs = db
        .query_row("SELECT count(*) FROM job", [], |r| r.get::<_, i64>(0))
        .unwrap();
    let original_schedule = db
        .query_row("SELECT count(*) FROM schedule", [], |r| r.get::<_, i64>(0))
        .unwrap();
    let pins = PinnedSource::open(&root, "fixture.db").unwrap();
    let mut prepared = Prepared::new(&node_environment(), pins).unwrap();
    run(&mut prepared, &id, &cwd, true, &|| Ok(())).unwrap();
    assert!(db
        .query_row(
            "SELECT time_archived FROM session_v2 WHERE id=?1",
            [&id],
            |r| r.get::<_, Option<i64>>(0)
        )
        .unwrap()
        .is_some());
    db.execute(
        "UPDATE session_v2 SET title='late unrelated preserved' WHERE id=?1",
        [&other],
    )
    .unwrap();
    run(&mut prepared, &id, &cwd, false, &|| Ok(())).unwrap();
    assert!(db
        .query_row(
            "SELECT time_archived FROM session_v2 WHERE id=?1",
            [&id],
            |r| r.get::<_, Option<i64>>(0)
        )
        .unwrap()
        .is_none());
    assert_eq!(
        db.query_row("SELECT title FROM session_v2 WHERE id=?1", [&other], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        "late unrelated preserved"
    );
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(
        run(&mut prepared, &id, &cwd, true, &|| Ok(()))
            .unwrap_err()
            .code,
        "ARCHIVE_SOURCE_UNAVAILABLE"
    );
    db.execute_batch("ROLLBACK").unwrap();
    run(&mut prepared, &id, &cwd, true, &|| Ok(())).unwrap();
    let calls = AtomicUsize::new(0);
    let result = run(&mut prepared, &id, &cwd, false, &|| {
        if calls.fetch_add(1, Ordering::SeqCst) > 2 {
            Err(error("SCOPE_REVOKED"))
        } else {
            Ok(())
        }
    });
    assert!(
        result.is_ok()
            || matches!(
                result.unwrap_err().code.as_str(),
                "SCOPE_REVOKED" | "ARCHIVE_OUTCOME_UNKNOWN"
            )
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM job", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        original_jobs
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM schedule", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        original_schedule
    );
    println!("isolated fixture: {}", root.display());
}

#[test]
fn OpenCodeArchive_RejectsHotJournalAndPreventsJournalWrites_008() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = fixture();
    fs::write(
        root.path().join("fixture.db-journal"),
        b"unverified rollback journal",
    )
    .unwrap();
    assert!(PinnedSource::open(root.path(), "fixture.db").is_err());
    assert_eq!(
        fs::read(root.path().join("fixture.db-journal")).unwrap(),
        b"unverified rollback journal"
    );
    let fresh = fixture();
    let pins = PinnedSource::open(fresh.path(), "fixture.db").unwrap();
    assert!(fs::OpenOptions::new()
        .write(true)
        .share_mode(7)
        .open(fresh.path().join("fixture.db-journal"))
        .is_err());
    pins.verify().unwrap();
}
#[test]
fn OpenCodeArchive_DisconnectedOrMismatchedReceiptIsUnknown_009() {
    use std::os::windows::process::CommandExt;
    for response in [
        "",
        r#"process.stdout.write(JSON.stringify({state:'archived',nativeSessionId:'ses_wrong',archived:true})+'\n')"#,
    ] {
        let root = fixture();
        let pins = PinnedSource::open(root.path(), "fixture.db").unwrap();
        let environment = node_environment();
        let mut prepared = Prepared::new(&environment, pins).unwrap();
        // 自建假协议进程不打开任何来源，只模拟已发请求后的断连/错误身份回执。
        let mut command =
            Command::new(crate::cli::native_rename::claude::node_program(&environment).unwrap());
        command.arg("-e").arg(format!("process.stdout.write('{{\"state\":\"ready\"}}\\n');process.stdin.resume();process.stdin.on('end',()=>{{{response}}});"));
        command.env_clear();
        for key in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .creation_flags(0x08000000 | 0x4)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        prepared.command = command;
        assert_eq!(
            run(
                &mut prepared,
                "ses_target",
                root.path().to_str().unwrap(),
                true,
                &|| Ok(())
            )
            .unwrap_err()
            .code,
            "ARCHIVE_OUTCOME_UNKNOWN"
        );
    }
}

#[test]
fn OpenCodeArchive_TimeoutRetainsOwnerAndFreeze_011() {
    use std::{
        sync::{atomic::AtomicBool, mpsc, Arc},
        time::{Duration, Instant},
    };
    let gate = crate::version_history::maintenance::AdmissionGate::new();
    let (frozen, admission) = gate.begin_metadata().unwrap();
    let (release, wait) = mpsc::sync_channel(1);
    let settled = Arc::new(AtomicBool::new(false));
    let done = settled.clone();
    let result = owned(Duration::from_millis(20), move || {
        // 模拟查询/等待暂时无法证明收口，而不是把 kill 请求当 reap。
        wait.recv().unwrap();
        drop(admission);
        frozen.release_review().unwrap();
        done.store(true, Ordering::SeqCst);
        Ok(())
    });
    assert_eq!(result.unwrap_err().code, "ARCHIVE_OUTCOME_UNKNOWN");
    assert!(!settled.load(Ordering::SeqCst));
    assert!(gate.check_metadata_idle().is_err());
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !settled.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(settled.load(Ordering::SeqCst));
    gate.check_metadata_idle().unwrap();
}

#[test]
fn OpenCodeArchive_DeadlineRecheckedAfterSlowSource_012() {
    let started = AtomicUsize::new(0);
    let result = before_deadline(
        std::time::Instant::now() + std::time::Duration::from_millis(20),
        || {
            started.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(30));
            Ok(())
        },
    );
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(result.unwrap_err().code, "ARCHIVE_SOURCE_UNAVAILABLE");
}
