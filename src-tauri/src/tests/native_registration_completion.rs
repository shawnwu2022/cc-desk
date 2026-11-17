//! TEST_ONLY：原存储事务与封闭确认的故障注入，不创建真实凭据或当前授权。
use super::*;
use crate::remote_native_pairing_store::{
    tests::{record, reopen_synthetic_file_store, synthetic_file_store, TestStore},
    MutationAuthorization, RecordState,
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Allow;
impl MutationAuthorization for Allow {
    fn check(&mut self, _: &StoreTarget, _: &Mutation) -> Result<(), StoreError> {
        Ok(())
    }
}
fn root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("enrollment-completion-")
        .tempdir_in(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join(".audit"),
        )
        .unwrap()
}
fn initial(store: &TestStore) -> (Mutation, LockedSnapshot) {
    let mutation = Mutation::Initialize {
        store_id: uuid::Uuid::new_v4().to_string(),
        record: record(),
    };
    let saved = store.apply(mutation.clone(), &mut Allow).unwrap();
    (mutation, saved)
}
fn pending(store: &TestStore, mutation: &Mutation, saved: &LockedSnapshot) -> PendingInstallation {
    PendingInstallation::persisted(store.target(), mutation, saved).unwrap()
}
#[test]
#[allow(non_snake_case)]
fn NativeEnrollment_ExactTargetDigestAndCas_001() {
    let dir = root();
    let store = synthetic_file_store(dir.path()).unwrap();
    let (mutation, saved) = initial(&store);
    let other_dir = root();
    let other = synthetic_file_store(other_dir.path()).unwrap();
    initial(&other);
    let check = || Ok(());
    let proof = pending(&store, &mutation, &saved)
        .complete(DeliveryComplete(()), &check)
        .unwrap();
    assert_eq!(
        other.confirm_installation(proof),
        Err(StoreError::Unauthorized)
    );

    let mut wrong = saved.clone();
    wrong.devices[0].record.as_mut().unwrap().epochs.policy += 1;
    assert!(PendingInstallation::persisted(store.target(), &mutation, &wrong).is_err());
    wrong.devices[0].device_id = uuid::Uuid::new_v4().to_string();
    assert!(PendingInstallation::persisted(store.target(), &mutation, &wrong).is_err());

    let stale = pending(&store, &mutation, &saved);
    let confirmed = store
        .confirm_installation(
            pending(&store, &mutation, &saved)
                .complete(DeliveryComplete(()), &check)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(confirmed.expected.revision, 2);
    let candidate = store
        .select_installed(&confirmed.expected, &record().device_id)
        .unwrap();
    store.revalidate_installed(&candidate).unwrap();
    assert_eq!(candidate.record(), &record());
    assert_eq!(candidate.expected(), &confirmed.expected);
    assert!(matches!(
        other.revalidate_installed(&candidate),
        Err(
            crate::remote_native_pairing_store::InstalledCandidateError::Store(
                StoreError::Unauthorized
            )
        )
    ));
    assert!(matches!(
        store.select_installed(&confirmed.expected, &uuid::Uuid::new_v4().to_string()),
        Err(crate::remote_native_pairing_store::InstalledCandidateError::Missing)
    ));
    assert_eq!(
        serde_json::to_value(&confirmed.devices[0]).unwrap()["enrollment"]["state"],
        "confirmed"
    );
    assert_eq!(
        store.confirm_installation(stale.complete(DeliveryComplete(()), &check).unwrap()),
        Err(StoreError::Conflict)
    );

    let mut second = record();
    second.device_id = uuid::Uuid::new_v4().to_string();
    let next = store
        .apply(
            Mutation::Remember {
                expected: confirmed.expected,
                record: second.clone(),
            },
            &mut Allow,
        )
        .unwrap();
    let old = next
        .devices
        .iter()
        .find(|e| e.device_id == record().device_id)
        .unwrap();
    assert_eq!(
        serde_json::to_value(old).unwrap()["enrollment"]["revision"],
        2
    );
    assert_eq!(next.expected.revision, 3);
    assert!(matches!(
        store.revalidate_installed(&candidate),
        Err(
            crate::remote_native_pairing_store::InstalledCandidateError::Store(
                StoreError::Conflict
            )
        )
    ));
    drop(store);
    let store = reopen_synthetic_file_store(dir.path()).unwrap();
    assert_eq!(store.load().unwrap(), next);
    assert!(matches!(
        store.revalidate_installed(&candidate),
        Err(
            crate::remote_native_pairing_store::InstalledCandidateError::Store(
                StoreError::Unauthorized
            )
        )
    ));
    let current = store
        .select_installed(&next.expected, &record().device_id)
        .unwrap();
    store.revalidate_installed(&current).unwrap();

    let m = Mutation::Remember {
        expected: saved.expected,
        record: second,
    };
    assert!(store.apply(m, &mut Allow).is_err());
    let revoked = store
        .apply(
            Mutation::Forget {
                expected: next.expected,
                device_id: record().device_id,
            },
            &mut Allow,
        )
        .unwrap();
    let tombstone = revoked
        .devices
        .iter()
        .find(|e| e.device_id == record().device_id)
        .unwrap();
    assert_eq!(tombstone.state, RecordState::Revoked);
    assert!(matches!(
        store.revalidate_installed(&current),
        Err(
            crate::remote_native_pairing_store::InstalledCandidateError::Store(
                StoreError::Conflict
            )
        )
    ));
    assert!(matches!(
        store.select_installed(&revoked.expected, &record().device_id),
        Err(crate::remote_native_pairing_store::InstalledCandidateError::Revoked)
    ));
    assert!(serde_json::to_value(tombstone)
        .unwrap()
        .get("enrollment")
        .is_none());
    drop(store);
    let reopened = reopen_synthetic_file_store(dir.path()).unwrap();
    assert_eq!(reopened.load().unwrap(), revoked);
}
#[test]
#[allow(non_snake_case)]
fn NativeEnrollment_ForgetOrUnrelatedWriteFencesPending_002() {
    for forget in [true, false] {
        let dir = root();
        let store = synthetic_file_store(dir.path()).unwrap();
        let (mutation, saved) = initial(&store);
        let pending = pending(&store, &mutation, &saved);
        let change = if forget {
            Mutation::Forget {
                expected: saved.expected,
                device_id: record().device_id,
            }
        } else {
            let mut second = record();
            second.device_id = uuid::Uuid::new_v4().to_string();
            Mutation::Remember {
                expected: saved.expected,
                record: second,
            }
        };
        let changed = store.apply(change, &mut Allow).unwrap();
        let check = || Ok(());
        assert_eq!(
            store.confirm_installation(pending.complete(DeliveryComplete(()), &check).unwrap()),
            Err(StoreError::Conflict)
        );
        assert_eq!(store.load().unwrap(), changed);
    }
}
#[test]
#[allow(non_snake_case)]
fn NativeEnrollment_CancelAndPublicationFailureStayLocked_003() {
    for fail_at in [1, 2, 3, 4] {
        let dir = root();
        let store = synthetic_file_store(dir.path()).unwrap();
        let (mutation, saved) = initial(&store);
        let calls = AtomicUsize::new(0);
        let check = || {
            if calls.fetch_add(1, Ordering::SeqCst) + 1 == fail_at {
                Err(super::super::error("REMOTE_HOST_LOCKED"))
            } else {
                Ok(())
            }
        };
        match pending(&store, &mutation, &saved).complete(DeliveryComplete(()), &check) {
            Ok(proof) => assert_eq!(
                store.confirm_installation(proof),
                Err(StoreError::Unauthorized)
            ),
            Err(_) => assert_eq!(fail_at, 1),
        }
        assert_eq!(store.load().unwrap(), saved);
        drop(store);
        let reopened = reopen_synthetic_file_store(dir.path()).unwrap();
        assert_eq!(reopened.load().unwrap(), saved);
    }

    let dir = root();
    let store = synthetic_file_store(dir.path()).unwrap();
    let (mutation, saved) = initial(&store);
    crate::remote_native_pairing_store::tests::fail_after_publish(&store);
    let check = || Ok(());
    let proof = pending(&store, &mutation, &saved)
        .complete(DeliveryComplete(()), &check)
        .unwrap();
    assert_eq!(store.confirm_installation(proof), Err(StoreError::Unknown));
    assert_eq!(store.load(), Err(StoreError::Poisoned));
    drop(store);
    // 重开仅恢复历史快照，不恢复当前许可，也不重放不确定写入。
    let reopened = reopen_synthetic_file_store(dir.path()).unwrap();
    let loaded = reopened.load().unwrap();
    assert_eq!(loaded.expected.revision, 2);
    assert_eq!(
        serde_json::to_value(&loaded.devices[0]).unwrap()["enrollment"]["state"],
        "confirmed"
    );
}

#[test]
#[allow(non_snake_case)]
fn NativeEnrollment_CandidateRejectsUncertainInstance_004() {
    use crate::remote_native_pairing_store::InstalledCandidateError as Blocked;
    let dir = root();
    let store = synthetic_file_store(dir.path()).unwrap();
    let (mutation, saved) = initial(&store);
    let check = || Ok(());
    let confirmed = store
        .confirm_installation(
            pending(&store, &mutation, &saved)
                .complete(DeliveryComplete(()), &check)
                .unwrap(),
        )
        .unwrap();
    let candidate = store
        .select_installed(&confirmed.expected, &record().device_id)
        .unwrap();
    store.revalidate_installed(&candidate).unwrap();
    crate::remote_native_pairing_store::tests::fail_after_publish(&store);
    assert_eq!(
        store.apply(
            Mutation::Forget {
                expected: confirmed.expected,
                device_id: record().device_id,
            },
            &mut Allow
        ),
        Err(StoreError::Unknown)
    );
    assert!(matches!(
        store.revalidate_installed(&candidate),
        Err(Blocked::Store(StoreError::Poisoned))
    ));
    drop(store);
    let reopened = reopen_synthetic_file_store(dir.path()).unwrap();
    let state = reopened.load().unwrap();
    assert!(matches!(
        reopened.revalidate_installed(&candidate),
        Err(Blocked::Store(StoreError::Unauthorized))
    ));
    assert!(matches!(
        reopened.select_installed(&state.expected, &record().device_id),
        Err(Blocked::Revoked)
    ));
}
