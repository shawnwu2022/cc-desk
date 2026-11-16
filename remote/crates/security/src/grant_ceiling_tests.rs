//! 仅使用已有公开测试签名材料，不创建真实凭据。
#![allow(non_snake_case)]
use super::*;
use crate::grant_tests::{admin, context, scope, signed};
use std::sync::atomic::{AtomicU64, Ordering};
fn fixture() -> (GrantGate, Arc<dyn GrantClock>, Arc<AtomicU64>) {
    let now = Arc::new(AtomicU64::new(1000));
    let source = now.clone();
    let clock: Arc<dyn GrantClock> = Arc::new(move || Ok(source.load(Ordering::Acquire)));
    let gate = GrantGate::new(context(), admin(), [4; 32], clock.clone()).unwrap();
    (gate, clock, now)
}
#[test]
fn GrantCeiling_ClockIdentityAndOnlyTighteningBeforeHistory_001() {
    let (mut gate, clock, _) = fixture();
    let foreign: Arc<dyn GrantClock> = Arc::new(|| Ok(1000));
    assert_eq!(gate.limit_expiry(&foreign, 5000), Err(GrantError::Policy));
    gate.limit_expiry(&clock, 5000).unwrap();
    gate.limit_expiry(&clock, 4000).unwrap();
    assert_eq!(gate.limit_expiry(&clock, 5000), Err(GrantError::Policy));
    let grant = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(15))
        .unwrap();
    assert_eq!(grant.expires_host_ms, "4000");
    for _ in 0..3 {
        assert!(gate.accept(&[0; 80]).is_err());
    }
    assert!(gate.pending.is_none());
    assert_eq!(gate.limit_expiry(&clock, 3000), Err(GrantError::Policy));
    gate.revoke();
    assert_eq!(gate.expiry_ceiling, Some(4000));
    assert!(gate.ever_challenged);
    assert!(gate.ever_accepted);
}
#[test]
fn GrantCeiling_LateChallengeNeverRestartsFixedWindow_002() {
    let (mut gate, clock, now) = fixture();
    gate.limit_expiry(&clock, 6500).unwrap();
    now.store(6000, Ordering::Release);
    let grant = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(900))
        .unwrap();
    assert_eq!(grant.created_host_ms, "6000");
    assert_eq!(grant.expires_host_ms, "6500");
    let bytes = signed(&grant, gate.issuer_kid());
    gate.verify_management(
        &bytes,
        &scope(),
        DataRole::Controller,
        Duration::from_secs(900),
        Some(&grant),
    )
    .unwrap();
    let accepted = gate.accept(&bytes).unwrap();
    assert_eq!(accepted.check(), Ok(500));
    now.store(6500, Ordering::Release);
    assert_eq!(gate.accept(&bytes).unwrap_err(), GrantError::Expired);
    assert!(
        gate.challenge(scope(), DataRole::Controller, Duration::from_secs(900))
            .is_err()
    );
}
#[test]
fn GrantCeiling_AllManagementCandidatesAndAcceptFastPathCheckCeiling_003() {
    for mode in 0..3 {
        let (mut gate, clock, _) = fixture();
        gate.limit_expiry(&clock, 5000).unwrap();
        let mut grant = gate
            .challenge(scope(), DataRole::Controller, Duration::from_secs(2))
            .unwrap();
        grant.expires_host_ms = "6000".into();
        let bytes = signed(&grant, gate.issuer_kid());
        // 私有故障注入：生产 setter 不允许既有授权之后更换或扩大上限。
        gate.pending = Some((grant.clone(), 0));
        if mode == 1 {
            gate.accepted = Some(VerifiedGrant(Arc::new(Accepted {
                payload: grant.clone(),
                state: gate.state.clone(),
                closed: watch::channel(false).0,
            })));
        }
        let issued = (mode == 2).then_some(&grant);
        assert_eq!(
            gate.verify_management(
                &bytes,
                &scope(),
                DataRole::Controller,
                Duration::from_secs(10),
                issued
            ),
            Err(GrantError::Policy)
        );
        assert_eq!(gate.accept(&bytes).unwrap_err(), GrantError::Policy);
    }
}
#[test]
fn GrantCeiling_PostSignatureTimeRecheckRejectsExpiry_004() {
    let calls = Arc::new(AtomicU64::new(0));
    let expire_at = Arc::new(AtomicU64::new(u64::MAX));
    let c = calls.clone();
    let e = expire_at.clone();
    let clock: Arc<dyn GrantClock> = Arc::new(move || {
        let n = c.fetch_add(1, Ordering::AcqRel) + 1;
        Ok(if n >= e.load(Ordering::Acquire) {
            5000
        } else {
            1000
        })
    });
    let mut gate = GrantGate::new(context(), admin(), [4; 32], clock.clone()).unwrap();
    gate.limit_expiry(&clock, 5000).unwrap();
    let grant = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(10))
        .unwrap();
    let bytes = signed(&grant, gate.issuer_kid());
    expire_at.store(calls.load(Ordering::Acquire) + 2, Ordering::Release);
    assert_eq!(gate.accept(&bytes).unwrap_err(), GrantError::Expired);
    assert!(gate.accepted.is_none());
}
#[test]
fn GrantCeiling_OriginalBoundsAndUnrestrictedEntryRemain_005() {
    let (mut gate, clock, _) = fixture();
    gate.limit_expiry(&clock, 5000).unwrap();
    assert_eq!(
        gate.challenge(scope(), DataRole::Controller, Duration::from_millis(900001))
            .unwrap_err(),
        GrantError::Bounds
    );
    let (mut old, _, _) = fixture();
    let grant = old
        .challenge(scope(), DataRole::Controller, Duration::from_secs(900))
        .unwrap();
    assert_eq!(grant.expires_host_ms, "901000");
    assert!(old.accept(&signed(&grant, old.issuer_kid())).is_ok());
}
