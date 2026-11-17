//! 原 NTFS lifetime 锁实测；记录/许可为隔离测试材料，不启用生产 owner。
use super::*;
use std::{
    io::{BufRead, Write},
    process::{Child, Command, Stdio},
};
#[test]
fn UnattendedCustody_ExclusiveObjectsAndOriginalTransactions_001() {
    let d = directory();
    let s = create(d.path());
    let disabled = initialize(&s);
    let enabled = enable(&s, &disabled);
    let custody = s.backend.claim_owner_custody().unwrap();
    custody.check(&s.backend).unwrap();
    let other = reopen(d.path());
    assert!(matches!(
        other.backend.claim_owner_custody(),
        Err(StoreError::Busy)
    ));
    assert!(matches!(
        s.backend.claim_owner_custody(),
        Err(StoreError::Busy)
    ));
    let observed = s.observe(&enabled.expected(), 1100).unwrap();
    custody.check(&s.backend).unwrap();
    let revoked = s.revoke(&observed.expected(), 1100).unwrap();
    assert_eq!(revoked.0.status, Status::Revoked);
    let journal = std::fs::read(path(&s, "commit.journal")).unwrap();
    let blob = raw(&s);
    drop(custody);
    let next = other.backend.claim_owner_custody().unwrap();
    next.check(&other.backend).unwrap();
    assert_eq!(raw(&s), blob);
    assert_eq!(std::fs::read(path(&s, "commit.journal")).unwrap(), journal);
}
#[test]
fn UnattendedCustody_ForeignAndBackendDrop_002() {
    let d = directory();
    let s = create(d.path());
    let old = initialize(&s);
    let enabled = enable(&s, &old);
    let original = raw(&s);
    let custody = s.backend.claim_owner_custody().unwrap();
    let foreign_dir = directory();
    let foreign = create(foreign_dir.path());
    initialize(&foreign);
    assert_eq!(
        custody.check(&foreign.backend),
        Err(StoreError::Unauthorized)
    );
    drop(s);
    let reopened = reopen(d.path());
    custody.check(&reopened.backend).unwrap();
    assert!(matches!(
        reopened.backend.claim_owner_custody(),
        Err(StoreError::Busy)
    ));
    assert_eq!(
        reopened.observe(&enabled.expected(), 1000).unwrap(),
        enabled
    );
    drop(custody);
    let _next = reopened.backend.claim_owner_custody().unwrap();
    assert_eq!(raw(&reopened), original);
}
struct Helper {
    child: Child,
    reader: Option<std::thread::JoinHandle<()>>,
    lines: std::sync::mpsc::Receiver<String>,
    waited: bool,
}
impl Helper {
    fn spawn(root: &Path, mode: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "remote_unattended_store::tests::custody::UnattendedCustody_ProcessHelper_900",
                "--nocapture",
            ])
            .env("CC_DESK_TEST_UNATTENDED_CUSTODY_ROOT", root)
            .env("CC_DESK_TEST_UNATTENDED_CUSTODY_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines().take(32) {
                if send.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            reader: Some(reader),
            lines,
            waited: false,
        }
    }
    fn ready(&self) {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let line = self
                .lines
                .recv_timeout(end.saturating_duration_since(Instant::now()))
                .unwrap();
            if line == "TEST_ONLY_CUSTODY_READY" {
                return;
            }
        }
    }
    fn finish(&mut self) -> std::process::ExitStatus {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.waited = true;
                self.reader.take().unwrap().join().unwrap();
                return status;
            }
            assert!(Instant::now() < end, "custody helper did not exit");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        if !self.waited {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
#[test]
fn UnattendedCustody_ProcessCompetitionAndNoDestructorExit_003() {
    let d = directory();
    let s = create(d.path());
    let old = initialize(&s);
    let enabled = enable(&s, &old);
    let blob = raw(&s);
    let journal = std::fs::read(path(&s, "commit.journal")).unwrap();
    let custody = s.backend.claim_owner_custody().unwrap();
    let mut probe = Helper::spawn(d.path(), "blocked");
    assert!(probe.finish().success());
    drop(probe);
    drop(custody);
    let mut owner = Helper::spawn(d.path(), "hold-exit");
    owner.ready();
    assert!(matches!(
        s.backend.claim_owner_custody(),
        Err(StoreError::Busy)
    ));
    owner.child.stdin.as_mut().unwrap().write_all(b"x").unwrap();
    assert_eq!(owner.finish().code(), Some(7));
    drop(owner);
    let next = s.backend.claim_owner_custody().unwrap();
    next.check(&s.backend).unwrap();
    assert_eq!(s.observe(&enabled.expected(), 1000).unwrap(), enabled);
    assert_eq!(raw(&s), blob);
    assert_eq!(std::fs::read(path(&s, "commit.journal")).unwrap(), journal);
}
#[test]
#[ignore = "dedicated bounded custody helper process"]
fn UnattendedCustody_ProcessHelper_900() {
    let root = std::env::var_os("CC_DESK_TEST_UNATTENDED_CUSTODY_ROOT").unwrap();
    let mode = std::env::var("CC_DESK_TEST_UNATTENDED_CUSTODY_MODE").unwrap();
    let backend = WindowsUnattendedBackend::open_existing(Path::new(&root)).unwrap();
    if mode == "blocked" {
        assert!(matches!(
            backend.claim_owner_custody(),
            Err(StoreError::Busy)
        ));
        return;
    }
    assert_eq!(mode, "hold-exit");
    let _custody = backend.claim_owner_custody().unwrap();
    println!("TEST_ONLY_CUSTODY_READY");
    std::io::stdout().flush().unwrap();
    let mut byte = [0];
    std::io::Read::read_exact(&mut std::io::stdin(), &mut byte).unwrap();
    std::process::exit(7);
}

#[test]
fn UnattendedCustody_UnknownBlocksExistingAndNewCustody_004() {
    let d = directory();
    let s = create(d.path());
    let disabled = initialize(&s);
    let custody = s.backend.claim_owner_custody().unwrap();
    s.backend.test_journal_fault(5);
    assert_eq!(
        s.enable(permit(&s, disabled.expected(), 1000, 2000), 1000),
        Err(StoreError::Unknown)
    );
    assert_eq!(custody.check(&s.backend), Err(StoreError::Poisoned));
    assert!(matches!(
        s.backend.claim_owner_custody(),
        Err(StoreError::Poisoned)
    ));
}
#[test]
fn UnattendedCustody_UninitializedCustodyDoesNotInitializeStore_005() {
    let d = directory();
    let s = create(d.path());
    let journal = std::fs::read(path(&s, "commit.journal")).unwrap();
    assert!(journal.is_empty());
    let custody = s.backend.claim_owner_custody().unwrap();
    custody.check(&s.backend).unwrap();
    drop(custody);
    assert!(!path(&s, "delegation.dpapi").exists());
    assert_eq!(std::fs::read(path(&s, "commit.journal")).unwrap(), journal);
}
