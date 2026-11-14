#![allow(non_snake_case)]
use super::*;
use crate::{P256Spki, Role, cancellation};
fn local(client: bool) -> LocalIdentity {
    let (k, p) = crate::test_identities::identity(client, false);
    LocalIdentity::new(
        if client { Role::Client } else { Role::Host },
        P256Spki::new(&p).unwrap(),
        k.key.clone(),
    )
    .unwrap()
}
fn pin(client: bool) -> PeerPin {
    let (_, p) = crate::test_identities::identity(client, false);
    PeerPin::new(
        if client { Role::Client } else { Role::Host },
        P256Spki::new(&p).unwrap(),
    )
}
#[tokio::test]
async fn RegistrationChannel_MutualPinnedTls_001() {
    let (c, h) = tokio::io::duplex(65536);
    let (_cancel, t) = cancellation();
    let (client, host) = tokio::join!(
        registration_connect(
            Unbound::new(c),
            local(true),
            pin(false),
            Duration::from_secs(2),
            Duration::from_secs(30),
            t.clone()
        ),
        registration_accept(
            Unbound::new(h),
            local(false),
            pin(true),
            Duration::from_secs(2),
            Duration::from_secs(30),
            t
        )
    );
    assert!(
        client.is_ok(),
        "the independently pinned registration client must authenticate"
    );
    assert!(
        host.is_ok(),
        "the independently pinned registration host must authenticate"
    );
    for e in [
        client.as_ref().unwrap().evidence(),
        host.as_ref().unwrap().evidence(),
    ] {
        assert!(e.full_handshake && e.raw_public_key && e.alpn_matches);
        assert_eq!(e.protocol, rustls::ProtocolVersion::TLSv1_3);
    }
}

async fn channels() -> (
    RegistrationChannel<tokio::io::DuplexStream>,
    RegistrationChannel<tokio::io::DuplexStream>,
    crate::CancelHandle,
) {
    let (c, h) = tokio::io::duplex(65536);
    let (cancel, t) = cancellation();
    let (c, h) = tokio::join!(
        registration_connect(
            Unbound::new(c),
            local(true),
            pin(false),
            Duration::from_secs(2),
            Duration::from_secs(30),
            t.clone()
        ),
        registration_accept(
            Unbound::new(h),
            local(false),
            pin(true),
            Duration::from_secs(2),
            Duration::from_secs(30),
            t
        )
    );
    (
        c.unwrap().into_channel().unwrap(),
        h.unwrap().into_channel().unwrap(),
        cancel,
    )
}
#[tokio::test]
async fn RegistrationChannel_OrderedExchange_002() {
    let (mut c, mut h, _cancel) = channels().await;
    let client = async {
        assert_eq!(c.client_create().await.unwrap(), b"create");
        assert_eq!(c.client_created(b"created").await.unwrap(), b"confirm");
        assert_eq!(
            c.client_asserted(b"asserted").await.unwrap(),
            RegistrationOutcome::Rejected
        );
        c.client_ack().await.unwrap();
    };
    let host = async {
        assert_eq!(h.host_create(b"create").await.unwrap(), b"created");
        assert_eq!(h.host_confirm(b"confirm").await.unwrap(), b"asserted");
        h.host_finish(RegistrationOutcome::Rejected).await.unwrap();
    };
    tokio::join!(client, host);
}

#[tokio::test]
async fn RegistrationChannel_NoNormalAlpnDowngrade_003() {
    for client_registration in [true, false] {
        let (c, h) = tokio::io::duplex(65536);
        let (_cancel, t) = cancellation();
        if client_registration {
            let (c, h) = tokio::join!(
                registration_connect(
                    Unbound::new(c),
                    local(true),
                    pin(false),
                    Duration::from_secs(1),
                    Duration::from_secs(2),
                    t.clone()
                ),
                crate::accept(
                    Unbound::new(h),
                    local(false),
                    pin(true),
                    Duration::from_secs(1),
                    t
                )
            );
            assert!(c.is_err() && h.is_err());
        } else {
            let (c, h) = tokio::join!(
                crate::connect(
                    Unbound::new(c),
                    local(true),
                    pin(false),
                    Duration::from_secs(1),
                    t.clone()
                ),
                registration_accept(
                    Unbound::new(h),
                    local(false),
                    pin(true),
                    Duration::from_secs(1),
                    Duration::from_secs(2),
                    t
                )
            );
            assert!(c.is_err() && h.is_err());
        }
    }
}
#[tokio::test]
async fn RegistrationChannel_WrongIndependentPin_004() {
    let (c, h) = tokio::io::duplex(65536);
    let (_cancel, t) = cancellation();
    let (_, wrong) = crate::test_identities::identity(true, false);
    let (c, h) = tokio::join!(
        registration_connect(
            Unbound::new(c),
            local(true),
            PeerPin::new(Role::Host, P256Spki::new(&wrong).unwrap()),
            Duration::from_secs(1),
            Duration::from_secs(2),
            t.clone()
        ),
        registration_accept(
            Unbound::new(h),
            local(false),
            pin(true),
            Duration::from_secs(1),
            Duration::from_secs(2),
            t
        )
    );
    assert!(c.is_err() && h.is_err());
}
#[tokio::test(start_paused = true)]
async fn RegistrationChannel_TotalLifetimeNeverRenews_005() {
    let (mut c, mut h, _cancel) = channels().await;
    let original = c.deadline();
    tokio::time::advance(Duration::from_secs(29)).await;
    let (c1, h1) = tokio::join!(c.client_create(), h.host_create(b"create"));
    // h awaits creation, so use a separate full exchange below instead of a fresh timer.
    assert!(c1.is_ok());
    assert_eq!(h1, Err(RegistrationError::Deadline));
    assert_eq!(c.deadline(), original);
    assert_eq!(
        c.client_created(b"late").await,
        Err(RegistrationError::Deadline)
    );
}
#[tokio::test]
async fn RegistrationChannel_WrongPhaseAndDroppedFutureCloseIo_006() {
    let (mut c, mut h, _cancel) = channels().await;
    assert_eq!(
        c.client_created(b"early").await,
        Err(RegistrationError::Order)
    );
    assert_eq!(c.client_create().await, Err(RegistrationError::Closed));
    assert!(h.host_create(b"request").await.is_err());
    let (mut c, mut h, _cancel) = channels().await;
    {
        let future = c.client_create();
        tokio::pin!(future);
        tokio::select! {_=&mut future=>panic!("unexpected payload"),_=tokio::time::sleep(Duration::from_millis(1))=>{}}
    }
    assert_eq!(c.client_create().await, Err(RegistrationError::Closed));
    assert!(h.host_create(b"request").await.is_err());
}
#[tokio::test]
async fn RegistrationChannel_CancelPendingRead_007() {
    let (mut c, mut h, cancel) = channels().await;
    let reading = c.client_create();
    tokio::pin!(reading);
    let stop = async {
        tokio::task::yield_now().await;
        cancel.cancel();
    };
    let (result, _) = tokio::join!(&mut reading, stop);
    assert_eq!(result, Err(RegistrationError::Cancelled));
    assert_eq!(
        h.host_create(b"request").await,
        Err(RegistrationError::Cancelled)
    );
}
#[tokio::test]
async fn RegistrationChannel_FrameBoundsOrderAndTruncation_008() {
    use tokio::io::AsyncWriteExt;
    for (kind, length, body, expected) in [
        (2, 1, vec![0], RegistrationError::Order),
        (1, 16385, vec![], RegistrationError::Bounds),
        (1, 2, vec![0], RegistrationError::Io),
    ] {
        let (mut c, mut h, _cancel) = channels().await;
        let mut w = h.wire.take().unwrap();
        let send = async {
            let mut b = b"CDR1".to_vec();
            b.push(kind);
            b.extend((length as u32).to_be_bytes());
            b.extend(body);
            w.io.write_all(&b).await.unwrap();
            w.io.shutdown().await.unwrap();
        };
        let (result, _) = tokio::join!(c.client_create(), send);
        assert_eq!(result, Err(expected));
        assert_eq!(c.client_create().await, Err(RegistrationError::Closed));
    }
}
async fn asserted_pair(
    c: &mut RegistrationChannel<tokio::io::DuplexStream>,
    h: &mut RegistrationChannel<tokio::io::DuplexStream>,
) {
    let client = async {
        assert_eq!(c.client_create().await.unwrap(), b"create");
        assert_eq!(c.client_created(b"created").await.unwrap(), b"confirm");
    };
    let host = async {
        assert_eq!(h.host_create(b"create").await.unwrap(), b"created");
        let mut w = h.take(Stage::HostConfirm).unwrap();
        w.write(3, b"confirm").await.unwrap();
        h.keep(w, Stage::HostConfirm).unwrap();
    };
    tokio::join!(client, host);
}
#[tokio::test]
async fn RegistrationChannel_RejectsForgedDecisionTranscript_009() {
    let (mut c, mut h, _cancel) = channels().await;
    asserted_pair(&mut c, &mut h).await;
    let server = async {
        let mut w = h.take(Stage::HostConfirm).unwrap();
        assert_eq!(w.read(4).await.unwrap(), b"asserted");
        let d = Decision {
            version: 1,
            transcript: [0; 32],
            outcome: RegistrationOutcome::Rejected,
        };
        w.write(5, &serde_json::to_vec(&d).unwrap()).await.unwrap();
    };
    let (result, _) = tokio::join!(c.client_asserted(b"asserted"), server);
    assert_eq!(result, Err(RegistrationError::Transcript));
}
#[tokio::test]
async fn RegistrationChannel_AckMismatchIsNotSuccess_010() {
    let (mut c, mut h, _cancel) = channels().await;
    asserted_pair(&mut c, &mut h).await;
    let client = async {
        assert_eq!(
            c.client_asserted(b"asserted").await.unwrap(),
            RegistrationOutcome::Rejected
        );
        let mut w = c.take(Stage::ClientAck).unwrap();
        w.write(6, &[0; 32]).await.unwrap();
    };
    let server = async {
        let mut w = h.take(Stage::HostConfirm).unwrap();
        w.read(4).await.unwrap();
        h.keep(w, Stage::HostFinish).unwrap();
        h.host_finish(RegistrationOutcome::Rejected).await
    };
    let (_, result) = tokio::join!(client, server);
    assert_eq!(result, Err(RegistrationError::Transcript));
    assert_eq!(
        h.host_finish(RegistrationOutcome::Rejected).await,
        Err(RegistrationError::Closed)
    );
}
#[tokio::test]
async fn RegistrationChannel_InstalledDecisionLossStaysUncertain_011() {
    let (mut c, mut h, _cancel) = channels().await;
    asserted_pair(&mut c, &mut h).await;
    let installed = RegistrationOutcome::RunLocalCandidateInstalled {
        receipt: "1".into(),
        scope: cc_desk_remote_record::grant::SessionScope {
            session: "03".repeat(16),
            run: "TEST_ONLY".into(),
            generation: "1".into(),
        },
    };
    let client = async {
        let observed = c.client_asserted(b"asserted").await.unwrap();
        assert_eq!(observed, installed);
        c.close();
    };
    let server = async {
        let mut w = h.take(Stage::HostConfirm).unwrap();
        w.read(4).await.unwrap();
        h.keep(w, Stage::HostFinish).unwrap();
        h.host_finish(installed.clone()).await
    };
    let (_, result) = tokio::join!(client, server);
    assert!(result.is_err());
    assert_eq!(
        h.host_finish(installed).await,
        Err(RegistrationError::Closed)
    );
}
#[tokio::test]
async fn RegistrationChannel_ExporterSeparatesIdenticalAttempts_012() {
    let (c, h, _a) = channels().await;
    let (c2, h2, _b) = channels().await;
    assert_eq!(
        c.wire.as_ref().unwrap().hash(),
        h.wire.as_ref().unwrap().hash()
    );
    assert_eq!(
        c2.wire.as_ref().unwrap().hash(),
        h2.wire.as_ref().unwrap().hash()
    );
    assert_ne!(
        c.wire.as_ref().unwrap().hash(),
        c2.wire.as_ref().unwrap().hash()
    );
}
#[tokio::test]
async fn RegistrationChannel_SetupAndTotalLimits_013() {
    for (setup, total) in [
        (Duration::ZERO, Duration::from_secs(30)),
        (Duration::from_secs(11), Duration::from_secs(30)),
        (Duration::from_secs(1), Duration::ZERO),
        (Duration::from_secs(1), Duration::from_secs(121)),
    ] {
        let (io, _peer) = tokio::io::duplex(1024);
        let (_cancel, t) = cancellation();
        assert!(matches!(
            registration_connect(Unbound::new(io), local(true), pin(false), setup, total, t).await,
            Err(SecurityError::Limit)
        ));
    }
}
#[tokio::test]
async fn RegistrationChannel_NativeDecisionChecksOriginalLifetime_014() {
    let (mut c, mut h, cancel) = channels().await;
    assert!(h.check_host_start().is_ok());
    assert_eq!(h.check_host_decision(), Err(RegistrationError::Order));
    assert_eq!(c.check_host_start(), Err(RegistrationError::Order));
    asserted_pair(&mut c, &mut h).await;
    let client = async {
        assert!(c.client_asserted(b"asserted").await.is_err());
    };
    let host = async {
        let mut w = h.take(Stage::HostConfirm).unwrap();
        w.read(4).await.unwrap();
        h.keep(w, Stage::HostFinish).unwrap();
        assert_eq!(h.check_host_start(), Err(RegistrationError::Order));
        assert!(h.check_host_decision().is_ok());
        cancel.cancel();
        assert_eq!(h.check_host_decision(), Err(RegistrationError::Cancelled));
        h.close();
    };
    tokio::join!(client, host);
}

#[tokio::test(start_paused = true)]
async fn RegistrationChannel_OriginalAbsoluteEndSurvivesDeferredPolling_015() {
    let (c, h) = tokio::io::duplex(65536);
    let (_cancel, token) = cancellation();
    let original = tokio::time::Instant::now();
    let end = original + Duration::from_secs(30);
    let host = registration_accept_until(
        Unbound::new(h),
        local(false),
        pin(true),
        original + Duration::from_secs(10),
        end,
        token.clone(),
    );
    tokio::time::advance(Duration::from_secs(5)).await;
    let (client, host) = tokio::join!(
        registration_connect(
            Unbound::new(c),
            local(true),
            pin(false),
            Duration::from_secs(2),
            Duration::from_secs(30),
            token
        ),
        host,
    );
    let mut client = client.unwrap().into_channel().unwrap();
    let mut host = host.unwrap().into_channel().unwrap();
    assert_eq!(host.deadline(), end);
    asserted_pair(&mut client, &mut host).await;
    let client = async {
        assert!(client.client_asserted(b"asserted").await.is_err());
    };
    let server = async {
        let mut wire = host.take(Stage::HostConfirm).unwrap();
        wire.read(4).await.unwrap();
        host.keep(wire, Stage::HostFinish).unwrap();
        assert!(host.check_host_decision().is_ok());
        tokio::time::advance(Duration::from_secs(25)).await;
        assert_eq!(tokio::time::Instant::now(), end);
        assert_eq!(host.check_host_decision(), Err(RegistrationError::Deadline));
        host.close();
    };
    tokio::join!(client, server);
}

#[tokio::test(start_paused = true)]
async fn RegistrationChannel_AbsoluteSetupDoesNotRestartAfterAdmission_016() {
    use tokio::io::AsyncReadExt;
    let (io, mut peer) = tokio::io::duplex(1024);
    let (_cancel, token) = cancellation();
    let original = tokio::time::Instant::now();
    let pending = registration_accept_until(
        Unbound::new(io),
        local(false),
        pin(true),
        original + Duration::from_secs(10),
        original + Duration::from_secs(40),
        token,
    );
    tokio::time::advance(Duration::from_secs(9)).await;
    assert!(matches!(pending.await, Err(SecurityError::Deadline)));
    assert_eq!(
        tokio::time::Instant::now(),
        original + Duration::from_secs(10)
    );
    assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
}

#[tokio::test(start_paused = true)]
async fn RegistrationChannel_AbsoluteLimitsFailBeforeIo_017() {
    use tokio::io::AsyncReadExt;
    let now = tokio::time::Instant::now();
    for (setup, total, expected) in [
        (now, now + Duration::from_secs(30), SecurityError::Deadline),
        (now + Duration::from_secs(1), now, SecurityError::Deadline),
        (
            now + Duration::from_secs(11),
            now + Duration::from_secs(30),
            SecurityError::Limit,
        ),
        (
            now + Duration::from_secs(1),
            now + Duration::from_secs(121),
            SecurityError::Limit,
        ),
    ] {
        let (io, mut peer) = tokio::io::duplex(1024);
        let (_cancel, token) = cancellation();
        let result = registration_accept_until(
            Unbound::new(io),
            local(false),
            pin(true),
            setup,
            total,
            token,
        )
        .await;
        assert!(matches!(result, Err(error) if error == expected));
        assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
    }
}
