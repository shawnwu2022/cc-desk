#![allow(non_snake_case)]
use super::*;
use serde_json::json;
fn thread(id: uuid::Uuid, parent: Option<uuid::Uuid>) -> Value {
    json!({"id":id.to_string(),"parentThreadId":parent.map(|id| id.to_string())})
}
#[test]
fn NativeArchiveRelations_AncestorDescendantAndSibling_021() {
    let root = uuid::Uuid::new_v4();
    let child = uuid::Uuid::new_v4();
    let sibling = uuid::Uuid::new_v4();
    let records = HashMap::from([
        (root, thread(root, None)),
        (child, thread(child, Some(root))),
        (sibling, thread(sibling, Some(root))),
    ]);
    for (target, owner, busy) in [
        (root, child, true),
        (child, root, true),
        (child, sibling, false),
        (root, root, true),
    ] {
        let result = check(
            target,
            &records[&target],
            &BTreeSet::from([owner]),
            &mut |id| records.get(&id).cloned().ok_or_else(unavailable),
        );
        if busy {
            assert_eq!(result.unwrap_err().code, "ARCHIVE_BUSY");
        } else {
            result.unwrap();
        }
    }
}
#[test]
fn NativeArchiveRelations_MissingFieldsWrongIdentityAndReadFailure_022() {
    let target = uuid::Uuid::new_v4();
    let owner = uuid::Uuid::new_v4();
    for value in [
        json!({"id":owner.to_string()}),
        json!({"id":target.to_string(),"parentThreadId":null}),
        json!({"id":owner.to_string(),"parentThreadId":"not-a-uuid"}),
    ] {
        assert_eq!(
            check(
                target,
                &thread(target, None),
                &BTreeSet::from([owner]),
                &mut |_| Ok(value.clone())
            )
            .unwrap_err()
            .code,
            "ARCHIVE_RELATION_UNAVAILABLE"
        );
    }
    assert!(check(
        target,
        &thread(target, None),
        &BTreeSet::from([owner]),
        &mut |_| Err(unavailable())
    )
    .is_err());
}
#[test]
fn NativeArchiveRelations_CycleAndDepthBudgetReject_023() {
    let target = uuid::Uuid::new_v4();
    let owner = uuid::Uuid::new_v4();
    assert!(check(
        target,
        &thread(target, None),
        &BTreeSet::from([owner]),
        &mut |id| Ok(thread(id, Some(id)))
    )
    .is_err());
    let mut count = 0;
    assert!(check(
        target,
        &thread(target, None),
        &BTreeSet::from([owner]),
        &mut |id| {
            count += 1;
            Ok(thread(id, Some(uuid::Uuid::new_v4())))
        }
    )
    .is_err());
    assert!(count <= MAX_DEPTH);
}
