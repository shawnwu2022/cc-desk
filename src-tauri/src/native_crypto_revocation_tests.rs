#![allow(non_snake_case)]

use super::super::owner::*;
use super::*;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    task::{Context, Wake, Waker},
    time::{Duration, Instant},
};

struct Release(Option<mpsc::SyncSender<()>>);
impl Release {
    fn open(mut self) {
        if let Some(send) = self.0.take() {
            let _ = send.send(());
        }
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(send) = self.0.take() {
            let _ = send.send(());
        }
    }
}
fn pause() -> (Box<dyn FnOnce() + Send>, mpsc::Receiver<()>, Release) {
    let (entered, observed) = mpsc::sync_channel(1);
    let (release, held) = mpsc::sync_channel(1);
    (
        Box::new(move || {
            let _ = entered.send(());
            let _ = held.recv_timeout(Duration::from_secs(10));
        }),
        observed,
        Release(Some(release)),
    )
}
struct OriginalThread<T>(Option<std::thread::JoinHandle<T>>);
impl<T: Send + 'static> OriginalThread<T> {
    fn spawn(work: impl FnOnce() -> T + Send + 'static) -> Self {
        Self(Some(std::thread::spawn(work)))
    }
    fn join(mut self) -> T {
        self.0.take().unwrap().join().expect("fixture thread")
    }
}
impl<T> Drop for OriginalThread<T> {
    fn drop(&mut self) {
        if let Some(original) = self.0.take() {
            let _ = original.join();
        }
    }
}
struct BlockingWake {
    first: AtomicBool,
    hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl Wake for BlockingWake {
    fn wake(self: Arc<Self>) {
        if !self.first.swap(true, Ordering::AcqRel) {
            let hook = { self.hook.lock().unwrap().take() };
            if let Some(hook) = hook {
                hook();
            }
        }
    }
}
fn parts() -> (Vec<u8>, Vec<u8>) {
    let point = hex(vector()["public_key_sec1"].as_str().unwrap());
    let mut spki = hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    spki.extend(&point);
    (point, spki)
}
fn join_known(owner: &mut KeyCustodian) {
    let until = Instant::now() + Duration::from_secs(3);
    while owner.holds_original_join() {
        owner.observe();
        assert!(Instant::now() < until, "original key join");
        std::thread::yield_now();
    }
    assert!(matches!(
        owner.observe(),
        KeyRetirement::Retired | KeyRetirement::NoKeyCreated
    ));
}

#[test]
fn CngHost_DeferredStopKeepsSiblingJoining_001() {
    let (point, spki) = parts();
    let (claim, claimed, release_claim) = pause();
    let (mut first, first_handle) = KeyCustodian::start_for_test(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki,
        WorkerHooks {
            before_claim: Some(claim),
            ..Default::default()
        },
    )
    .unwrap();
    let (point, spki) = parts();
    let (mut second, second_handle) = KeyCustodian::start(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki,
    )
    .unwrap();
    assert!(second_handle
        .begin(b"PUBLIC_SECOND_KEY".to_vec(), SignatureEncoding::Fixed)
        .unwrap()
        .wait_blocking()
        .is_ok());
    let mut ticket = first_handle
        .begin(b"PUBLIC_BLOCKED_WAKE".to_vec(), SignatureEncoding::Fixed)
        .unwrap();
    claimed.recv_timeout(Duration::from_secs(3)).unwrap();
    let (wake, notified, release_wake) = pause();
    let waker = Waker::from(Arc::new(BlockingWake {
        first: AtomicBool::new(false),
        hook: Mutex::new(Some(wake)),
    }));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    {
        let _entered = runtime.enter();
        assert!(std::pin::Pin::new(&mut ticket)
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
    }
    let notice_first = first_handle.prepare_shutdown();
    let notice_second = second_handle.prepare_shutdown();
    assert!(matches!(
        notified.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let stop = OriginalThread::spawn(move || {
        notice_first.notify();
        notice_second.notify();
    });
    notified.recv_timeout(Duration::from_secs(3)).unwrap();
    let until = Instant::now() + Duration::from_secs(1);
    while second.holds_original_join() && Instant::now() < until {
        second.observe();
        std::thread::yield_now();
    }
    let sibling_joined = !second.holds_original_join();
    release_wake.open();
    stop.join();
    release_claim.open();
    drop(ticket);
    first.shutdown();
    second.shutdown();
    join_known(&mut first);
    join_known(&mut second);
    assert!(
        sibling_joined,
        "first synchronous wake prevented the second key shutdown and original join"
    );
}

fn prepared(
    token: Arc<NativeRevocation>,
    hooks: WorkerHooks,
) -> (KeyCustodian, KeyRequestHandle, KeyStart) {
    let (point, spki) = parts();
    KeyCustodian::prepare_revocable_for_test(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki,
        token,
        hooks,
    )
    .unwrap()
}
fn attempt(
    handle: &KeyRequestHandle,
    token: Arc<NativeRevocation>,
) -> (Arc<SigningFence>, NativeSigningAttempt) {
    let fence = SigningFence::new_revocable(Instant::now() + Duration::from_secs(20), token);
    let attempt = fence
        .advance(
            std::slice::from_ref(handle),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    (fence, attempt)
}

#[test]
fn CngHost_RevocationAtOriginalChecks_002() {
    for stage in 0..5 {
        let token = NativeRevocation::new();
        let (mut owner, handle, start) = prepared(token.clone(), WorkerHooks::default());
        start.release();
        let (fence, attempt) = attempt(&handle, token.clone());
        let revoke = token.clone();
        match stage {
            0 => {
                attempt.hook_admission(Box::new(move || revoke.revoke()));
            }
            1 => {
                attempt.hook_claim(Box::new(move || revoke.revoke()));
            }
            2 => {
                attempt.hook_returned(Box::new(move || revoke.revoke()));
            }
            3 => {}
            4 => {
                let published = attempt
                    .publish_if_current(|still_current| {
                        revoke.revoke();
                        still_current()
                    })
                    .unwrap();
                assert!(!published);
            }
            _ => unreachable!(),
        }
        let request = handle.begin_attempt(
            b"PUBLIC_REVOKED_AT_BOUNDARY".to_vec(),
            SignatureEncoding::Fixed,
            attempt.clone(),
        );
        let rejected = match request {
            Err(_) => true,
            Ok(mut ticket) => {
                if stage == 3 {
                    let revoke = token.clone();
                    ticket.hook_delivery(Box::new(move || revoke.revoke()));
                }
                ticket.wait_blocking().is_err()
            }
        };
        handle.prepare_shutdown().notify();
        join_known(&mut owner);
        assert!(
            rejected && token.is_revoked() && !attempt.is_current(),
            "stage={stage}"
        );
        assert!(attempt.publish_if_current(|_| ()).is_err());
        assert!(fence
            .advance(
                std::slice::from_ref(&handle),
                Instant::now() + Duration::from_secs(5)
            )
            .is_err());
        assert_eq!(
            owner.native_sign_count(),
            if stage == 2 || stage == 3 { 1 } else { 0 },
            "stage={stage}"
        );
    }
}

#[test]
fn CngHost_FreeAndUnknownKeepOriginalJoin_003() {
    for panics in [false, true] {
        let token = NativeRevocation::new();
        let (mut sibling, sibling_handle, sibling_start) =
            prepared(token.clone(), WorkerHooks::default());
        sibling_start.release();
        assert!(sibling_handle
            .begin(b"PUBLIC_JOIN_SIBLING".to_vec(), SignatureEncoding::Fixed)
            .unwrap()
            .wait_blocking()
            .is_ok());
        let (tail, returned, release) = pause();
        let (owner, handle, start) = prepared(
            token.clone(),
            WorkerHooks {
                after_native: panics.then(|| {
                    Box::new(|| panic!("TEST_ONLY_NATIVE_RETURN_PANIC")) as Box<dyn FnOnce() + Send>
                }),
                after_free: Some(tail),
                ..Default::default()
            },
        );
        let slot = super::super::signers::NativeKeySlot::new(owner, handle.clone());
        start.release();
        let (_, attempt) = attempt(&handle, token.clone());
        let result = handle
            .begin_attempt(
                b"PUBLIC_FREE_THEN_TAIL".to_vec(),
                SignatureEncoding::Fixed,
                attempt,
            )
            .unwrap()
            .wait_blocking();
        if !panics {
            assert!(result.is_ok());
            handle.prepare_shutdown().notify();
        } else {
            assert!(result.is_err());
        }
        returned.recv_timeout(Duration::from_secs(3)).unwrap();
        let observation = slot.observe_complete();
        assert!(observation.holds_original_join);
        assert_eq!(
            observation.retirement,
            if panics {
                KeyRetirement::Unknown
            } else {
                KeyRetirement::Pending
            }
        );
        assert!(token.is_revoked());
        sibling_handle.prepare_shutdown().notify();
        join_known(&mut sibling);
        assert_eq!(sibling.native_sign_count(), 1);
        release.open();
        let until = Instant::now() + Duration::from_secs(3);
        while slot.observe_complete().holds_original_join {
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
        let observation = slot.observe_complete();
        assert!(!observation.holds_original_join);
        assert_eq!(
            observation.retirement,
            if panics {
                KeyRetirement::Unknown
            } else {
                KeyRetirement::Retired
            }
        );
    }
}

#[test]
fn CngHost_PreGateAndFactoryFailureRevoke_004() {
    for mode in 0..3 {
        let token = NativeRevocation::new();
        let opened = Arc::new(AtomicBool::new(false));
        let calls = opened.clone();
        let (_, spki) = parts();
        let (mut owner, handle, start) = KeyCustodian::prepare_revocable(
            move || {
                calls.store(true, Ordering::Release);
                assert!(mode != 2, "TEST_ONLY_FACTORY_PANIC");
                Err(CryptoError::Key)
            },
            SigningPurpose::Endpoint,
            spki,
            token.clone(),
        )
        .unwrap();
        if mode == 0 {
            token.revoke();
        }
        start.release();
        let until = Instant::now() + Duration::from_secs(3);
        while owner.observe_complete().holds_original_join {
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
        assert!(token.is_revoked());
        assert_eq!(opened.load(Ordering::Acquire), mode != 0);
        assert_eq!(
            owner.observe_complete().retirement,
            if mode == 0 {
                KeyRetirement::NoKeyCreated
            } else {
                KeyRetirement::Unknown
            }
        );
        assert!(handle
            .begin(b"NO_REUSE".to_vec(), SignatureEncoding::Fixed)
            .is_err());
    }
    let token = NativeRevocation::new();
    assert!(KeyCustodian::prepare_revocable(
        || panic!("invalid metadata cannot open"),
        SigningPurpose::Endpoint,
        vec![],
        token.clone()
    )
    .is_err());
    assert!(token.is_revoked());
}

#[test]
fn CngHost_DefaultClientFenceRemainsIndependent_005() {
    let token = NativeRevocation::new();
    let (mut host, host_handle, host_start) = prepared(token.clone(), WorkerHooks::default());
    host_start.release();
    let (_, host_attempt) = attempt(&host_handle, token.clone());
    let (point, spki) = parts();
    let public = point.clone();
    let (mut client, client_handle) = KeyCustodian::start(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki,
    )
    .unwrap();
    let fence = SigningFence::new(Instant::now() + Duration::from_secs(20));
    let client_attempt = fence
        .advance(
            std::slice::from_ref(&client_handle),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    token.revoke();
    assert!(!host_attempt.is_current() && client_attempt.is_current());
    let signature = client_handle
        .begin_attempt(
            b"PUBLIC_INDEPENDENT_CLIENT".to_vec(),
            SignatureEncoding::Fixed,
            client_attempt,
        )
        .unwrap()
        .wait_blocking()
        .unwrap();
    ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, &public)
        .verify(b"PUBLIC_INDEPENDENT_CLIENT", &signature)
        .unwrap();
    assert!(fence
        .advance(
            std::slice::from_ref(&client_handle),
            Instant::now() + Duration::from_secs(5)
        )
        .is_ok());
    host_handle.prepare_shutdown().notify();
    client_handle.shutdown();
    join_known(&mut host);
    join_known(&mut client);
}

#[test]
fn CngHost_FailureRevokesBeforeExternalWake_006() {
    struct Probe {
        token: Arc<NativeRevocation>,
        observed: Arc<AtomicBool>,
        called: Arc<AtomicBool>,
    }
    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            self.called.store(true, Ordering::Release);
            self.observed
                .fetch_and(self.token.is_revoked(), Ordering::AcqRel);
        }
    }
    let token = NativeRevocation::new();
    let observed = Arc::new(AtomicBool::new(true));
    let called = Arc::new(AtomicBool::new(false));
    let (claim, claimed, release) = pause();
    let (mut owner, handle, start) = prepared(
        token.clone(),
        WorkerHooks {
            before_claim: Some(claim),
            after_native: Some(Box::new(|| panic!("TEST_ONLY_FAILURE_BEFORE_WAKE"))),
            ..Default::default()
        },
    );
    start.release();
    let (_, attempt) = attempt(&handle, token.clone());
    let mut ticket = handle
        .begin_attempt(
            b"PUBLIC_FAILURE_WAKE".to_vec(),
            SignatureEncoding::Fixed,
            attempt,
        )
        .unwrap();
    claimed.recv_timeout(Duration::from_secs(3)).unwrap();
    let waker = Waker::from(Arc::new(Probe {
        token: token.clone(),
        observed: observed.clone(),
        called: called.clone(),
    }));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    {
        let _entered = runtime.enter();
        assert!(std::pin::Pin::new(&mut ticket)
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
    }
    release.open();
    let until = Instant::now() + Duration::from_secs(3);
    while owner.observe_complete().holds_original_join {
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    assert!(
        token.is_revoked() && observed.load(Ordering::Acquire) && called.load(Ordering::Acquire),
        "failure must revoke before sender/waker destruction"
    );
    assert_eq!(owner.observe_complete().retirement, KeyRetirement::Unknown);
    assert!(runtime.block_on(ticket).is_err());
}

#[test]
fn CngHost_FreeFailureRetainsUnknownOriginal_007() {
    const FLAG: &str = "CC_DESK_TEST_ONLY_CNG_HOST_FREE_FAILURE";
    if std::env::var(FLAG).ok().as_deref() == Some("synthetic-007") {
        let token = NativeRevocation::new();
        let (created, observed) = mpsc::sync_channel(1);
        let (_, spki) = parts();
        let expected: [u8; 91] = spki.clone().try_into().unwrap();
        let (mut owner, handle, start) = KeyCustodian::prepare_revocable(
            move || {
                // 仅合成句柄值，每次free都被固定拦截，不打开或释放任何真实CNG对象。
                let key = NativeOwnedSoftwareKey {
                    provider: NCRYPT_PROV_HANDLE(2),
                    key: NCRYPT_KEY_HANDLE(1),
                    approved_purpose: SigningPurpose::Endpoint,
                    approved_spki: expected,
                    worker: std::thread::current().id(),
                    unknown: false,
                    _not_send_or_sync: PhantomData,
                    #[cfg(feature = "remote-host")]
                    silent_metadata: false,
                    #[cfg(feature = "remote-host")]
                    mock_api: None,
                    free_effect: Some(Box::new(|_| Err(CryptoError::Unknown))),
                };
                created.send(()).unwrap();
                Ok(key)
            },
            SigningPurpose::Endpoint,
            spki,
            token.clone(),
        )
        .unwrap();
        start.release();
        observed.recv_timeout(Duration::from_secs(3)).unwrap();
        handle.prepare_shutdown().notify();
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            let observation = owner.observe_complete();
            if observation.retirement == KeyRetirement::Unknown {
                assert!(
                    observation.holds_original_join && !owner.key_freed() && token.is_revoked()
                );
                println!("TEST_ONLY_SYNTHETIC_UNKNOWN_ORIGINAL_RETAINED_NOT_RETIRED");
                // 负向事实保留原线程；隔离进程结束不冒充Free或原join成功。
                std::process::exit(0);
            }
            assert!(Instant::now() < until, "unknown free not observed");
            std::thread::yield_now();
        }
    }
    struct Child(Option<std::process::Child>);
    impl Drop for Child {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let exe = std::env::current_exe().unwrap();
    let child = std::process::Command::new(exe)
        .args([
            "--exact",
            "native_crypto::tests::revocation_tests::CngHost_FreeFailureRetainsUnknownOriginal_007",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(FLAG, "synthetic-007")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut original = Child(Some(child));
    let until = Instant::now() + Duration::from_secs(8);
    while original.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(Instant::now() < until, "owned negative fixture timed out");
        std::thread::yield_now();
    }
    let output = original.0.take().unwrap().wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("TEST_ONLY_SYNTHETIC_UNKNOWN_ORIGINAL_RETAINED_NOT_RETIRED"));
}
