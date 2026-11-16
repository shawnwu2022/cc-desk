use super::*;

async fn pending() -> (
    Pending<tokio::io::DuplexStream>,
    Pending<tokio::io::DuplexStream>,
    CancelHandle,
) {
    let (cio, hio) = tokio::io::duplex(65536);
    let (cancel, t) = cancellation();
    let (c, h) = tokio::join!(
        connect(
            Unbound::new(cio),
            local(true),
            pin(false),
            Duration::from_secs(2),
            t.clone()
        ),
        accept(
            Unbound::new(hio),
            local(false),
            pin(true),
            Duration::from_secs(2),
            t
        )
    );
    (c.unwrap(), h.unwrap(), cancel)
}

#[tokio::test]
#[allow(non_snake_case)]
async fn SnapshotPull_AuthenticatedFactReachesOriginalChannel_002() {
    for enabled in [false, true] {
        let (c, h, _cancel) = pending().await;
        let (c, h) = if enabled {
            tokio::join!(
                bind_client_live_snapshot_pull_channel(c, trust()),
                bind_host_live_snapshot_pull_channel(h, snapshot())
            )
        } else {
            tokio::join!(
                bind_client_live_control_channel(c, trust()),
                bind_host_live_control_channel(h, snapshot())
            )
        };
        let (c, h) = (c.unwrap(), h.unwrap());
        assert_eq!(c.snapshot_pull_negotiated(), enabled);
        assert_eq!(h.snapshot_pull_negotiated(), enabled);
        let mut gate = crate::GrantGate::for_binding(
            h.context(),
            crate::grant_tests::admin(),
            [4; 32],
            Arc::new(|| Ok(1000)),
        )
        .unwrap();
        let mut state = crate::ClientGrantState::new(Arc::new(|| Ok(1000)));
        let mut signer = Signer;
        let grant_trust = crate::ClientGrantTrust {
            issuer: crate::grant_tests::admin(),
            principal: [4; 32],
            scope: crate::grant_tests::scope(),
            role: cc_desk_remote_record::grant::DataRole::Controller,
        };
        let policy = crate::NativeGrantPolicy {
            scope: crate::grant_tests::scope(),
            role: cc_desk_remote_record::grant::DataRole::Controller,
            duration: Duration::from_secs(4),
        };
        let (c, h) = tokio::join!(
            c.grant_client(
                &grant_trust,
                &mut signer,
                &mut state,
                Duration::from_secs(1)
            ),
            h.grant_host(&mut gate, policy, Duration::from_secs(1))
        );
        let (mut c, mut h) = (c.unwrap(), h.unwrap());
        let end = Instant::now() + Duration::from_secs(1);
        let (c, h) = (
            c.start_peer_pending(end).unwrap(),
            h.start_peer_pending(end).unwrap(),
        );
        assert_eq!(c.authority().snapshot_pull_negotiated(), enabled);
        assert_eq!(h.authority().snapshot_pull_negotiated(), enabled);
        // 协商事实不是 Installed 或控制授权，原 bootstrap 尚未完成。
        assert!(c.installed_fence().is_err());
        assert!(h.installed_fence().is_err());
        if enabled {
            let request = h.host_install_request().await.unwrap();
            let (proof, done) =
                tokio::join!(c.client_installed(), h.complete_host_install(request, 7));
            done.unwrap();
            let proof = proof.unwrap();
            assert_eq!(proof.consume_for(&c).unwrap(), 7);
            let fact = h.live_keepalive_fact(Role::Host).unwrap().unwrap();
            assert!(fact.remaining_ms_for(&h, Role::Host).unwrap() > 0);
            assert!(fact.validate_for(&c, Role::Host).is_err());
            assert!(fact.validate_for(&h, Role::Client).is_err());
            let other: Arc<dyn crate::GrantClock> = Arc::new(|| Ok(1000));
            assert!(!fact.uses_clock(&other));
            assert!(
                h.live_keepalive_fact(Role::Host)
                    .unwrap()
                    .unwrap()
                    .test_stale_fence()
                    .validate_for(&h, Role::Host)
                    .is_err()
            );
            h.authority().request_close();
            assert!(fact.validate_for(&h, Role::Host).is_err());
            assert!(h.live_keepalive_fact(Role::Host).is_err());
        }
        c.authority().request_close();
        h.authority().request_close();
        let (c, h) = tokio::join!(c.wait_closed_checked(), h.wait_closed_checked());
        assert!(c.is_ok() && h.is_ok());
    }
}

#[tokio::test]
#[allow(non_snake_case)]
async fn SnapshotPull_MixedModesRefuseWithoutFallback_003() {
    for client_new in [false, true] {
        let (c, h, _cancel) = pending().await;
        let (c, h) = if client_new {
            tokio::join!(
                bind_client_live_snapshot_pull_channel(c, trust()),
                bind_host_live_control_channel(h, snapshot())
            )
        } else {
            tokio::join!(
                bind_client_live_control_channel(c, trust()),
                bind_host_live_snapshot_pull_channel(h, snapshot())
            )
        };
        assert!(c.is_err() && h.is_err());
    }
}

#[tokio::test]
#[allow(non_snake_case)]
async fn SnapshotPull_OriginalTranscriptRejectsStrippedChangedReordered_004() {
    for client_side in [false, true] {
        for mutation in 0..5 {
            let (c, h, _cancel) = pending().await;
            let hooks = Hooks {
                alter: Some(Arc::new(move |mut message| {
                    let caps = match &mut message {
                        Message::HostOffer(v) => Some(&mut v.required_capabilities),
                        Message::ClientOffer(v) => Some(&mut v.required_capabilities),
                        _ => None,
                    };
                    if let Some(caps) = caps {
                        match mutation {
                            0 => {
                                caps.remove(5);
                            }
                            1 => {
                                caps[5] = "snapshot-pull-v2".into();
                            }
                            2 => {
                                caps.swap(4, 5);
                            }
                            3 => {
                                caps.push(caps[5].clone());
                            }
                            _ => {
                                caps.push("unknown".into());
                            }
                        }
                    }
                    message
                })),
                ..Hooks::default()
            };
            let (c, h) = if client_side {
                tokio::join!(
                    establish_client(c, trust(), hooks, vec![1], BindMode::LiveSnapshotPull),
                    establish_host(
                        h,
                        snapshot(),
                        Hooks::default(),
                        vec![1],
                        BindMode::LiveSnapshotPull
                    )
                )
            } else {
                tokio::join!(
                    establish_client(
                        c,
                        trust(),
                        Hooks::default(),
                        vec![1],
                        BindMode::LiveSnapshotPull
                    ),
                    establish_host(h, snapshot(), hooks, vec![1], BindMode::LiveSnapshotPull)
                )
            };
            assert!(c.is_err() && h.is_err());
        }
    }
}

struct Signer;
impl crate::NativeGrantSigner for Signer {
    fn confirm_and_sign<'a>(
        &'a mut self,
        grant: &'a crate::VerifiedCanonicalGrant,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<u8>, crate::GrantError>> + Send + 'a>,
    > {
        Box::pin(async move {
            Ok(crate::grant_tests::signed(
                grant.payload(),
                grant.issuer_kid(),
            ))
        })
    }
}
