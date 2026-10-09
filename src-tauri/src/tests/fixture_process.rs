//! Test-only custody of a disposable child; no Job or production admission changes.
use std::{
    io::{self, Read, Write},
    ops::{Deref, DerefMut},
    process::{Child, ExitStatus},
    time::{Duration, Instant},
};

pub(super) struct FixtureChild(Child);
impl FixtureChild {
    pub(super) fn new(child: Child) -> Self {
        Self(child)
    }
    pub(super) fn release(&mut self, byte: u8) -> io::Result<()> {
        self.0
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("fixture release writer unavailable"))?
            .write_all(&[byte])
    }
    pub(super) fn wait_bounded(&mut self, budget: Duration) -> io::Result<ExitStatus> {
        let deadline = Instant::now() + budget;
        loop {
            if let Some(status) = self.0.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "fixture child exit",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Deref for FixtureChild {
    type Target = Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for FixtureChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for FixtureChild {
    fn drop(&mut self) {
        // EOF is failure in the worker. Never convert an assertion failure into release success.
        drop(self.0.stdin.take());
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = self.0.kill();
        if let Err(error) = self.wait_bounded(Duration::from_secs(5)) {
            eprintln!(
                "FIXTURE_CHILD_CLEANUP pid={} kind={:?}",
                self.0.id(),
                error.kind()
            );
        }
    }
}

fn read_release(reader: &mut impl Read, expected: u8) -> io::Result<()> {
    let mut byte = [0];
    reader.read_exact(&mut byte)?;
    if byte[0] != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fixture release byte",
        ));
    }
    Ok(())
}
pub(super) fn await_fixture_release(expected: u8) {
    // The sole writer is held by FixtureChild. Supervisor death closes it without a timer.
    if read_release(&mut io::stdin().lock(), expected).is_err() {
        std::process::exit(124);
    }
}

pub(crate) fn wait_for_preparation(
    deadline: Instant,
    mut observe: impl FnMut() -> io::Result<bool>,
) -> io::Result<()> {
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "fixture preparation",
            ));
        }
        let ready = observe()?;
        // A file or synchronous native query that returns late cannot rescue the guard.
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "fixture preparation",
            ));
        }
        if ready {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn behavior_deadline(
    preparation_deadline: Instant,
    prepared_at: Instant,
    behavior_budget: Duration,
) -> io::Result<Instant> {
    if prepared_at >= preparation_deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "fixture preparation",
        ));
    }
    Ok(prepared_at + behavior_budget)
}

// 检查 R/A 才允许正常 release，X 保留失败。
#[test]
fn FixtureProcess_ReleaseByte_001() {
    assert!(read_release(&mut io::Cursor::new(b"R"), b'R').is_ok());
    assert!(read_release(&mut io::Cursor::new(b"A"), b'A').is_ok());
    assert_eq!(
        read_release(&mut io::Cursor::new(b"X"), b'R')
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
}
// 检查监督者关闭管道后的 EOF 不能成为正常 release。
#[test]
fn FixtureProcess_SupervisorEof_002() {
    assert_eq!(
        read_release(&mut io::Cursor::new(b""), b'R')
            .unwrap_err()
            .kind(),
        io::ErrorKind::UnexpectedEof
    );
}
// 检查 61 秒准备完成后仍获得完整的 15 秒行为窗口。
#[test]
fn FixtureProcess_PreparationBudget_003() {
    let started = Instant::now();
    let prepared_at = started + Duration::from_secs(61);
    let budget = Duration::from_secs(15);
    assert_eq!(
        behavior_deadline(started + Duration::from_secs(300), prepared_at, budget).unwrap()
            - prepared_at,
        budget
    );
}
// 检查在准备上限到达或超出时拒绝进入行为阶段。
#[test]
fn FixtureProcess_LatePreparation_004() {
    let deadline = Instant::now();
    for finished in [deadline, deadline + Duration::from_secs(1)] {
        assert_eq!(
            behavior_deadline(deadline, finished, Duration::from_secs(15))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
    }
}

// 准备观察先不完整，再满足条件；只有真实 ready 才能结束等待。
#[test]
fn FixtureProcess_PreparationWaitReady_005() {
    let mut observations = 0;
    wait_for_preparation(Instant::now() + Duration::from_secs(1), || {
        observations += 1;
        Ok(observations == 2)
    })
    .unwrap();
    assert_eq!(observations, 2);
}

// 已过准备期限时，即使文件已经存在也不能补救为成功。
#[test]
fn FixtureProcess_PreparationWaitLate_006() {
    assert_eq!(
        wait_for_preparation(Instant::now(), || Ok(true))
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
}

// 同步观察晚于准备期限才返回 ready，仍保持超时失败。
#[test]
fn FixtureProcess_PreparationWaitLateQuery_007() {
    let deadline = Instant::now() + Duration::from_millis(50);
    let mut observed = false;
    assert_eq!(
        wait_for_preparation(deadline, || {
            observed = true;
            std::thread::sleep(
                deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(2),
            );
            Ok(true)
        })
        .unwrap_err()
        .kind(),
        io::ErrorKind::TimedOut
    );
    assert!(
        observed,
        "regression must exercise a late observation return"
    );
}

// 真实观察失败保留原错误，不因准备 guard 而吞掉错误。
#[test]
fn FixtureProcess_PreparationWaitIo_008() {
    assert_eq!(
        wait_for_preparation(Instant::now() + Duration::from_secs(1), || {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "fixture"))
        })
        .unwrap_err()
        .kind(),
        io::ErrorKind::PermissionDenied
    );
}
