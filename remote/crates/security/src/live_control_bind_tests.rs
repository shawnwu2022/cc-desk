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
async fn LiveControl_AuthenticatedFactReachesOriginalChannel_002() {
    for enabled in [false, true] {
        let (c, h, _cancel) = pending().await;
        let (c, h) = if enabled {
            tokio::join!(
                bind_client_live_control_channel(c, trust()),
                bind_host_live_control_channel(h, snapshot())
            )
        } else {
            tokio::join!(
                bind_client_control_lease_channel(c, trust()),
                bind_host_control_lease_channel(h, snapshot())
            )
        };
        let (c, h) = (c.unwrap(), h.unwrap());
        assert_eq!(c.data_keepalive_negotiated(), enabled);
        assert_eq!(h.data_keepalive_negotiated(), enabled);
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
        assert_eq!(c.authority().data_keepalive_negotiated(), enabled);
        assert_eq!(h.authority().data_keepalive_negotiated(), enabled);
        // 协商事实不是 Installed 或控制授权，原 bootstrap 尚未完成。
        assert!(c.installed_fence().is_err());
        assert!(h.installed_fence().is_err());
        c.authority().request_close();
        h.authority().request_close();
        let (c, h) = tokio::join!(c.wait_closed_checked(), h.wait_closed_checked());
        assert!(c.is_ok() && h.is_ok());
    }
}

#[tokio::test]
#[allow(non_snake_case)]
async fn LiveControl_MixedModesRefuseWithoutFallback_003() {
    for client_new in [false, true] {
        let (c, h, _cancel) = pending().await;
        let (c, h) = if client_new {
            tokio::join!(
                bind_client_live_control_channel(c, trust()),
                bind_host_control_lease_channel(h, snapshot())
            )
        } else {
            tokio::join!(
                bind_client_control_lease_channel(c, trust()),
                bind_host_live_control_channel(h, snapshot())
            )
        };
        assert!(c.is_err() && h.is_err());
    }
}

#[tokio::test]
#[allow(non_snake_case)]
async fn LiveControl_OriginalTranscriptRejectsStrippedChangedReordered_004() {
    for client_side in [false, true] {
        for mutation in 0..3 {
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
                                caps.remove(2);
                            }
                            1 => {
                                caps[2] = "data-keepalive-v2".into();
                            }
                            _ => {
                                caps.swap(2, 3);
                            }
                        }
                    }
                    message
                })),
                ..Hooks::default()
            };
            let (c, h) = if client_side {
                tokio::join!(
                    establish_client(c, trust(), hooks, vec![1], BindMode::LiveControl),
                    establish_host(
                        h,
                        snapshot(),
                        Hooks::default(),
                        vec![1],
                        BindMode::LiveControl
                    )
                )
            } else {
                tokio::join!(
                    establish_client(c, trust(), Hooks::default(), vec![1], BindMode::LiveControl),
                    establish_host(h, snapshot(), hooks, vec![1], BindMode::LiveControl)
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

#[tokio::test]
#[allow(non_snake_case)]
async fn LiveControl_ObserverGrantRefusedBothSides_005() {
    for enabled in [true] {
        let (c, h, _cancel) = pending().await;
        let (c, h) = if enabled {
            tokio::join!(
                bind_client_live_control_channel(c, trust()),
                bind_host_live_control_channel(h, snapshot())
            )
        } else {
            tokio::join!(
                bind_client_control_lease_channel(c, trust()),
                bind_host_control_lease_channel(h, snapshot())
            )
        };
        let (c, h) = (c.unwrap(), h.unwrap());
        assert_eq!(c.data_keepalive_negotiated(), enabled);
        assert_eq!(h.data_keepalive_negotiated(), enabled);
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
            role: cc_desk_remote_record::grant::DataRole::Observer,
        };
        let policy = crate::NativeGrantPolicy {
            scope: crate::grant_tests::scope(),
            role: cc_desk_remote_record::grant::DataRole::Observer,
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
        assert!(c.is_err() && h.is_err());
    }
}
