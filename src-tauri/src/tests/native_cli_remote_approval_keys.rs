#![allow(non_snake_case)]
use super::*;
use crate::remote_host_approval::{FrozenHostApproval, NativeHostApproval};
use crate::remote_host_controller::{HostControlReply, RemoteHostController};
use crate::remote_host_network::*;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) fn description(
    relay: &NetworkRelayFixture,
    admission: &crate::run_supervisor::NativeRunAdmission,
    clock: Arc<dyn cc_desk_remote_security::GrantClock>,
) -> crate::remote_host_approval::HostApprovalDescription {
    let (_send, revoked) = tokio::sync::watch::channel(false);
    let source = Arc::new(ApprovedHostSource {
        clock,
        revoked,
        expires_host_ms: 61000,
    });
    let (owner, material) = NativeHostApproval::test_from_inputs(
        network_inputs(relay, admission.scope().unwrap(), source),
        admission,
        true,
    );
    drop(material);
    owner.test_take_description()
}
fn counts() -> Arc<[AtomicUsize; 2]> {
    Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)])
}
fn supplier(
    relay: &NetworkRelayFixture,
    opened: Arc<[AtomicUsize; 2]>,
) -> crate::native_crypto::host::FrozenHostSupplier {
    crate::native_crypto::test_host_supplier(
        cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&relay.origin).unwrap(),
        Instant::now() + Duration::from_secs(60),
        opened,
    )
}
fn frozen(
    relay: &NetworkRelayFixture,
    admission: &crate::run_supervisor::NativeRunAdmission,
    opened: Arc<[AtomicUsize; 2]>,
    clock: Arc<dyn cc_desk_remote_security::GrantClock>,
) -> FrozenHostApproval {
    FrozenHostApproval::supplied(
        description(relay, admission, clock),
        admission,
        Instant::now() + Duration::from_secs(60),
        supplier(relay, opened),
    )
    .unwrap()
}
async fn finish(controller: &RemoteHostController) -> bool {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(result) = controller.test_finish_preparation() {
                return result;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn NativeRemote_ApprovedNativeKeysRegisterAndHandshake_012() {
    approved_native_keys_trial(false, false, None, false, false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn NativeRemote_PreparedKeysWaitBeyondSignatureWindow_014() {
    approved_native_keys_trial(true, false, None, false, false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn NativeRemote_LateClientHelloUsesOriginalEndpointStage_015() {
    approved_native_keys_trial(false, true, None, false, false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn NativeRemote_StageGateHonorsOriginalSetupEnd_016() {
    approved_native_keys_trial(false, false, Some(false), false, false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn NativeRemote_StageGateCancelKeepsOriginalOwners_017() {
    approved_native_keys_trial(false, false, Some(true), false, false).await;
}
async fn approved_native_keys_trial(
    delay_network: bool,
    delay_hello: bool,
    gate_failure: Option<bool>,
    existing_adapter: bool,
    autonomous: bool,
) {
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let mut fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let url: tauri::Url = "http://tauri.localhost/".parse().unwrap();
    let document = crate::cli::document::DocumentAuthority::new(
        fixture.service.registry().clone(),
        url.clone(),
    )
    .unwrap();
    let mut resources = tauri::ResourceTable::default();
    let binding = Arc::new(document.attach(&mut resources).unwrap());
    document.started(&url);
    document.finished(&url);
    fixture.caller = document.test_caller();
    let host = Arc::new(RemoteHostService::default());
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
    let status = fixture.start();
    let mut cleanup = Cleanup {
        supervisor: fixture.supervisor.clone(),
        host: host.clone(),
        run: status.run.clone(),
        pin: Some(fixture.supervisor.pin_shutdown(&status.run)),
        stop: None,
        proofs: Vec::new(),
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let relay_point = network_hex("046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5");
    let mut relay = NetworkRelayFixture::start_with_host_point(Some(relay_point)).await;
    let admit = || {
        fixture
            .supervisor
            .admit_remote_run(
                fixture
                    .service
                    .access(&fixture.caller, &status.run)
                    .unwrap(),
                &status.run,
            )
            .unwrap()
    };
    let original = admit();
    let trial_supplier = |opened| {
        if existing_adapter {
            crate::native_crypto::test_existing_host_supplier(
                cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&relay.origin).unwrap(),
                Instant::now() + Duration::from_secs(60),
                opened,
            )
        } else {
            supplier(&relay, opened)
        }
    };
    // 全部公开描述检查必须在任一 opener 运行前完成。
    for mismatch in 0..5 {
        let opened = counts();
        let mut fields = description(&relay, &original, Arc::new(|| Ok(1000)));
        match mismatch {
            0 => fields.endpoint_id = [0; 32],
            1 => fields.endpoint_role = cc_desk_remote_security::Role::Client,
            2 => fields.relay_binding.0 = [8; 16],
            3 => {
                fields.binding.origin = cc_desk_remote_record::appbind::CanonicalOrigin::trusted(
                    "wss://wrong.example/relay",
                )
                .unwrap()
            }
            _ => fields.relay_binding.1 = cc_desk_remote_transport::wss::RelayRole::Phone,
        }
        assert!(FrozenHostApproval::supplied(
            fields,
            &original,
            Instant::now() + Duration::from_secs(60),
            trial_supplier(opened.clone())
        )
        .is_err());
        assert_eq!(opened.each_ref().map(|n| n.load(Ordering::Acquire)), [0, 0]);
    }
    if autonomous {
        use std::{
            future::Future,
            task::{Context, Wake, Waker},
        };
        struct ExpiryWake {
            controller: std::sync::Weak<RemoteHostController>,
            token: Arc<crate::native_crypto::owner::NativeRevocation>,
            observed: Arc<AtomicUsize>,
        }
        impl Wake for ExpiryWake {
            fn wake(self: Arc<Self>) {
                let unlocked = self
                    .controller
                    .upgrade()
                    .is_some_and(|c| c.test_state_lock_available());
                self.observed.store(
                    if unlocked && self.token.is_revoked() {
                        1
                    } else {
                        2
                    },
                    Ordering::Release,
                );
            }
        }
        let expiring = Arc::new(RemoteHostController::default());
        let rejected_service = Arc::new(RemoteHostService::default());
        let unopened = counts();
        expiring
            .install_frozen_configuration(
                &binding,
                fixture.caller.clone(),
                &original,
                FrozenHostApproval::supplied(
                    description(&relay, &original, Arc::new(|| Ok(1000))),
                    &original,
                    Instant::now() + Duration::from_secs(60),
                    trial_supplier(unopened.clone()),
                )
                .unwrap(),
            )
            .unwrap();
        expiring
            .test_request_frozen(&binding, &fixture.caller, &original, true)
            .unwrap();
        let expired = expiring.test_approval_owner();
        let expired_token = expired.test_native_token();
        let observed = Arc::new(AtomicUsize::new(0));
        let waker = Waker::from(Arc::new(ExpiryWake {
            controller: Arc::downgrade(&expiring),
            token: expired_token.clone(),
            observed: observed.clone(),
        }));
        let mut receiver = expired.test_watch_receiver();
        let mut changed = Box::pin(receiver.changed());
        assert!(changed
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
        expired.test_expire();
        let refused = expiring.prepare_owned(
            &binding,
            &fixture.caller,
            admit(),
            runtime.clone(),
            rejected_service.clone(),
        );
        assert!(refused.is_err());
        assert_eq!(
            observed.load(Ordering::Acquire),
            1,
            "expiry wake must see token revoked and controller lock available"
        );
        assert!(rejected_service.test_activation_observation().is_none());
        assert_eq!(
            unopened.each_ref().map(|n| n.load(Ordering::Acquire)),
            [0, 0]
        );
        eprintln!("TEST_ONLY_DRIVER_EXPIRY lock_available=true revoked_before_wake=true reservation=false opens=[0,0]");
    }
    let opened = counts();
    let controller = RemoteHostController::default();
    assert_eq!(
        controller
            .current(&binding, &fixture.caller, &original)
            .unwrap(),
        HostControlReply::Locked
    );
    let clock_threads = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let clock_log = clock_threads.clone();
    let query_thread = std::thread::current().id();
    let frozen = FrozenHostApproval::supplied(
        description(
            &relay,
            &original,
            Arc::new(move || {
                clock_log.lock().push(std::thread::current().id());
                Ok(1000)
            }),
        ),
        &original,
        Instant::now() + Duration::from_secs(60),
        trial_supplier(opened.clone()),
    )
    .unwrap();
    controller
        .install_frozen_configuration(&binding, fixture.caller.clone(), &original, frozen)
        .unwrap();
    assert_eq!(
        controller
            .test_request_frozen(&binding, &fixture.caller, &original, true)
            .unwrap(),
        HostControlReply::ApprovalRecorded
    );
    let owner = controller.test_approval_owner();
    let token = owner.test_native_token();
    assert!(!token.is_revoked());
    assert_eq!(opened.each_ref().map(|n| n.load(Ordering::Acquire)), [0, 0]);
    if autonomous {
        let mut foreign = fixture.caller.clone();
        foreign.window_label.push_str("-TEST_ONLY_FOREIGN");
        assert!(controller
            .prepare_owned(&binding, &foreign, admit(), runtime.clone(), host.clone())
            .is_err());
        assert!(!token.is_revoked() && host.test_activation_observation().is_none());
        assert_eq!(
            controller
                .current(&binding, &fixture.caller, &original)
                .unwrap(),
            HostControlReply::ApprovalRecorded
        );
    }
    clock_threads.lock().clear();
    let (completed, completion) = tokio::sync::oneshot::channel();
    if autonomous {
        controller.test_driver_hooks(crate::remote_host::driver::DriverHooks {
            after_work: Some(Box::new(move || {
                let _ = completed.send(());
            })),
            ..Default::default()
        });
    } else {
        drop(completed);
    }
    if autonomous {
        controller
            .prepare_owned(
                &binding,
                &fixture.caller,
                admit(),
                runtime.clone(),
                host.clone(),
            )
            .unwrap();
    } else {
        controller
            .test_prepare_owned(
                &binding,
                &fixture.caller,
                admit(),
                runtime.clone(),
                host.clone(),
            )
            .unwrap();
    }
    let duplicate = if autonomous {
        controller.prepare_owned(
            &binding,
            &fixture.caller,
            admit(),
            runtime.clone(),
            host.clone(),
        )
    } else {
        controller.test_prepare_owned(
            &binding,
            &fixture.caller,
            admit(),
            runtime.clone(),
            host.clone(),
        )
    };
    assert!(duplicate.is_err());
    assert!(
        !token.is_revoked(),
        "duplicate must not cancel original claim"
    );
    let registered = if autonomous {
        // 等原 driver 完成信号期间完全不读 current，不用观察者帮助推进。
        tokio::time::timeout(Duration::from_secs(5), completion)
            .await
            .is_ok_and(|r| r.is_ok())
    } else {
        finish(&controller).await
    };
    if !registered {
        let prior = controller
            .current(&binding, &fixture.caller, &original)
            .unwrap();
        controller.begin_shutdown();
        // 先按原保管链收口，再断言；读取状态不替代启动推进。
        if autonomous {
            let _ =
                tokio::time::timeout(Duration::from_secs(5), host.test_retire_activation()).await;
        } else {
            let _ = finish(&controller).await;
        }
        drop(original);
        let physical = cleanup.close();
        let relay_known = relay.close().await;
        eprintln!("TEST_ONLY_AUTONOMOUS_REGISTER registered={registered} prior={prior:?} physical={physical} relay={relay_known}");
        assert!(physical && relay_known);
        assert!(registered, "prepare did not autonomously register");
        return;
    }

    assert_eq!(
        controller
            .current(&binding, &fixture.caller, &original)
            .unwrap(),
        HostControlReply::Registered
    );
    if autonomous {
        let calls = clock_threads.lock().clone();
        assert_eq!(calls.len(), 2, "claim and registration sample exactly once");
        assert!(calls.iter().all(|thread| *thread != query_thread));
        assert_ne!(
            calls[0], calls[1],
            "prepare must join before separate registration driver"
        );
        for _ in 0..8 {
            assert_eq!(
                controller
                    .current(&binding, &fixture.caller, &original)
                    .unwrap(),
                HostControlReply::Registered
            );
        }
        assert_eq!(
            clock_threads.lock().len(),
            2,
            "current must not sample clock or register"
        );
    }
    assert!(
        host.test_activation_observation().unwrap().outstanding,
        "business registration is not key retirement"
    );
    assert!(!token.is_revoked());
    // 相同公开 key、另一批准的材料不得消耗 B 的 fresh 槽或撤销 B；退回 A 仍由 A 原 owner 保管。
    if !delay_network && !delay_hello && gate_failure.is_none() {
        let mut a_reservation =
            crate::remote_host::activation::ActivationOperation::test_reservation();
        let a_operation = a_reservation.operation();
        let a_token = crate::native_crypto::owner::NativeRevocation::new();
        let supplied = trial_supplier(counts());
        let a_spki = supplied.binding().endpoint_spki;
        let a_set = a_reservation
            .prepare_keys(a_token.clone(), Instant::now() + Duration::from_secs(30))
            .unwrap()
            .prepare(supplied)
            .unwrap();
        a_reservation.spawn(|_| true).unwrap();
        let material = HostAttemptMaterials {
            stages: Some(a_set.stages),
            local: cc_desk_remote_security::LocalIdentity::new(
                cc_desk_remote_security::Role::Host,
                cc_desk_remote_security::P256Spki::new(&a_spki).unwrap(),
                a_set.endpoint,
            )
            .unwrap(),
            relay: a_set.relay,
            outer_accepted: None,
            termination: None,
        };
        let refused = host
            .replenish_native_source(
                &runtime,
                &original,
                &host.test_registered_source_token(),
                material,
            )
            .err()
            .unwrap();
        assert_eq!(refused.0, HostNetworkError::Authority);
        assert!(
            !token.is_revoked() && !a_token.is_revoked() && host.test_source_material_pending()
        );
        drop(refused);
        assert!(a_token.is_revoked() && !token.is_revoked() && host.test_source_material_pending());
        let end = Instant::now() + Duration::from_secs(5);
        while a_operation.retire().outstanding {
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        assert!(!a_operation.snapshot().unknown);
    }
    let (accepted, ready) = tokio::sync::oneshot::channel();
    let (terminated, ending) = tokio::sync::oneshot::channel();
    host.test_observe_fresh_material(accepted, terminated);
    let mut proceed = if delay_hello || gate_failure.is_some() {
        let (release, wait) = tokio::sync::oneshot::channel();
        host.test_gate_fresh_after_accepted(wait);
        Some(release)
    } else {
        None
    };
    if delay_network {
        // 测量真实旧窗口过期，不用 sleep 安排线程先后；原批准 cap 不变。
        let release = tokio::time::Instant::now() + Duration::from_millis(5200);
        tokio::time::sleep_until(release).await;
        assert!(tokio::time::Instant::now() >= release);
        assert!(!token.is_revoked());
    }
    let attempt = host
        .test_begin_registered_network(runtime.clone(), admit())
        .unwrap();
    let mut peer = None;
    let result: Result<(), &'static str> = async {
        ready.await.map_err(|_| "native relay acceptance")?;
        if let Some(cancel_gate) = gate_failure {
            if cancel_gate {
                drop(proceed.take());
            }
            let failure = attempt.wait_admitted().await;
            let expected = if cancel_gate {
                HostNetworkError::Closed
            } else {
                HostNetworkError::Deadline
            };
            if failure != Err(expected) {
                return Err("original setup gate did not refuse");
            }
            attempt
                .wait_closed()
                .await
                .map_err(|_| "gate original join")?;
            if !token.is_revoked() {
                return Err("failed stage retained authority");
            }
            return Ok(());
        }
        if delay_hello {
            // 原任务在 Accepted 后、mux 创建前受门保管；避免独立 2 秒 IO idle timeout 掩盖签名窗口。
            // 保留原 8 秒 setup end，晚于旧五秒窗才启动 Phone。
            let release = tokio::time::Instant::now() + Duration::from_millis(5200);
            tokio::time::sleep_until(release).await;
            assert!(tokio::time::Instant::now() >= release);
            proceed
                .take()
                .unwrap()
                .send(())
                .map_err(|_| "original setup gate closed")?;
        }
        let (phone, admitted) = tokio::join!(
            network_client(&relay, original.scope().unwrap()),
            attempt.wait_admitted()
        );
        peer = Some(phone?);
        admitted.map_err(|_| "native handshake handoff")?;
        if opened.each_ref().map(|n| n.load(Ordering::Acquire)) != [1, 1] {
            return Err("two original anonymous workers only");
        }
        if token.is_revoked() {
            return Err("successful registration revoked keys");
        }
        owner.test_revoke_only();
        if !token.is_revoked() || owner.test_watch_revoked() {
            return Err("original lease token not shared");
        }
        tokio::time::timeout(Duration::from_secs(2), attempt.wait_closed())
            .await
            .map_err(|_| "native token close timeout")?
            .map_err(|_| "native token join")?;
        Ok(())
    }
    .await;
    attempt.request_close();
    if let Some(peer) = peer.take() {
        let _ = retire_network_peer(peer, &attempt).await;
    }
    let _ = attempt.wait_closed().await;
    let ending = ending.await.unwrap();
    let network_known = host.test_join_network().await;
    controller.begin_shutdown();
    if autonomous {
        assert!(
            tokio::time::timeout(Duration::from_secs(5), host.test_retire_activation())
                .await
                .unwrap()
        );
    }
    drop(original);
    let physical = cleanup.close();
    let relay_known = relay.close().await;
    eprintln!("TEST_ONLY_FROZEN_APPROVAL result={result:?} cause={:?} mux={:?} opens={:?} token_revoked={} network_joined={network_known}", ending.cause, ending.mux, opened.each_ref().map(|n| n.load(Ordering::Acquire)), token.is_revoked());
    assert!(physical && relay_known && network_known);
    assert!(result.is_ok(), "{result:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn NativeRemote_OriginalApprovalClaimFailureCustody_013() {
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let status = fixture.start();
    let host = Arc::new(RemoteHostService::default());
    let mut cleanup = Cleanup {
        supervisor: fixture.supervisor.clone(),
        host,
        run: status.run.clone(),
        pin: Some(fixture.supervisor.pin_shutdown(&status.run)),
        stop: None,
        proofs: Vec::new(),
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut relay = NetworkRelayFixture::start().await;
    let admission = fixture
        .supervisor
        .admit_remote_run(
            fixture
                .service
                .access(&fixture.caller, &status.run)
                .unwrap(),
            &status.run,
        )
        .unwrap();
    // claim 前拒绝无副作用；真正取得一次性 claim 后的每个失败先撤销原 token。
    for mode in 0..9 {
        let opened = counts();
        let fault = Arc::new(AtomicUsize::new(0));
        let read = fault.clone();
        let reenter: Arc<parking_lot::Mutex<Option<std::sync::Weak<NativeHostApproval>>>> =
            Arc::new(parking_lot::Mutex::new(None));
        let callback = reenter.clone();
        let clock = Arc::new(move || match read.load(Ordering::Acquire) {
            1 => Err(cc_desk_remote_security::GrantError::Clock),
            2 => panic!("TEST_ONLY_CLAIM_CLOCK_PANIC"),
            3 => Ok(61000),
            4 => {
                if let Some(owner) = callback.lock().as_ref().and_then(|o| o.upgrade()) {
                    owner.cancel();
                }
                Ok(1000)
            }
            _ => Ok(1000),
        });
        let owner = Arc::new(
            NativeHostApproval::test_record_frozen(
                frozen(&relay, &admission, opened.clone(), clock),
                mode != 7,
            )
            .unwrap(),
        );
        *reenter.lock() = Some(Arc::downgrade(&owner));
        let token = owner.test_native_token();
        if mode == 7 {
            assert!(owner.claim_activation(&admission).is_err());
            assert!(token.is_revoked());
            assert_eq!(opened.each_ref().map(|n| n.load(Ordering::Acquire)), [0, 0]);
            continue;
        }
        if mode == 8 {
            owner.test_expire();
            assert!(owner.claim_activation(&admission).is_err());
            assert!(token.is_revoked());
            assert_eq!(opened.each_ref().map(|n| n.load(Ordering::Acquire)), [0, 0]);
            continue;
        }
        let service = Arc::new(RemoteHostService::default());
        let mut reservation = service.reserve_activation().unwrap();
        let operation = reservation.operation();
        let claim = owner.claim_activation(&admission).unwrap();
        assert!(Arc::ptr_eq(&token, &claim.native_token()));
        assert!(owner.claim_activation(&admission).is_err());
        assert!(
            !token.is_revoked(),
            "duplicate Consumed must not revoke original ticket"
        );
        let end = claim.end();
        if mode == 4 {
            owner.cancel();
        }
        if mode == 5 {
            drop(owner);
        }
        let publisher = reservation.prepare_keys(claim.native_token(), end).unwrap();
        assert!(operation.snapshot().outstanding);
        match mode {
            0 => drop(claim),
            1..=3 => {
                fault.store(mode, Ordering::Release);
                assert!(claim.validate(&admission).is_err());
            }
            4 | 5 => {
                assert!(claim.validate(&admission).is_err());
            }
            6 => {
                fault.store(4, Ordering::Release);
                assert!(claim.validate(&admission).is_err());
            }
            _ => unreachable!(),
        }
        // 必须在任何 retire、publisher/reservation Drop 之前取证，不能被清理遮盖。
        let revoked_before_cleanup = token.is_revoked();
        let opens_before_cleanup = opened.each_ref().map(|n| n.load(Ordering::Acquire));
        drop(publisher);
        drop(reservation);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if operation.retire().retired() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            revoked_before_cleanup,
            "claim failure mode {mode} left original token live"
        );
        assert_eq!(opens_before_cleanup, [0, 0]);
        assert!(!operation.snapshot().unknown);
        eprintln!("TEST_ONLY_CLAIM_FAILURE mode={mode} revoked_before_cleanup={revoked_before_cleanup} original_operation_retired=true opens={opens_before_cleanup:?}");
    }

    // 活 key 使用原批准 cap 自主失效，期间不依赖 UI poll、外部 clock 或退出。
    let opened = counts();
    let cap = Instant::now() + Duration::from_secs(1);
    let bundle = FrozenHostApproval::supplied(
        description(&relay, &admission, Arc::new(|| Ok(1000))),
        &admission,
        cap,
        supplier(&relay, opened.clone()),
    )
    .unwrap();
    let owner = NativeHostApproval::test_record_frozen(bundle, true).unwrap();
    let token = owner.test_native_token();
    let service = Arc::new(RemoteHostService::default());
    let mut reservation = service.reserve_activation().unwrap();
    let operation = reservation.operation();
    let claim = owner.claim_activation(&admission).unwrap();
    assert_eq!(claim.end(), cap, "approval must preserve the original cap");
    let publisher = reservation
        .prepare_keys(claim.native_token(), claim.end())
        .unwrap();
    let ticket = claim.validate(&admission).unwrap();
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
    let worker_admission = fixture
        .supervisor
        .admit_remote_run(
            fixture
                .service
                .access(&fixture.caller, &status.run)
                .unwrap(),
            &status.run,
        )
        .unwrap();
    let weak = Arc::downgrade(&service);
    reservation
        .spawn_prepared(move |signal| {
            ticket.prepare_frozen(runtime, worker_admission, weak, signal, publisher)
        })
        .unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(result) = operation.result() {
                break result;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap());
    tokio::time::timeout(Duration::from_secs(1), async {
        while opened.each_ref().map(|n| n.load(Ordering::Acquire)) != [1, 1] {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!token.is_revoked());
    assert!(operation.snapshot().outstanding);
    tokio::time::timeout(Duration::from_secs(3), async {
        while !token.is_revoked() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!owner.test_watch_revoked());
    assert!(Instant::now() >= cap);
    let expired_before_cleanup = token.is_revoked();
    service.request_exit(0, false);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if operation.retire().retired() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!operation.snapshot().unknown);
    eprintln!("TEST_ONLY_APPROVAL_ORIGINAL_CAP expired_before_cleanup={expired_before_cleanup} watch=false opens=[1,1] retired=true");
    drop(admission);
    assert!(cleanup.close());
    assert!(relay.close().await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn NativeRemote_ApprovedExistingAdapterAnonymousHandshake_018() {
    approved_native_keys_trial(false, false, None, true, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn NativeRemote_OwnedDriverRegistersWithoutObserver_019() {
    approved_native_keys_trial(false, false, None, true, true).await;
}
