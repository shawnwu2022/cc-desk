use super::*;

#[test]
#[allow(non_snake_case)]
fn NativeRegistrationStart_StrictCredentialFreeDto_001() {
    let valid = configuration::test_json();
    let frozen = FrozenRegistrationStart::parse(&valid).unwrap();
    assert!(frozen.end.saturating_duration_since(Instant::now()) <= Duration::from_secs(120));
    assert!(frozen.context().is_ok());
    for field in [
        "credentialId",
        "credentialSpki",
        "approved",
        "privateKey",
        "scope",
        "token",
        "clock",
    ] {
        let mut value: serde_json::Value = serde_json::from_str(&valid).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert(field.into(), true.into());
        assert!(
            FrozenRegistrationStart::parse(&value.to_string()).is_err(),
            "{field}"
        );
        let mut value: serde_json::Value = serde_json::from_str(&valid).unwrap();
        value["browser"]
            .as_object_mut()
            .unwrap()
            .insert(field.into(), true.into());
        assert!(
            FrozenRegistrationStart::parse(&value.to_string()).is_err(),
            "browser {field}"
        );
    }
    assert!(FrozenRegistrationStart::parse(
        &valid.replace("\"version\":1", "\"version\":1,\"version\":1")
    )
    .is_err());
    assert!(FrozenRegistrationStart::parse(&" ".repeat(16385)).is_err());
}
#[test]
#[allow(non_snake_case)]
fn NativeRegistrationStart_NoCancelAndExpiryNeverIssuePermit_002() {
    for action in ["none", "no", "cancel", "expired"] {
        let mut frozen = FrozenRegistrationStart::parse(&configuration::test_json()).unwrap();
        if action == "expired" {
            frozen.end = Instant::now();
        }
        let gate = NativeRegistrationKeyApproval::test_pending(frozen);
        match action {
            "no" => gate.test_complete(false),
            "cancel" => {
                gate.test_complete(true);
                gate.cancel()
            }
            "expired" => gate.test_complete(true),
            _ => {}
        }
        assert!(gate.take().is_none(), "{action}");
    }
}
#[test]
#[allow(non_snake_case)]
fn NativeRegistrationStart_YesOneShotOriginalTokenAndDeadline_003() {
    let frozen = FrozenRegistrationStart::parse(&configuration::test_json()).unwrap();
    let end = frozen.end;
    let gate = NativeRegistrationKeyApproval::test_pending(frozen);
    gate.test_complete(true);
    let permit = gate.take().unwrap();
    assert_eq!(permit.signing.end(), end);
    assert!(gate.take().is_none());
    assert!(permit.signing.matches(&permit.signing.token(), end));
    assert!(!permit
        .signing
        .matches(&crate::native_crypto::owner::NativeRevocation::new(), end));
    gate.cancel();
    assert!(!permit.signing.matches(&permit.signing.token(), end));
}

#[test]
#[allow(non_snake_case)]
fn NativeRegistrationStart_ObserverChoiceCannotIssueKeyPermit_015() {
    let mut request: serde_json::Value = serde_json::from_str(&configuration::test_json()).unwrap();
    request["accessMode"] = "observerOnce".into();
    let frozen = FrozenRegistrationStart::parse(&request.to_string()).unwrap();
    assert!(!frozen.remember());
    assert_eq!(
        frozen.data_role(),
        cc_desk_remote_record::grant::DataRole::Observer
    );
    assert!(frozen.render().contains("one-time read-only Observer"));
    let gate = NativeRegistrationKeyApproval::test_pending(frozen);
    gate.test_complete(true);
    assert!(
        gate.take().is_none(),
        "closed Observer role issued key permit"
    );
    assert!(
        gate.take().is_none(),
        "closed Observer permit could be retried"
    );
}

#[test]
#[allow(non_snake_case)]
fn NativeRegistrationStart_ObserverCannotRemember_016() {
    let mut request: serde_json::Value = serde_json::from_str(&configuration::test_json()).unwrap();
    request["accessMode"] = "observerOnce".into();
    request["remember"] = true.into();
    assert!(FrozenRegistrationStart::parse(&request.to_string()).is_err());
    request["accessMode"] = "controller".into();
    let frozen = FrozenRegistrationStart::parse(&request.to_string()).unwrap();
    assert!(frozen.remember());
    assert_eq!(
        frozen.data_role(),
        cc_desk_remote_record::grant::DataRole::Controller
    );
    let gate = NativeRegistrationKeyApproval::test_pending(frozen);
    gate.test_complete(true);
    assert!(
        gate.take().is_some(),
        "explicit Controller changed legacy eligibility"
    );
    request["accessMode"] = "unknown".into();
    assert!(FrozenRegistrationStart::parse(&request.to_string()).is_err());
}
#[test]
#[allow(non_snake_case)]
fn NativeRegistrationStart_IndependentCustodyDoesNotConsumeHostSlot_004() {
    let service = crate::remote_host::RemoteHostService::default();
    let first = ActivationReservation::registration();
    let second = ActivationReservation::registration();
    assert!(service.reserve_activation().is_ok());
    drop(first);
    drop(second);
}

#[test]
#[allow(non_snake_case)]
fn NativeRegistrationStart_CancelReturnedBeforeLateBrowserYes_005() {
    let gate = AttemptCommitGate::default();
    gate.cancel();
    let mut installed = 0;
    assert!(gate
        .commit(|| {
            installed += 1;
            Ok(())
        })
        .is_err());
    assert_eq!(installed, 0);
}
#[test]
#[allow(non_snake_case)]
fn NativeRegistrationStart_CommitAndCancelLinearize_006() {
    let gate = Arc::new(AttemptCommitGate::default());
    let (entered, seen) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let first = gate.clone();
    let worker = std::thread::spawn(move || {
        first.commit(|| {
            entered.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(42)
        })
    });
    seen.recv_timeout(Duration::from_secs(3)).unwrap();
    let (cancelled, returned) = std::sync::mpsc::channel();
    let second = gate.clone();
    let stopper = std::thread::spawn(move || {
        second.cancel();
        cancelled.send(()).unwrap();
    });
    assert!(returned.recv_timeout(Duration::from_millis(20)).is_err());
    release.send(()).unwrap();
    assert_eq!(worker.join().unwrap().unwrap(), 42);
    stopper.join().unwrap();
    assert!(gate.commit(|| Ok(99)).is_err());
}

#[test]
#[allow(non_snake_case)]
fn NativeRegistrationStart_DeliveryFailureUnknownRetainsReceipt_007() {
    let mut reply = RegistrationReply::RetirementUnknown {
        receipt: Some("42".into()),
        remembered: true,
    };
    settle_failure(&mut reply, false, false);
    assert_eq!(
        reply,
        RegistrationReply::RetirementUnknown {
            receipt: Some("42".into()),
            remembered: true
        }
    );
    settle_failure(&mut reply, true, false);
    assert_eq!(
        reply,
        RegistrationReply::RetirementUnknown {
            receipt: Some("42".into()),
            remembered: true
        }
    );
}

// 持有真实 WS 的底层写入，不以 sleep 或概率重跑替代尾帧顺序验证。
struct HeldRegistrationSocket {
    io: tokio::io::DuplexStream,
    entered: Option<tokio::sync::oneshot::Sender<()>>,
    release: Option<tokio::sync::oneshot::Receiver<()>>,
}
impl tokio::io::AsyncRead for HeldRegistrationSocket {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_read(cx, buf)
    }
}
impl tokio::io::AsyncWrite for HeldRegistrationSocket {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        use std::{future::Future, task::Poll};
        if let Some(entered) = self.entered.take() {
            let _ = entered.send(());
        }
        if let Some(release) = self.release.as_mut() {
            match std::pin::Pin::new(release).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(())) => self.release = None,
                Poll::Ready(Err(_)) => {
                    return Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()))
                }
            }
        }
        std::pin::Pin::new(&mut self.io).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_shutdown(cx)
    }
}
async fn held_registration_tail(
    total: Duration,
) -> (
    cc_desk_remote_transport::OwnedTunnel,
    cc_desk_remote_security::CancelHandle,
    tokio::sync::oneshot::Sender<()>,
    tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
) {
    use tokio::io::AsyncWriteExt;
    use tokio_tungstenite::{tungstenite::protocol::Role, WebSocketStream};
    let (io, peer) = tokio::io::duplex(65536);
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let socket = WebSocketStream::from_raw_socket(
        HeldRegistrationSocket {
            io,
            entered: Some(entered),
            release: Some(released),
        },
        Role::Client,
        Some(cc_desk_remote_transport::websocket_config()),
    )
    .await;
    let peer = WebSocketStream::from_raw_socket(
        peer,
        Role::Server,
        Some(cc_desk_remote_transport::websocket_config()),
    )
    .await;
    let (cancel, mut token) = cancellation();
    let (mut stream, tunnel) = cc_desk_remote_transport::test_only_start_registration_stream(
        socket,
        [9; 16],
        Duration::from_secs(3),
        tokio::time::Instant::now() + total,
        async move { token.cancelled().await },
    )
    .unwrap();
    stream
        .write_all(b"TEST_ONLY original final TLS bytes")
        .await
        .unwrap();
    stream.shutdown().await.unwrap();
    drop(stream);
    tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .unwrap()
        .unwrap();
    (tunnel, cancel, release, peer)
}
#[tokio::test]
#[allow(non_snake_case)]
async fn NativeRegistrationStart_HeldTailMustDrainBeforeCompletion_008() {
    use futures_util::StreamExt;
    let (tunnel, cancel, release, mut peer) = held_registration_tail(Duration::from_secs(3)).await;
    let completion = complete_registration_delivery(async { Ok(()) }, &tunnel, || Ok(()));
    tokio::pin!(completion);
    assert!(
        futures_util::poll!(&mut completion).is_pending(),
        "delivered ACK is not proof that the original WebSocket tail send drained"
    );
    assert!(!tunnel.test_only_retirement().known_joined());
    release.send(()).unwrap();
    completion.await.unwrap();
    cancel.cancel();
    tunnel.request_close();
    tunnel.wait_closed().await.unwrap();
    let Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes))) = peer.next().await else {
        panic!("original final bytes lost before retirement")
    };
    let tail = b"TEST_ONLY original final TLS bytes";
    assert_eq!(bytes.len(), 24 + tail.len());
    assert_eq!(&bytes[24..], tail);
}
#[tokio::test]
#[allow(non_snake_case)]
async fn NativeRegistrationStart_FailedDeliveryStillCancelsHeldTail_009() {
    let (tunnel, cancel, _release, _peer) = held_registration_tail(Duration::from_secs(3)).await;
    assert!(complete_registration_delivery(
        async { Err(error("REMOTE_HOST_LOCKED")) },
        &tunnel,
        || Ok(())
    )
    .await
    .is_err());
    cancel.cancel();
    tunnel.request_close();
    tokio::time::timeout(Duration::from_millis(200), tunnel.wait_closed())
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
#[allow(non_snake_case)]
async fn NativeRegistrationStart_HeldTailKeepsOriginalCancelAndEnd_010() {
    for external_cancel in [false, true] {
        let (tunnel, cancel, _release, _peer) =
            held_registration_tail(Duration::from_millis(150)).await;
        let completion = complete_registration_delivery(async { Ok(()) }, &tunnel, || Ok(()));
        tokio::pin!(completion);
        assert!(futures_util::poll!(&mut completion).is_pending());
        if external_cancel {
            cancel.cancel();
        }
        assert!(tokio::time::timeout(Duration::from_millis(500), completion)
            .await
            .unwrap()
            .is_err());
        assert!(!tunnel.registration_tail_drained());
        assert!(tunnel.test_only_retirement().known_joined());
    }
}
#[tokio::test]
#[allow(non_snake_case)]
async fn NativeRegistrationStart_HeldTailRechecksOwnerBeforeSuccess_011() {
    let (tunnel, _cancel, release, _peer) = held_registration_tail(Duration::from_secs(3)).await;
    let valid = AtomicBool::new(true);
    let completion = complete_registration_delivery(async { Ok(()) }, &tunnel, || {
        if valid.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(error("REMOTE_HOST_LOCKED"))
        }
    });
    tokio::pin!(completion);
    assert!(futures_util::poll!(&mut completion).is_pending());
    valid.store(false, Ordering::Release);
    release.send(()).unwrap();
    assert!(
        completion.await.is_err(),
        "joined transport cannot replace the final original-owner check"
    );
    assert!(tunnel.test_only_retirement().known_joined());
}
#[tokio::test]
#[allow(non_snake_case)]
async fn NativeRegistrationStart_HeldTailInterruptedWaitRetainsOwner_012() {
    let (tunnel, cancel, _release, _peer) = held_registration_tail(Duration::from_secs(3)).await;
    {
        let completion = complete_registration_delivery(async { Ok(()) }, &tunnel, || Ok(()));
        tokio::pin!(completion);
        assert!(futures_util::poll!(&mut completion).is_pending());
    }
    assert!(!tunnel.test_only_retirement().known_joined());
    cancel.cancel();
    tunnel.request_close();
    tokio::time::timeout(Duration::from_millis(200), tunnel.wait_closed())
        .await
        .unwrap()
        .unwrap();
    assert!(tunnel.test_only_retirement().known_joined());
}
#[tokio::test]
#[allow(non_snake_case)]
async fn NativeRegistrationStart_HeldTailBrokenPipeKeepsReceiptUnconfirmed_013() {
    let (tunnel, _cancel, release, _peer) = held_registration_tail(Duration::from_secs(3)).await;
    let completion = complete_registration_delivery(async { Ok(()) }, &tunnel, || Ok(()));
    tokio::pin!(completion);
    assert!(futures_util::poll!(&mut completion).is_pending());
    drop(release);
    assert!(completion.await.is_err());
    assert!(tunnel.test_only_retirement().known_joined());
    assert!(!tunnel.registration_tail_drained());
    let original = RegistrationReply::DeliveryUnconfirmed {
        receipt: "42".into(),
        remembered: true,
    };
    let mut reply = original.clone();
    settle_failure(&mut reply, false, true);
    assert_eq!(reply, original);
}

#[test]
#[allow(non_snake_case)]
fn NativeRegistrationStart_ConfirmationPublicationCancelBarrier_014() {
    use std::sync::mpsc;
    let gate = Arc::new(AttemptCommitGate::default());
    let reply = Arc::new(Mutex::new(RegistrationReply::DeliveryUnconfirmed {
        receipt: "TEST_ONLY".into(),
        remembered: true,
    }));
    let held = reply.lock();
    let (entered, seen) = mpsc::channel();
    let worker_gate = gate.clone();
    let worker_reply = reply.clone();
    let worker = std::thread::spawn(move || {
        publish_registration(
            &worker_gate,
            &worker_reply,
            RegistrationReply::InstalledLocked {
                receipt: "TEST_ONLY".into(),
                remembered: true,
            },
            || {
                entered.send(()).unwrap();
                Ok(())
            },
            || Ok(()),
        )
    });
    seen.recv_timeout(Duration::from_secs(5)).unwrap();
    let (started, start_seen) = mpsc::channel();
    let (cancelled, cancel_seen) = mpsc::channel();
    let cancelling = gate.clone();
    let cancel = std::thread::spawn(move || {
        started.send(()).unwrap();
        cancelling.cancel();
        cancelled.send(()).unwrap();
    });
    start_seen.recv_timeout(Duration::from_secs(5)).unwrap();
    let cancellation_waited_for_publication = cancel_seen
        .recv_timeout(Duration::from_millis(200))
        .is_err();
    drop(held);
    assert!(worker.join().unwrap().is_ok());
    cancel.join().unwrap();
    assert!(
        cancellation_waited_for_publication,
        "cancel overtook a pending success publication"
    );
    assert!(gate.stopped.load(Ordering::Acquire));

    let before = reply.lock().clone();
    assert!(publish_registration(
        &gate,
        &reply,
        RegistrationReply::Locked,
        || panic!("cancelled owner must not confirm"),
        || Ok(())
    )
    .is_err());
    assert_eq!(*reply.lock(), before);

    let gate = AttemptCommitGate::default();
    let reply = Mutex::new(RegistrationReply::Locked);
    assert!(publish_registration(
        &gate,
        &reply,
        RegistrationReply::InstalledLocked {
            receipt: "TEST_ONLY".into(),
            remembered: false,
        },
        || Ok(()),
        || Err(error("REMOTE_HOST_LOCKED"))
    )
    .is_err());
    assert_eq!(*reply.lock(), RegistrationReply::Locked);
}
