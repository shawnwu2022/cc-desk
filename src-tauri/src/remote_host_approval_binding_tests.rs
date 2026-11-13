#![allow(non_snake_case)]
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn lease() -> Arc<ApprovalLease> {
    ApprovalLease::new()
}

#[test]
fn HostApproval_SourceFaultRevokesWithoutPoll_007() {
    for mode in 0..3 {
        let lease = lease();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let clock = ApprovalClock {
            original: Arc::new(move || {
                let first = observed.fetch_add(1, Ordering::AcqRel) == 0;
                if first && mode == 0 {
                    return Err(GrantError::Clock);
                }
                if first && mode == 2 {
                    panic!("TEST_ONLY_APPROVAL_CLOCK");
                }
                Ok(123)
            }),
            lease: lease.clone(),
            end: if mode == 1 {
                Instant::now() - Duration::from_millis(1)
            } else {
                Instant::now() + APPROVAL
            },
        };
        let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| clock.now_ms()));
        assert!(!matches!(first, Ok(Ok(_))));
        let before = calls.load(Ordering::Acquire);
        // 新观察包装器不能恢复同一已失效 lease；不调用 UI poll/cancel/retire。
        let later = ApprovalClock {
            original: clock.original.clone(),
            lease,
            end: Instant::now() + APPROVAL,
        };
        assert_eq!(later.now_ms(), Err(GrantError::Revoked), "mode={mode}");
        assert_eq!(calls.load(Ordering::Acquire), before);
    }
}

#[test]
fn HostApproval_RevokePrecedesWatchAndClockNeverNotifies_008() {
    use std::{
        future::Future,
        task::{Context, Wake, Waker},
    };
    struct Probe {
        lease: std::sync::Weak<ApprovalLease>,
        called: AtomicUsize,
        entered: std::sync::mpsc::SyncSender<bool>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            self.called.fetch_add(1, Ordering::AcqRel);
            self.entered
                .send(self.lease.upgrade().is_some_and(|lease| lease.is_revoked()))
                .unwrap();
            self.release
                .lock()
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
        }
    }
    for pure_clock_failure in [false, true] {
        let lease = lease();
        let mut receiver = lease.revoked.subscribe();
        let (entered, observed) = std::sync::mpsc::sync_channel(1);
        let (release, released) = std::sync::mpsc::sync_channel(1);
        let probe = Arc::new(Probe {
            lease: Arc::downgrade(&lease),
            called: AtomicUsize::new(0),
            entered,
            release: Mutex::new(released),
        });
        let waker = Waker::from(probe.clone());
        let mut changed = Box::pin(receiver.changed());
        assert!(changed
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
        if pure_clock_failure {
            let clock = ApprovalClock {
                original: Arc::new(|| Err(GrantError::Clock)),
                lease: lease.clone(),
                end: Instant::now() + APPROVAL,
            };
            assert_eq!(clock.now_ms(), Err(GrantError::Clock));
            assert!(lease.is_revoked());
            assert_eq!(probe.called.load(Ordering::Acquire), 0);
            assert!(!*lease.revoked.borrow());
        } else {
            let cancel = lease.clone();
            let worker = std::thread::spawn(move || cancel.cancel());
            let revoked_before_wake = observed.recv_timeout(Duration::from_secs(3));
            release.send(()).unwrap();
            worker.join().unwrap();
            assert_eq!(revoked_before_wake, Ok(true));
        }
        drop(changed);
    }
}

#[test]
fn HostApproval_MachineFailuresRevokeOriginalLease_009() {
    for mode in 0..4 {
        let now = Instant::now();
        let owner = ApprovalMachine::pending(42, now, now + APPROVAL);
        let lease = owner.lease.clone();
        match mode {
            0 => owner.complete(false, now),
            1 => {
                assert!(owner.take_at(now + PENDING).is_none());
            }
            2 => owner.complete(true, now + PENDING),
            _ => {
                owner.complete(true, now);
                let permit = owner.take_at(now).unwrap();
                drop(owner);
                assert!(lease.is_revoked());
                assert!(permit.consume(|_, _| panic!("lost owner")).is_none());
                continue;
            }
        }
        assert!(lease.is_revoked(), "mode={mode}");
    }
}
