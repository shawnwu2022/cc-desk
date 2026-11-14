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
