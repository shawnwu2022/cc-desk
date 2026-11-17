#![allow(non_snake_case)]
use super::*;
use crate::native_crypto::{
    existing::{ExistingKeyLoader, MockKeyApi},
    host::{Opener, OpenerMailbox},
    owner::*,
};
use parking_lot::Mutex;
use std::{
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Good,
    ProviderErr,
    OpenErr,
    OpenPanic,
    PropertyErr,
    PropertyPanic,
    Length,
    Group,
    Usage,
    PublicErr,
    PublicMismatch,
    CancelProvider,
}
#[derive(Clone, Debug)]
struct Event {
    name: &'static str,
    worker: std::thread::ThreadId,
    revoked: bool,
}
struct Api {
    mode: Mode,
    token: Arc<NativeRevocation>,
    public: [u8; 91],
    events: Mutex<Vec<Event>>,
    name: String,
    free_fail: usize,
    free_panic: bool,
    verified_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    provider_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    open_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    free_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    public_signal: Mutex<Option<mpsc::SyncSender<()>>>,
}
impl Api {
    fn new(mode: Mode, token: Arc<NativeRevocation>) -> Arc<Self> {
        Arc::new(Self {
            mode,
            token,
            public: public(),
            events: Mutex::new(Vec::new()),
            name: "TEST_ONLY_EXISTING".into(),
            free_fail: 0,
            free_panic: false,
            verified_hook: Mutex::new(None),
            provider_hook: Mutex::new(None),
            open_hook: Mutex::new(None),
            free_hook: Mutex::new(None),
            public_signal: Mutex::new(None),
        })
    }
    fn record(&self, name: &'static str) {
        self.events.lock().push(Event {
            name,
            worker: std::thread::current().id(),
            revoked: self.token.is_revoked(),
        });
    }
}
fn public() -> [u8; 91] {
    let p = hex(vector()["public_key_sec1"].as_str().unwrap());
    [SPKI_HEAD, &p[1..]].concat().try_into().unwrap()
}
impl MockKeyApi for Api {
    fn after_verified(&self) {
        let hook = self.verified_hook.lock().take();
        if let Some(hook) = hook {
            hook();
        }
    }
    fn provider(&self, name: windows_core::PCWSTR, flags: u32) -> Result<usize, CryptoError> {
        assert_eq!(
            unsafe { name.to_string().unwrap() },
            "Microsoft Software Key Storage Provider"
        );
        assert_eq!(flags, 0);
        self.record("provider");
        let hook = self.provider_hook.lock().take();
        if let Some(hook) = hook {
            hook();
        }
        if self.mode == Mode::ProviderErr {
            return Err(CryptoError::Native);
        }
        if self.mode == Mode::CancelProvider {
            self.token.revoke();
        }
        Ok(101)
    }
    fn open(
        &self,
        provider: usize,
        name: &[u16],
        legacy: u32,
        flags: u32,
    ) -> Result<usize, CryptoError> {
        self.record("open");
        let hook = self.open_hook.lock().take();
        if let Some(hook) = hook {
            hook();
        }
        assert_eq!(provider, 101);
        assert_eq!(legacy, 0);
        assert_eq!(flags, NCRYPT_SILENT_FLAG.0);
        assert_eq!(
            name,
            self.name.encode_utf16().chain(Some(0)).collect::<Vec<_>>()
        );
        if self.mode == Mode::OpenPanic {
            panic!("TEST_ONLY after provider");
        }
        if self.mode == Mode::OpenErr {
            return Err(CryptoError::Native);
        }
        Ok(202)
    }
    fn property(
        &self,
        key: usize,
        name: windows_core::PCWSTR,
        flags: u32,
    ) -> Result<Vec<u8>, CryptoError> {
        assert_eq!(flags, NCRYPT_SILENT_FLAG.0);
        let name = unsafe { name.to_string().unwrap() };
        self.record("property");
        assert_eq!(key, 202);
        if self.mode == Mode::PropertyPanic {
            panic!("TEST_ONLY after key");
        }
        if self.mode == Mode::PropertyErr {
            return Err(CryptoError::Native);
        }
        if name == "Length" {
            return Ok(if self.mode == Mode::Length {
                384u32
            } else {
                256u32
            }
            .to_le_bytes()
            .to_vec());
        }
        if name == "Algorithm Group" {
            return Ok(if self.mode == Mode::Group {
                "RSA\0"
            } else {
                "ECDSA\0"
            }
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect());
        }
        assert_eq!(name, "Key Usage");
        Ok(if self.mode == Mode::Usage {
            0u32
        } else {
            NCRYPT_ALLOW_SIGNING_FLAG
        }
        .to_le_bytes()
        .to_vec())
    }
    fn public(&self, key: usize, flags: u32) -> Result<Vec<u8>, CryptoError> {
        assert_eq!(flags, NCRYPT_SILENT_FLAG.0);
        self.record("public");
        assert_eq!(key, 202);
        if let Some(send) = self.public_signal.lock().take() {
            send.send(()).unwrap();
        }
        if self.mode == Mode::PublicErr {
            return Err(CryptoError::Native);
        }
        let mut blob = Vec::from(BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes());
        blob.extend(32u32.to_le_bytes());
        blob.extend(&self.public[SPKI_HEAD.len()..]);
        if self.mode == Mode::PublicMismatch {
            blob[8] ^= 1;
        }
        Ok(blob)
    }
    fn free(&self, handle: usize) -> Result<(), CryptoError> {
        self.record(if handle == 202 {
            "free_key"
        } else {
            "free_provider"
        });
        assert!(handle == 101 || handle == 202);
        let hook = self.free_hook.lock().take();
        if let Some(hook) = hook {
            hook();
        }
        if self.free_fail == handle {
            if self.free_panic {
                panic!("TEST_ONLY free panic");
            }
            return Err(CryptoError::Unknown);
        }
        Ok(())
    }
}
fn mailbox(api: Arc<Api>) -> OpenerMailbox {
    let mut loader =
        ExistingKeyLoader::new("TEST_ONLY_EXISTING", SigningPurpose::Endpoint, public()).unwrap();
    loader.test_api(api);
    Arc::new(Mutex::new(Some(Opener::Existing(loader))))
}
fn owner(
    api: Arc<Api>,
    cap: Instant,
    hooks: WorkerHooks,
) -> (KeyCustodian, KeyRequestHandle, KeyStart) {
    KeyCustodian::prepare_host_for_test(
        mailbox(api.clone()),
        SigningPurpose::Endpoint,
        public().to_vec(),
        api.token.clone(),
        cap,
        hooks,
    )
    .unwrap()
}
fn join(owner: &mut KeyCustodian) -> KeyObservation {
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        let fact = owner.observe_complete();
        if !fact.holds_original_join {
            return fact;
        }
        assert!(Instant::now() < end, "original worker did not join");
        std::thread::yield_now();
    }
}
fn verify_free(api: &Api, expected: &[&str]) {
    let events = api.events.lock();
    let names: Vec<_> = events
        .iter()
        .filter(|e| e.name.starts_with("free"))
        .map(|e| e.name)
        .collect();
    assert_eq!(names, expected);
    assert!(events
        .iter()
        .filter(|e| e.name.starts_with("free"))
        .all(|e| e.revoked));
    assert!(events.iter().all(|e| e.worker == events[0].worker));
    assert_ne!(events[0].worker, std::thread::current().id());
}
#[test]
fn HostOpen_PartialPanicRevokesBeforeFree_001() {
    for (mode, expected) in [
        (Mode::OpenPanic, vec!["free_provider"]),
        (Mode::PropertyPanic, vec!["free_key", "free_provider"]),
    ] {
        let token = NativeRevocation::new();
        let api = Api::new(mode, token.clone());
        let (mut owner, signal, start) = owner(
            api.clone(),
            Instant::now() + Duration::from_secs(5),
            WorkerHooks::default(),
        );
        start.release();
        let fact = join(&mut owner);
        verify_free(&api, &expected);
        assert!(token.is_revoked());
        assert_eq!(fact.retirement, KeyRetirement::Unknown);
        eprintln!(
            "TEST_ONLY_PARTIAL_PANIC before_free=true joined={}",
            !fact.holds_original_join
        );
        drop(signal);
    }
}
#[test]
fn HostOpen_KnownRejectionRetiresOriginalPartial_002() {
    for mode in [
        Mode::ProviderErr,
        Mode::OpenErr,
        Mode::PropertyErr,
        Mode::Length,
        Mode::Group,
        Mode::Usage,
        Mode::PublicErr,
        Mode::PublicMismatch,
    ] {
        let token = NativeRevocation::new();
        let api = Api::new(mode, token.clone());
        let (mut owner, signal, start) = owner(
            api.clone(),
            Instant::now() + Duration::from_secs(5),
            WorkerHooks::default(),
        );
        start.release();
        let fact = join(&mut owner);
        assert!(token.is_revoked());
        if mode == Mode::ProviderErr {
            assert_eq!(fact.retirement, KeyRetirement::NoKeyCreated);
            verify_free(&api, &[]);
        } else {
            assert_eq!(fact.retirement, KeyRetirement::Retired);
            verify_free(
                &api,
                if mode == Mode::OpenErr {
                    &["free_provider"]
                } else {
                    &["free_key", "free_provider"]
                },
            );
        }
        drop(signal);
    }
}
#[test]
fn HostOpen_PreCancelAndDeadlineNeverOpen_003() {
    for pre_cancel in [true, false] {
        let token = NativeRevocation::new();
        let api = Api::new(Mode::Good, token.clone());
        let cap = if pre_cancel {
            Instant::now() + Duration::from_secs(5)
        } else {
            Instant::now()
        };
        let (mut owner, signal, start) = owner(api.clone(), cap, WorkerHooks::default());
        if pre_cancel {
            token.revoke();
        }
        start.release();
        assert_eq!(join(&mut owner).retirement, KeyRetirement::NoKeyCreated);
        assert!(api.events.lock().is_empty());
        assert!(token.is_revoked());
        drop(signal);
    }
    let token = NativeRevocation::new();
    let api = Api::new(Mode::CancelProvider, token);
    let (mut owner, signal, start) = owner(
        api.clone(),
        Instant::now() + Duration::from_secs(5),
        WorkerHooks::default(),
    );
    start.release();
    assert_eq!(join(&mut owner).retirement, KeyRetirement::Retired);
    assert_eq!(
        api.events.lock().iter().map(|e| e.name).collect::<Vec<_>>(),
        vec!["provider", "free_provider"]
    );
    verify_free(&api, &["free_provider"]);
    drop(signal);
}
struct Gate(Option<mpsc::SyncSender<()>>);
impl Drop for Gate {
    fn drop(&mut self) {
        if let Some(send) = self.0.take() {
            let _ = send.send(());
        }
    }
}
fn gate() -> (Box<dyn FnOnce() + Send>, mpsc::Receiver<()>, Gate) {
    let (send, entered) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    (
        Box::new(move || {
            send.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
        }),
        entered,
        Gate(Some(release)),
    )
}
#[test]
fn HostOpen_BlockedFreeKeepsOwnerAndSiblingJoins_004() {
    let token = NativeRevocation::new();
    let first = Api::new(Mode::OpenErr, token.clone());
    let (hook, at_free, release) = gate();
    *first.free_hook.lock() = Some(hook);
    let (mut a, sa, ga) = owner(
        first.clone(),
        Instant::now() + Duration::from_secs(5),
        WorkerHooks::default(),
    );
    let second = Api::new(Mode::Good, token.clone());
    let (mut b, sb, gb) = owner(
        second,
        Instant::now() + Duration::from_secs(5),
        WorkerHooks::default(),
    );
    ga.release();
    at_free.recv_timeout(Duration::from_secs(3)).unwrap();
    gb.release();
    let bf = join(&mut b);
    let af = a.observe_complete();
    drop(release);
    let final_a = join(&mut a);
    assert!(af.holds_original_join);
    assert_eq!(af.retirement, KeyRetirement::Pending);
    assert!(!bf.holds_original_join);
    assert_eq!(final_a.retirement, KeyRetirement::Retired);
    verify_free(&first, &["free_provider"]);
    drop((sa, sb));
}
#[test]
fn HostOpen_FreeReturnedButThreadTailStillPending_005() {
    let token = NativeRevocation::new();
    let api = Api::new(Mode::OpenErr, token);
    let (hook, at_tail, release) = gate();
    let (mut owner, signal, start) = owner(
        api.clone(),
        Instant::now() + Duration::from_secs(5),
        WorkerHooks {
            after_free: Some(hook),
            ..WorkerHooks::default()
        },
    );
    start.release();
    at_tail.recv_timeout(Duration::from_secs(3)).unwrap();
    let before = owner.observe_complete();
    let freed = owner.key_freed();
    drop(release);
    let after = join(&mut owner);
    assert!(freed);
    assert!(before.holds_original_join);
    assert_eq!(before.retirement, KeyRetirement::Pending);
    assert_eq!(after.retirement, KeyRetirement::Retired);
    verify_free(&api, &["free_provider"]);
    drop(signal);
}
#[test]
fn HostOpen_FakeHandleCannotReachNativeSigning_006() {
    let token = NativeRevocation::new();
    let api = Api::new(Mode::Good, token.clone());
    let (send, at_public) = mpsc::sync_channel(1);
    *api.public_signal.lock() = Some(send);
    let (mut owner, signal, start) = owner(
        api.clone(),
        Instant::now() + Duration::from_secs(5),
        WorkerHooks::default(),
    );
    start.release();
    at_public.recv_timeout(Duration::from_secs(3)).unwrap();
    let result = signal
        .begin(vec![1], SignatureEncoding::Fixed)
        .unwrap()
        .wait_blocking();
    assert!(result.is_err());
    assert!(token.is_revoked());
    join(&mut owner);
    verify_free(&api, &["free_key", "free_provider"]);
    drop(signal);
}

fn binding(cap: Instant) -> crate::native_crypto::host::FrozenHostBinding {
    use crate::native_crypto::host::{FrozenHostBinding, HostKeyProfileIdentity};
    let relay=hex("046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5");
    FrozenHostBinding {
        native_profile: HostKeyProfileIdentity {
            id: [7; 16],
            revision: 9,
        },
        endpoint_spki: public(),
        relay_spki: [SPKI_HEAD, &relay[1..]].concat().try_into().unwrap(),
        origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted("wss://relay.invalid/")
            .unwrap(),
        relay_id: [1; 16],
        profile_cap: cap,
    }
}
#[test]
fn HostOpen_ProfileFreezeHasNoNativeIoOrIssuer_007() {
    use crate::native_crypto::host_profile::NativeHostProfile;
    let b = binding(Instant::now() + Duration::from_secs(30));
    let token = NativeRevocation::new();
    let first = Api::new(Mode::Good, token.clone());
    let second = Api::new(Mode::Good, token);
    let mut name = String::from("TEST_ONLY_EXISTING");
    let profile = NativeHostProfile::new(b.clone(), &name, "TEST_ONLY_RELAY")
        .unwrap()
        .test_api(first.clone(), second.clone());
    name.clear();
    let frozen = profile.freeze();
    assert_eq!(frozen.binding().native_profile.revision, 9);
    assert_eq!(frozen.binding().endpoint_spki, b.endpoint_spki);
    assert!(first.events.lock().is_empty() && second.events.lock().is_empty());
    for (left, right) in [("", "relay"), ("same", "same"), ("bad\0name", "relay")] {
        assert!(NativeHostProfile::new(b.clone(), left, right).is_err());
    }
    let mut invalid = b.clone();
    invalid.relay_spki = invalid.endpoint_spki;
    assert!(NativeHostProfile::new(invalid, "endpoint", "relay").is_err());
    assert!(
        ExistingKeyLoader::new("TEST_ONLY_EXISTING", SigningPurpose::Issuer, public()).is_err()
    );
    drop(frozen);
    assert!(first.events.lock().is_empty() && second.events.lock().is_empty());
}
#[test]
fn HostOpen_SecondFailureClosesFirstWithoutUserExit_008() {
    use crate::{
        native_crypto::host_profile::NativeHostProfile,
        remote_host::activation::ActivationOperation,
    };
    let token = NativeRevocation::new();
    let cap = Instant::now() + Duration::from_secs(30);
    let b = binding(cap);
    let first = Api::new(Mode::Good, token.clone());
    let mut second = Api::new(Mode::OpenErr, token.clone());
    Arc::get_mut(&mut second).unwrap().name = "TEST_ONLY_RELAY".into();
    Arc::get_mut(&mut second).unwrap().public = b.relay_spki;
    let (sent, opened) = mpsc::sync_channel(1);
    *first.public_signal.lock() = Some(sent);
    let (second_hook, at_second, release_second) = gate();
    *second.open_hook.lock() = Some(second_hook);
    let (free_hook, at_free, release_free) = gate();
    *second.free_hook.lock() = Some(free_hook);
    let frozen = NativeHostProfile::new(b, "TEST_ONLY_EXISTING", "TEST_ONLY_RELAY")
        .unwrap()
        .test_api(first.clone(), second.clone())
        .freeze();
    let mut reservation = ActivationOperation::test_reservation();
    let operation = reservation.operation();
    let publisher = reservation.prepare_keys(token.clone(), cap).unwrap();
    let observer = operation.test_key_observer();
    let set = publisher.prepare(frozen).unwrap();
    opened.recv_timeout(Duration::from_secs(3)).unwrap();
    at_second.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(!token.is_revoked());
    drop(release_second);
    at_free.recv_timeout(Duration::from_secs(3)).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        if observer.snapshot().keys[0].is_some_and(|f| !f.holds_original_join) {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    let facts = observer.snapshot();
    assert!(token.is_revoked());
    assert!(facts.keys[1].unwrap().holds_original_join);
    assert_eq!(facts.keys[0].unwrap().retirement, KeyRetirement::Retired);
    drop(release_free);
    drop(set);
    drop(reservation);
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        let fact = operation.retire();
        if !fact.outstanding {
            assert!(!fact.unknown);
            break;
        }
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    verify_free(&first, &["free_key", "free_provider"]);
    verify_free(&second, &["free_provider"]);
}
#[test]
fn HostOpen_UnknownFreeRetainsOriginalWithoutRetry_009() {
    const FLAG: &str = "CC_DESK_TEST_EXISTING_FREE_UNKNOWN";
    if let Ok(mode) = std::env::var(FLAG) {
        let token = NativeRevocation::new();
        let mut api = Api::new(Mode::PropertyErr, token.clone());
        Arc::get_mut(&mut api).unwrap().free_fail = if mode == "provider" { 101 } else { 202 };
        Arc::get_mut(&mut api).unwrap().free_panic = mode == "panic";
        let (mut owner, _signal, start) = owner(
            api.clone(),
            Instant::now() + Duration::from_secs(5),
            WorkerHooks::default(),
        );
        start.release();
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            let fact = owner.observe_complete();
            if fact.retirement == KeyRetirement::Unknown {
                assert!(fact.holds_original_join);
                break;
            }
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
        assert!(token.is_revoked());
        for _ in 0..100 {
            let fact = owner.observe_complete();
            assert!(fact.holds_original_join);
            assert_eq!(fact.retirement, KeyRetirement::Unknown);
        }
        verify_free(
            &api,
            if mode == "provider" {
                &["free_key", "free_provider"]
            } else {
                &["free_key"]
            },
        );
        println!("TEST_ONLY_EXISTING_UNKNOWN_RETAINED:{mode}");
        // 有界负向 fixture 的进程结束不是原 owner 退休回执。
        std::process::exit(0);
    }
    struct Child(Option<std::process::Child>);
    impl Drop for Child {
        fn drop(&mut self) {
            if let Some(c) = self.0.as_mut() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
    for mode in ["key", "provider", "panic"] {
        use std::os::windows::process::CommandExt;
        let child=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","native_crypto::tests::existing_key_tests::HostOpen_UnknownFreeRetainsOriginalWithoutRetry_009","--nocapture","--test-threads=1"])
            .env(FLAG,mode).creation_flags(0x08000000).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        let mut child = Child(Some(child));
        let end = Instant::now() + Duration::from_secs(10);
        while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(Instant::now() < end, "owned child timeout");
            std::thread::yield_now();
        }
        let output = child.0.take().unwrap().wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout)
            .contains(&format!("TEST_ONLY_EXISTING_UNKNOWN_RETAINED:{mode}")));
    }
}
#[test]
fn HostOpen_AnonymousKeysSignThroughOwnedSlot_010() {
    let cap = Instant::now() + Duration::from_secs(10);
    let b = binding(cap);
    let token = NativeRevocation::new();
    let mut owned = Vec::new();
    let endpoint_scalar: [u8; 32] =
        hex("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721")
            .try_into()
            .unwrap();
    let mut relay_scalar = [0; 32];
    relay_scalar[31] = 1;
    for (purpose, spki, scalar, encoding) in [
        (
            SigningPurpose::Endpoint,
            b.endpoint_spki,
            endpoint_scalar,
            SignatureEncoding::Der,
        ),
        (
            SigningPurpose::Relay,
            b.relay_spki,
            relay_scalar,
            SignatureEncoding::Fixed,
        ),
    ] {
        let mut loader = ExistingKeyLoader::new("TEST_ONLY_ANONYMOUS", purpose, spki).unwrap();
        loader.test_anonymous(scalar);
        let mailbox = Arc::new(Mutex::new(Some(Opener::Existing(loader))));
        let (owner, signal, start) =
            KeyCustodian::prepare_host(mailbox, purpose, spki.to_vec(), token.clone(), cap)
                .unwrap();
        start.release();
        let signature = signal
            .begin(b"TEST_ONLY_EXISTING_SLOT_SIGN".to_vec(), encoding)
            .unwrap()
            .wait_blocking()
            .unwrap();
        let algorithm = if encoding == SignatureEncoding::Der {
            &ring::signature::ECDSA_P256_SHA256_ASN1
        } else {
            &ring::signature::ECDSA_P256_SHA256_FIXED
        };
        ring::signature::UnparsedPublicKey::new(algorithm, &spki[26..])
            .verify(b"TEST_ONLY_EXISTING_SLOT_SIGN", &signature)
            .unwrap();
        assert!(!token.is_revoked());
        assert_eq!(owner.native_sign_count(), 1);
        owned.push((owner, signal));
    }
    for (owner, _) in &owned {
        owner.shutdown();
    }
    for (owner, _) in &mut owned {
        assert_eq!(join(owner).retirement, KeyRetirement::Retired);
    }
}
#[test]
fn HostOpen_DeadlineDuringProviderCallRetainsPartial_011() {
    let token = NativeRevocation::new();
    let api = Api::new(Mode::Good, token.clone());
    let (hook, entered, release) = gate();
    *api.provider_hook.lock() = Some(hook);
    let cap = Instant::now() + Duration::from_secs(1);
    let (mut owner, signal, start) = owner(api.clone(), cap, WorkerHooks::default());
    start.release();
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    let (_send, wait) = mpsc::channel::<()>();
    assert!(wait
        .recv_timeout(cap.saturating_duration_since(Instant::now()))
        .is_err());
    assert!(owner.observe_complete().holds_original_join);
    drop(release);
    assert_eq!(join(&mut owner).retirement, KeyRetirement::Retired);
    assert!(token.is_revoked());
    assert_eq!(
        api.events.lock().iter().map(|e| e.name).collect::<Vec<_>>(),
        vec!["provider", "free_provider"]
    );
    verify_free(&api, &["free_provider"]);
    drop(signal);
}

#[path = "native_crypto_host_ready_tests.rs"]
mod ready;

pub(crate) fn single_run_keys(
    supplier: crate::native_crypto::host::FrozenHostSupplier,
    retire_hook: Option<Box<dyn FnOnce() + Send>>,
) -> crate::native_crypto::SingleRunTestKeys {
    use crate::native_crypto::host::{HostKeyCustody, NotifyHooks};
    let binding = supplier.binding().clone();
    let names = supplier.key_references().unwrap().clone();
    let token = NativeRevocation::new();
    let mut first = Api::new(Mode::Good, token.clone());
    {
        let a = Arc::get_mut(&mut first).unwrap();
        a.name = names[0].clone();
        a.public = binding.endpoint_spki;
    }
    let mut second = Api::new(Mode::Good, token.clone());
    {
        let a = Arc::get_mut(&mut second).unwrap();
        a.name = names[1].clone();
        a.public = binding.relay_spki;
    }
    *first.free_hook.lock() = retire_hook;
    let frozen = crate::native_crypto::host_profile::NativeHostProfile::new(
        binding.clone(),
        &names[0],
        &names[1],
    )
    .unwrap()
    .test_api(first, second)
    .freeze();
    let custody = HostKeyCustody::reserve_with(token, binding.profile_cap, NotifyHooks::default());
    let signers = custody.publisher().unwrap().prepare(frozen).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(Some(ready)) = custody.take_ready() {
            return crate::native_crypto::SingleRunTestKeys {
                custody,
                ready,
                signers,
            };
        }
        assert!(
            Instant::now() < end,
            "mock original workers failed readiness"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(crate) fn epoch_supplier(
    supplier: crate::native_crypto::host::FrozenHostSupplier,
    token: Arc<NativeRevocation>,
    generation: u64,
    trace: &crate::native_crypto::EpochTestTrace,
) -> crate::native_crypto::host::FrozenHostSupplier {
    let binding = supplier.binding().clone();
    let names = supplier.key_references().unwrap().clone();
    let mut first = Api::new(Mode::Good, token.clone());
    {
        let api = Arc::get_mut(&mut first).unwrap();
        api.name = names[0].clone();
        api.public = binding.endpoint_spki;
    }
    let second_mode = if generation == 2 {
        match trace.fault {
            crate::native_crypto::EpochTestFault::WrongSpki => Mode::PublicMismatch,
            crate::native_crypto::EpochTestFault::SecondOpen => Mode::OpenErr,
            _ => Mode::Good,
        }
    } else {
        Mode::Good
    };
    if generation == 1
        && matches!(
            trace.fault,
            crate::native_crypto::EpochTestFault::FreeUnknown
        )
    {
        Arc::get_mut(&mut first).unwrap().free_fail = 202;
    }
    let mut second = Api::new(second_mode, token);
    {
        let api = Arc::get_mut(&mut second).unwrap();
        api.name = names[1].clone();
        api.public = binding.relay_spki;
    }
    if generation == 2 {
        let action = trace.after_second_verified.clone();
        let events = trace.events.clone();
        *second.verified_hook.lock() = Some(Box::new(move || {
            events.lock().push((generation, "second_verified"));
            let action = action.lock().take();
            if let Some(action) = action {
                action();
            }
        }));
    }
    for (api, open, free) in [
        (&first, "endpoint_open", "endpoint_free"),
        (&second, "relay_open", "relay_free"),
    ] {
        let events = trace.events.clone();
        *api.open_hook.lock() = Some(Box::new(move || events.lock().push((generation, open))));
        let events = trace.events.clone();
        *api.free_hook.lock() = Some(Box::new(move || events.lock().push((generation, free))));
    }
    crate::native_crypto::host_profile::NativeHostProfile::new(binding, &names[0], &names[1])
        .unwrap()
        .test_api(first, second)
        .freeze()
}
