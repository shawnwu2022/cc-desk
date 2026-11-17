use super::*;
use crate::cli::{document::DocumentAuthority, run_registry::RunRegistry};
use std::sync::atomic::AtomicUsize;
fn document() -> (
    Arc<DocumentAuthority<NativeRun>>,
    Arc<DocumentBinding<NativeRun>>,
    CallerIdentity,
) {
    let url = "http://tauri.localhost/index.html".parse().unwrap();
    let a = DocumentAuthority::new(Arc::new(RunRegistry::new(2)), url).unwrap();
    let caller = a.test_caller();
    let mut table = tauri::ResourceTable::default();
    let binding = Arc::new(a.attach(&mut table).unwrap());
    let url = "http://tauri.localhost/index.html".parse().unwrap();
    a.started(&url);
    a.finished(&url);
    (a, binding, caller)
}
#[tokio::test]
async fn unpolled_old_creation_observer_cannot_join_a_new_attempt() {
    let (_a, binding, caller) = document();
    let service = Arc::new(RemoteClientService::default());
    let first = service
        .start_owned_creation(binding.clone(), caller.clone(), |_| async {
            Err(error("TEST_ONLY_FIRST"))
        })
        .unwrap();
    let stale = service.wait_creation(first);
    assert!(service.wait_creation(first).await.is_err());
    let (release, rx) = tokio::sync::oneshot::channel();
    let second = service
        .start_owned_creation(binding, caller, move |_| async move {
            rx.await.unwrap();
            Err(error("TEST_ONLY_SECOND"))
        })
        .unwrap();
    let old = tokio::time::timeout(Duration::from_millis(5), stale).await;
    release.send(()).unwrap();
    assert!(service.wait_creation(second).await.is_err());
    assert!(
        matches!(old,Ok(Err(e)) if e.code=="FORBIDDEN"),
        "an original observer must not rebind to the new creation slot"
    );
}
#[tokio::test]
async fn bootstrap_prestart_shutdown_calls_no_backend_and_unknown_factory_never_reopens() {
    let (_a, binding, caller) = document();
    let service = Arc::new(RemoteClientService::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    service.begin_shutdown();
    assert!(service
        .start_owned_creation(binding, caller, move |_| {
            observed.fetch_add(1, Ordering::AcqRel);
            async { Err(error("TEST_ONLY_UNUSED")) }
        })
        .is_err());
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert!(service.creation_join.lock().await.is_none());
    for panic in [false, true] {
        let (_a, binding, caller) = document();
        let service = Arc::new(RemoteClientService::default());
        let id = service
            .start_owned_creation(binding, caller, move |_| async move {
                assert!(!panic, "TEST_ONLY factory panic");
                Err(error("REMOTE_CLIENT_RETIRE_UNKNOWN"))
            })
            .unwrap();
        assert_eq!(
            service.wait_creation(id).await.unwrap_err().code,
            "REMOTE_CLIENT_RETIRE_UNKNOWN"
        );
        assert!(service.shutdown().await.is_err());
        assert!(service.begin_create().is_err());
    }
}
#[tokio::test]
async fn document_revocation_cancels_original_creation_and_seals_profile_admission() {
    let (a, binding, caller) = document();
    let service = Arc::new(RemoteClientService::default());
    let (entered, start) = tokio::sync::oneshot::channel();
    let id = service
        .start_owned_creation(binding, caller, move |mut stop| async move {
            entered.send(()).unwrap();
            stop.changed().await.unwrap();
            assert!(*stop.borrow());
            Err(error("TEST_ONLY_DOCUMENT_REVOKED"))
        })
        .unwrap();
    start.await.unwrap();
    a.revoke();
    assert_eq!(
        service.wait_creation(id).await.unwrap_err().code,
        "TEST_ONLY_DOCUMENT_REVOKED"
    );
    assert!(
        service.begin_create().is_err(),
        "a revoked original document cannot reopen native profile admission"
    );
}
#[tokio::test]
async fn bootstrap_observer_cancellation_retains_original_creation_and_admission() {
    let (_document, binding, caller) = document();
    let service = Arc::new(RemoteClientService::default());
    let (release, rx) = tokio::sync::oneshot::channel();
    let id = service
        .start_owned_creation(binding, caller, move |_stop| async move {
            rx.await.unwrap();
            Err(error("TEST_ONLY_BOOTSTRAP_REFUSED"))
        })
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(5), service.wait_creation(id))
            .await
            .is_err()
    );
    assert!(service.creation_join.lock().await.is_some());
    assert!(service.begin_create().is_err());
    release.send(()).unwrap();
    assert_eq!(
        service.wait_creation(id).await.unwrap_err().code,
        "TEST_ONLY_BOOTSTRAP_REFUSED"
    );
    assert!(service.creation_join.lock().await.is_none());
    assert!(service.begin_create().is_ok());
}
#[tokio::test]
async fn shutdown_waits_for_late_native_success_then_joins_original_instead_of_publishing() {
    let scope = cc_desk_remote_record::grant::SessionScope {
        session: "02".repeat(16),
        run: "TEST_ONLY_LATE_BOOTSTRAP".into(),
        generation: "3".into(),
    };
    let (c, h, _gate, _cancel) = current_test_channels::channels(scope).await;
    let r = h.host_install_request().await.unwrap();
    let (proof, done) = tokio::join!(c.client_installed(), h.complete_host_install(r, 7));
    done.unwrap();
    let client = NativeClient::from_installed(c, proof.unwrap()).unwrap();
    let (_document, binding, caller) = document();
    let service = Arc::new(RemoteClientService::default());
    let (release, rx) = tokio::sync::oneshot::channel();
    let (entered, started) = tokio::sync::oneshot::channel();
    service
        .start_owned_creation(binding.clone(), caller.clone(), move |_stop| async move {
            entered.send(()).unwrap();
            rx.await.unwrap();
            Ok(client)
        })
        .unwrap();
    started.await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(5), service.shutdown())
            .await
            .is_err()
    );
    assert!(service.creation_join.lock().await.is_some());
    release.send(()).unwrap();
    service.shutdown().await.unwrap();
    assert!(service.state.lock().resource.is_none());
    assert!(service.get(1, &caller, &binding).is_err());
    assert!(service.begin_create().is_err());
    h.close().await;
}

#[cfg(windows)]
#[test]
fn bootstrap_publication_gate_rejects_expired_and_replaced_attempts() {
    use crate::native_crypto::owner::SigningFence;
    let fence = SigningFence::new(std::time::Instant::now() + Duration::from_secs(3));
    let old = fence
        .advance(&[], std::time::Instant::now() + Duration::from_secs(1))
        .unwrap();
    let current = fence
        .advance(&[], std::time::Instant::now() + Duration::from_secs(1))
        .unwrap();
    assert!(old
        .publish_if_current(|_| panic!("stale publication"))
        .is_err());
    assert_eq!(current.publish_if_current(|_| 7).unwrap(), 7);
    let expired = fence
        .advance(&[], std::time::Instant::now() + Duration::from_millis(1))
        .unwrap();
    std::thread::sleep(Duration::from_millis(3));
    assert!(expired
        .publish_if_current(|_| panic!("expired publication"))
        .is_err());
}

#[cfg(windows)]
#[tokio::test]
async fn bootstrap_late_stale_result_stays_private_and_installed_owner_outlives_signing_end() {
    use crate::native_crypto::owner::SigningFence;
    for stale in [true, false] {
        let scope = cc_desk_remote_record::grant::SessionScope {
            session: "02".repeat(16),
            run: "TEST_ONLY_ATTEMPT_PUBLICATION".into(),
            generation: "3".into(),
        };
        let (c, h, _gate, _cancel) = current_test_channels::channels(scope).await;
        let request = h.host_install_request().await.unwrap();
        let (proof, done) = tokio::join!(c.client_installed(), h.complete_host_install(request, 7));
        done.unwrap();
        let client = NativeClient::from_installed(c, proof.unwrap()).unwrap();
        let (_document, binding, caller) = document();
        let service = Arc::new(RemoteClientService::default());
        let fence = SigningFence::new(std::time::Instant::now() + Duration::from_secs(3));
        let attempt = fence
            .advance(&[], std::time::Instant::now() + Duration::from_millis(200))
            .unwrap();
        let (release, rx) = tokio::sync::oneshot::channel();
        let id = service
            .start_owned_creation_checked(
                binding.clone(),
                caller.clone(),
                CreationAuthority::Native(attempt),
                move |_| async move {
                    rx.await.unwrap();
                    Ok(client)
                },
            )
            .unwrap();
        assert!(service.get(id, &caller, &binding).is_err());
        if stale {
            let _new = fence
                .advance(&[], std::time::Instant::now() + Duration::from_secs(1))
                .unwrap();
        }
        release.send(()).unwrap();
        let result = service.wait_creation(id).await;
        if stale {
            assert_eq!(result.unwrap_err().code, "NATIVE_SIGNING_ATTEMPT_INVALID");
            assert!(service.get(id, &caller, &binding).is_err());
            assert!(
                service.state.lock().resource.is_none(),
                "original private result must be joined before custody clears"
            );
        } else {
            result.unwrap();
            tokio::time::sleep(Duration::from_millis(210)).await;
            let original = service.get(id, &caller, &binding).unwrap();
            assert!(!original.closed.load(Ordering::Acquire));
            assert!(service
                .fresh_native_signers(std::time::Instant::now() + Duration::from_secs(1))
                .is_err());
            service.close_resource(original).await.unwrap();
        }
        h.close().await;
    }
}

#[cfg(windows)]
#[tokio::test]
async fn bootstrap_private_custody_refuses_data_before_and_after_failed_publication() {
    use crate::native_crypto::owner::SigningFence;
    let scope = cc_desk_remote_record::grant::SessionScope {
        session: "02".repeat(16),
        run: "TEST_ONLY_PRIVATE_CUSTODY".into(),
        generation: "3".into(),
    };
    let (c, h, _gate, _cancel) = current_test_channels::channels(scope).await;
    let request = h.host_install_request().await.unwrap();
    let (proof, done) = tokio::join!(c.client_installed(), h.complete_host_install(request, 7));
    done.unwrap();
    let client = NativeClient::from_installed(c, proof.unwrap()).unwrap();
    let (_document, binding, caller) = document();
    let service = RemoteClientService::default();
    let resource = OwnedClient::new(1, caller.clone(), &binding, client);
    service.state.lock().resource = Some(resource.clone());
    assert!(service.get(1, &caller, &binding).is_err());
    let fence = SigningFence::new(std::time::Instant::now() + Duration::from_secs(2));
    let old = fence
        .advance(&[], std::time::Instant::now() + Duration::from_secs(1))
        .unwrap();
    let _new = fence
        .advance(&[], std::time::Instant::now() + Duration::from_secs(1))
        .unwrap();
    assert!(service
        .publish_owned(resource.clone(), &binding, CreationAuthority::Native(old))
        .is_err());
    assert!(service.get(1, &caller, &binding).is_err());
    assert!(service
        .state
        .lock()
        .resource
        .as_ref()
        .is_some_and(|original| Arc::ptr_eq(original, &resource)));
    assert!(
        resource.join.lock().await.is_some(),
        "failed publication retains the original retirement worker"
    );
    service.close_resource(resource).await.unwrap();
    assert!(service.state.lock().resource.is_none());
    h.close().await;
}

#[cfg(windows)]
#[tokio::test]
async fn bootstrap_publication_waiting_for_service_lock_cannot_cross_attempt_end() {
    use crate::native_crypto::owner::SigningFence;
    let scope = cc_desk_remote_record::grant::SessionScope {
        session: "02".repeat(16),
        run: "TEST_ONLY_LOCKED_PUBLICATION".into(),
        generation: "3".into(),
    };
    let (c, h, _gate, _cancel) = current_test_channels::channels(scope).await;
    let request = h.host_install_request().await.unwrap();
    let (proof, done) = tokio::join!(c.client_installed(), h.complete_host_install(request, 7));
    done.unwrap();
    let client = NativeClient::from_installed(c, proof.unwrap()).unwrap();
    let (_document, binding, caller) = document();
    let service = Arc::new(RemoteClientService::default());
    let resource = OwnedClient::new(1, caller.clone(), &binding, client);
    service.state.lock().resource = Some(resource.clone());
    let fence = SigningFence::new(std::time::Instant::now() + Duration::from_secs(2));
    let attempt = fence
        .advance(&[], std::time::Instant::now() + Duration::from_millis(100))
        .unwrap();
    let (entered, arrival) = std::sync::mpsc::channel();
    attempt.hook_admission(Box::new(move || {
        entered.send(()).unwrap();
    }));
    let held = service.state.lock();
    let original_service = service.clone();
    let original_resource = resource.clone();
    let original_binding = binding.clone();
    let publication = std::thread::spawn(move || {
        original_service.publish_owned(
            original_resource,
            &original_binding,
            CreationAuthority::Native(attempt),
        )
    });
    arrival.recv_timeout(Duration::from_secs(1)).unwrap();
    std::thread::sleep(Duration::from_millis(110));
    drop(held);
    let result = publication.join().unwrap();
    let invisible = service.get(1, &caller, &binding).is_err();
    let retained = service
        .state
        .lock()
        .resource
        .as_ref()
        .is_some_and(|r| Arc::ptr_eq(r, &resource));
    service.close_resource(resource).await.unwrap();
    h.close().await;
    assert!(result.is_err() && invisible && retained,"expiry while waiting for service lock must withhold publication and retain original cleanup owner");
}
