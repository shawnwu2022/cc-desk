//! 两个原 ExistingKeyLoader + 全拦截 MockKeyApi；不调用真实 CNG。
use super::*;
use crate::native_crypto::host::{
    FrozenHostBinding, FrozenHostSupplier, HostKeyCustody, HostKeyProfileIdentity, HostKeysReady,
    NotifyHooks,
};
fn fixture(
    mode: Mode,
) -> (
    HostKeyCustody,
    FrozenHostSupplier,
    FrozenHostBinding,
    [Arc<Api>; 2],
) {
    fixture_options(mode, Duration::from_secs(10), false)
}
fn fixture_options(
    mode: Mode,
    duration: Duration,
    wrong_purpose: bool,
) -> (
    HostKeyCustody,
    FrozenHostSupplier,
    FrozenHostBinding,
    [Arc<Api>; 2],
) {
    let token = NativeRevocation::new();
    let end = Instant::now() + duration;
    let endpoint = public();
    let relay: [u8; 91] = crate::remote_native_pairing_store::tests::record()
        .native
        .relay_spki
        .try_into()
        .unwrap();
    let binding = FrozenHostBinding {
        native_profile: HostKeyProfileIdentity {
            id: [7; 16],
            revision: 1,
        },
        endpoint_spki: endpoint,
        relay_spki: relay,
        origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted("wss://localhost/relay")
            .unwrap(),
        relay_id: [8; 16],
        profile_cap: end,
    };
    let mut a = Api::new(Mode::Good, token.clone());
    Arc::get_mut(&mut a).unwrap().name = "TEST_ONLY_EP".into();
    let mut b = Api::new(mode, token.clone());
    {
        let b = Arc::get_mut(&mut b).unwrap();
        b.name = "TEST_ONLY_RELAY".into();
        b.public = relay;
    }
    let mut ep = ExistingKeyLoader::new(
        "TEST_ONLY_EP",
        if wrong_purpose {
            SigningPurpose::Relay
        } else {
            SigningPurpose::Endpoint
        },
        endpoint,
    )
    .unwrap();
    ep.test_api(a.clone());
    let mut re = ExistingKeyLoader::new("TEST_ONLY_RELAY", SigningPurpose::Relay, relay).unwrap();
    re.test_api(b.clone());
    let supplier = FrozenHostSupplier::from_openers(
        binding.clone(),
        Opener::Existing(ep),
        Opener::Existing(re),
    );
    (
        HostKeyCustody::reserve_with(token, end, NotifyHooks::default()),
        supplier,
        binding,
        [a, b],
    )
}
fn ready(owner: &HostKeyCustody) -> Option<HostKeysReady> {
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        match owner.take_ready() {
            Ok(Some(receipt)) => return Some(receipt),
            Err(_) => return None,
            Ok(None) => {}
        }
        if Instant::now() >= end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn retire(owner: &HostKeyCustody) {
    owner.revoke();
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        let observation = owner.advance();
        if !observation.outstanding {
            assert!(observation.disposal_complete);
            assert!(!observation.unknown);
            return;
        }
        assert!(
            Instant::now() < end,
            "original key/notification workers did not join"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn HostReady_ActualExistingLoadersProduceSingleReceipt_001() {
    let (owner, supplier, binding, apis) = fixture(Mode::Good);
    let set = owner.publisher().unwrap().prepare(supplier).unwrap();
    let receipt = ready(&owner);
    let valid = receipt
        .as_ref()
        .is_some_and(|r| r.check(&owner, &binding).is_ok());
    let single = receipt.is_some() && owner.take_ready().is_err();
    let consumed = receipt
        .map(|r| r.consume(&owner, &binding).is_ok())
        .unwrap_or(false);
    drop(set);
    retire(&owner);
    verify_free(&apis[0], &["free_key", "free_provider"]);
    verify_free(&apis[1], &["free_key", "free_provider"]);
    assert!(
        valid && single && consumed,
        "both verified original loaders must yield exactly one live receipt"
    );
}

#[test]
fn HostReady_FirstVerifiedSecondPendingOrFailedNeverAggregates_002() {
    for mode in [Mode::Good, Mode::OpenErr, Mode::PublicMismatch, Mode::Usage] {
        let (owner, supplier, binding, apis) = fixture(mode);
        let (entered_tx, entered_rx) = mpsc::sync_channel(0);
        let (release_tx, release_rx) = mpsc::sync_channel(0);
        *apis[1].open_hook.lock() = Some(Box::new(move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }));
        let set = owner.publisher().unwrap().prepare(supplier).unwrap();
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        while !owner.test_worker_opened(0) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(2));
        }
        let first_ready = owner.test_worker_opened(0);
        let pending = matches!(owner.take_ready(), Ok(None));
        release_tx.send(()).unwrap();
        let receipt = ready(&owner);
        let result = receipt
            .map(|r| r.consume(&owner, &binding).is_ok())
            .unwrap_or(false);
        drop(set);
        retire(&owner);
        verify_free(&apis[0], &["free_key", "free_provider"]);
        verify_free(
            &apis[1],
            if mode == Mode::OpenErr {
                &["free_provider"]
            } else {
                &["free_key", "free_provider"]
            },
        );
        assert!(first_ready && pending);
        assert_eq!(result, mode == Mode::Good);
    }
}
#[test]
fn HostReady_LateVerifiedCallbackAfterCancelCannotRevive_003() {
    let (owner, supplier, _binding, apis) = fixture(Mode::Good);
    let (entered_tx, entered_rx) = mpsc::sync_channel(0);
    let (release_tx, release_rx) = mpsc::sync_channel(0);
    *apis[1].verified_hook.lock() = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let set = owner.publisher().unwrap().prepare(supplier).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let first_end = Instant::now() + Duration::from_secs(2);
    while !owner.test_worker_opened(0) && Instant::now() < first_end {
        std::thread::sleep(Duration::from_millis(2));
    }
    let first_ready = owner.test_worker_opened(0);
    let pending = matches!(owner.take_ready(), Ok(None));
    owner.revoke();
    let outstanding = owner.advance().outstanding;
    release_tx.send(()).unwrap();
    let absent = ready(&owner).is_none();
    drop(set);
    retire(&owner);
    verify_free(&apis[0], &["free_key", "free_provider"]);
    verify_free(&apis[1], &["free_key", "free_provider"]);
    assert!(first_ready && pending && outstanding && absent);
}
#[test]
fn HostReady_ForeignCustodyProfileAndCancellationReject_004() {
    let (owner, supplier, binding, _apis) = fixture(Mode::Good);
    let set = owner.publisher().unwrap().prepare(supplier).unwrap();
    let receipt = ready(&owner).unwrap();
    let (foreign, other_supplier, _other_binding, _other_apis) = fixture(Mode::Good);
    let other_set = foreign
        .publisher()
        .unwrap()
        .prepare(other_supplier)
        .unwrap();
    let foreign_rejected = receipt.check(&foreign, &binding).is_err();
    let mut changed = binding.clone();
    changed.native_profile.revision += 1;
    let profile_rejected = receipt.check(&owner, &changed).is_err();
    let mut swapped = binding.clone();
    std::mem::swap(&mut swapped.endpoint_spki, &mut swapped.relay_spki);
    let swap_rejected = receipt.check(&owner, &swapped).is_err();
    owner.revoke();
    let cancel_rejected = receipt.check(&owner, &binding).is_err();
    drop(set);
    drop(other_set);
    retire(&owner);
    retire(&foreign);
    let retired_rejected = receipt.consume(&owner, &binding).is_err();
    assert!(
        foreign_rejected
            && profile_rejected
            && swap_rejected
            && cancel_rejected
            && retired_rejected
    );
}

#[test]
fn HostReady_VerifiedAfterDeadlineCannotPublish_005() {
    let (owner, supplier, binding, apis) =
        fixture_options(Mode::Good, Duration::from_secs(1), false);
    let (entered_tx, entered_rx) = mpsc::sync_channel(0);
    let (release_tx, release_rx) = mpsc::sync_channel(0);
    *apis[1].verified_hook.lock() = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let set = owner.publisher().unwrap().prepare(supplier).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    std::thread::sleep(
        binding
            .profile_cap
            .saturating_duration_since(Instant::now())
            + Duration::from_millis(5),
    );
    let pending_retirement = owner.advance().outstanding;
    release_tx.send(()).unwrap();
    let absent = ready(&owner).is_none();
    drop(set);
    retire(&owner);
    verify_free(&apis[1], &["free_key", "free_provider"]);
    assert!(pending_retirement && absent);
}
#[test]
fn HostReady_ActualLoaderWrongPurposeRejects_006() {
    let (owner, supplier, _binding, apis) =
        fixture_options(Mode::Good, Duration::from_secs(10), true);
    let set = owner.publisher().unwrap().prepare(supplier).unwrap();
    let absent = ready(&owner).is_none();
    drop(set);
    retire(&owner);
    // 用途不符在原 loader 开任何 native handle 前拒绝。
    assert!(absent);
    assert!(apis[0].events.lock().is_empty());
}
#[test]
fn HostReady_SuccessfulFixtureFactoryIsNotVerifiedLoader_007() {
    let (owner, _supplier, binding, _apis) = fixture(Mode::Good);
    let (tx, rx) = mpsc::sync_channel(2);
    let factory = |purpose, spki, tx: mpsc::SyncSender<()>| {
        Box::new(move || {
            // 空测试 slot 无 native handle，不触发 CNG 或真实密钥访问。
            let key = NativeOwnedSoftwareKey {
                provider: Default::default(),
                key: Default::default(),
                approved_purpose: purpose,
                approved_spki: spki,
                silent_metadata: true,
                worker: std::thread::current().id(),
                unknown: false,
                _not_send_or_sync: Default::default(),
                free_effect: None,
                mock_api: None,
            };
            tx.send(()).unwrap();
            Ok(key)
        }) as Box<dyn FnOnce() -> Result<NativeOwnedSoftwareKey, CryptoError> + Send>
    };
    let supplier = FrozenHostSupplier::test_new(
        binding.clone(),
        factory(SigningPurpose::Endpoint, binding.endpoint_spki, tx.clone()),
        factory(SigningPurpose::Relay, binding.relay_spki, tx),
    );
    let set = owner.publisher().unwrap().prepare(supplier).unwrap();
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let absent = ready(&owner).is_none();
    drop(set);
    retire(&owner);
    assert!(absent);
}
