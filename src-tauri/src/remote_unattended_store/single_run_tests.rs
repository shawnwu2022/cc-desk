//! A6b 子门禁；后续必须由真实最终批准与原 run 组成闭环。
use super::*;

fn schema_bytes(store: &TestStore, schema: u64, mode: Option<&str>) -> Vec<u8> {
    let mut value: serde_json::Value =
        serde_json::from_slice(&FakeSealer.unseal(&raw(store)).unwrap()).unwrap();
    let document = value["document"].as_object_mut().unwrap();
    document.insert("schema".into(), schema.into());
    if let Some(mode) = mode {
        document.insert("mode".into(), mode.into());
    } else {
        document.remove("mode");
    }
    // 可识别文档按原类型 canonical 计算；未知字段仍必须在 decoder 拒绝。
    let bytes = match serde_json::from_value::<Document>(value["document"].clone()) {
        Ok(document) => serde_json::to_vec(&document).unwrap(),
        Err(_) => serde_json::to_vec(&value["document"]).unwrap(),
    };
    value["digest"] = serde_json::to_value(<[u8; 32]>::from(Sha256::digest(&bytes))).unwrap();
    FakeSealer
        .seal(&serde_json::to_vec(&value).unwrap())
        .unwrap()
}

#[test]
fn UnattendedSingleRun_Schema2RequiresExplicitNoRestore_001() {
    let dir = directory();
    let store = create(dir.path());
    initialize(&store);
    let value = schema_bytes(&store, 2, Some("single_run_no_restore"));
    // 首次 RED：原 schema1 decoder 尚不认识严格 single-run 模式。
    let decoded = store.decode(&value);
    drop(store);
    assert!(
        decoded.is_ok(),
        "explicit schema2 single-run record must be recognized: {decoded:?}"
    );
}

#[test]
fn UnattendedSingleRun_LegacyBytesAndUnknownModesStayClosed_002() {
    let dir = directory();
    let store = create(dir.path());
    let original = initialize(&store);
    assert_eq!(store.decode(&raw(&store)).unwrap(), original.0);
    for (schema, mode) in [
        (2, None),
        (2, Some("restore")),
        (1, Some("single_run_no_restore")),
        (3, Some("single_run_no_restore")),
    ] {
        assert!(store.decode(&schema_bytes(&store, schema, mode)).is_err());
    }
}

#[test]
fn UnattendedSingleRun_RealJournalModeAndLockedReopen_003() {
    let dir = directory();
    let store = create(dir.path());
    let mut initial_permit = permit(&store, initial(), 1000, 1000);
    initial_permit.mode = Some(ExecutionMode::SingleRunNoRestore);
    let disabled = store.initialize(initial_permit).unwrap();
    assert_eq!(disabled.0.schema, 2);
    let mut enable_permit = permit(&store, disabled.expected(), 1000, 2000);
    enable_permit.mode = Some(ExecutionMode::SingleRunNoRestore);
    let enabled = store.enable(enable_permit, 1000).unwrap();
    let original_bytes = raw(&store);
    let original_stamp = store.stamp(&enabled.0, &original_bytes).unwrap();
    let mut missing_mode = enabled.0.clone();
    missing_mode.mode = None;
    assert_eq!(validate(&missing_mode), Err(StoreError::Invalid));
    let changed_bytes = store.encode(&missing_mode).unwrap();
    assert_ne!(
        store.stamp(&missing_mode, &changed_bytes).unwrap().blob,
        original_stamp.blob
    );
    drop(store);
    let reopened = reopen(dir.path());
    // 读入类型始终是 LockedSnapshot；本例不把历史 Enabled 解释成运行许可。
    let loaded = reopened.observe(&enabled.expected(), 1000).unwrap();
    assert_eq!(loaded, enabled);
    assert_eq!(raw(&reopened), original_bytes);
    let closed = reopened.revoke(&loaded.expected(), 1100).unwrap();
    assert_eq!(closed.0.status, Status::Revoked);
    assert_eq!(closed.0.mode, Some(ExecutionMode::SingleRunNoRestore));
}

#[test]
fn UnattendedSingleRun_ModeCannotUpgradeLegacyPermit_004() {
    let dir = directory();
    let store = create(dir.path());
    let disabled = initialize(&store);
    let old_bytes = raw(&store);
    let mut changed = permit(&store, disabled.expected(), 1000, 2000);
    changed.mode = Some(ExecutionMode::SingleRunNoRestore);
    assert_eq!(store.enable(changed, 1000), Err(StoreError::Unauthorized));
    assert_eq!(raw(&store), old_bytes);
}
