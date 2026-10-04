use super::*;
use crate::version_history::{
    catalog::{parse_release, CatalogService, CatalogSource, ReleaseMetadata},
    policy::HostPlatform,
};
use std::sync::Arc;

const MEASURED: &str =
    include_str!("../../../tests/fixtures/version-history-payload/v0.17.7-measured.json");
const REVIEWED_MEASUREMENTS: &str =
    include_str!("../../../tests/fixtures/version-history-payload/reviewed-measurements.json");
const FIXTURE_CATALOG: &str =
    include_str!("../../../tests/fixtures/version-history-payload/catalog.json");

struct Source(ReleaseMetadata);
impl CatalogSource for Source {
    fn list(&self, _page: u16) -> Result<Vec<ReleaseMetadata>, SafeError> {
        Ok(vec![self.0.clone()])
    }
    fn release(&self, _release_id: u64) -> Result<ReleaseMetadata, SafeError> {
        Ok(self.0.clone())
    }
}

// 只有测量过的版本、安装包大小和摘要精确匹配；生产切换仍由协调器门禁拒绝。
#[test]
fn HistoryPayload_ExactAdmission_001() {
    use crate::cli::{snapshot::CallerIdentity, types::WireU64};
    let mut held: Option<PayloadAdmission> = None;
    for variant in 0..4 {
        let mut release = parse_release(include_bytes!(
            "../../../tests/fixtures/version-history-payload/v0.17.7-selection.json"
        ))
        .unwrap();
        match variant {
            1 => release.assets[0].size += 1,
            2 => release.assets[0].digest = Some(format!("sha256:{}", "a".repeat(64))),
            3 => {
                release = parse_release(include_bytes!(
                    "../../../tests/fixtures/version-history-payload/v0.17.6-selection.json"
                ))
                .unwrap();
                release.assets[0].size = 4_966_193;
                release.assets[0].digest = Some(
                    "sha256:e9ffbc5ba627f0c133a4385db404342a7344729339185e6f9b8ee6b5969086ac"
                        .into(),
                );
            }
            _ => {}
        }
        let catalog = CatalogService::new(Arc::new(Source(release)), HostPlatform::WindowsX64);
        let caller = CallerIdentity {
            instance_id: "payload-policy-test".into(),
            window_label: "main".into(),
            webview_epoch: WireU64::parse("1").unwrap(),
        };
        let page = catalog.list(&caller, None).unwrap();
        let row = &page.rows[0];
        let selected = catalog
            .select(&caller, &row.release_id, row.asset_id.as_deref().unwrap())
            .unwrap();
        let selection = catalog
            .resolve_selection(&caller, &selected.selection_token)
            .unwrap();
        let result = PayloadAdmission::for_selection(&selection);
        if variant == 0 {
            let admission = result.unwrap();
            assert_eq!(admission.installed_bytes(), 18_599_575);
            assert!(matches!(
                review_block(&selection),
                Some(super::super::manager::SwitchReviewBlock::CoordinatorUnavailable)
            ));
            admission.verify_selection(&selection).unwrap();
            held = Some(admission);
        } else {
            assert_eq!(result.err().unwrap().code, "HISTORY_PAYLOAD_UNVERIFIED");
            assert_eq!(
                held.as_ref()
                    .unwrap()
                    .verify_selection(&selection)
                    .unwrap_err()
                    .code,
                "HISTORY_TARGET_CHANGED"
            );
            assert!(matches!(
                review_block(&selection),
                Some(super::super::manager::SwitchReviewBlock::PayloadUnverified)
            ));
        }
    }
}

// 清洁安装和种子安装具有同一目标输出，旧主程序可移除而来源附属文件必须保留。
#[test]
fn HistoryPayload_ExactInventory_002() {
    let fixture: serde_json::Value = serde_json::from_str(MEASURED).unwrap();
    let source: Vec<ManifestEntry> = serde_json::from_value(fixture["source"].clone()).unwrap();
    let installed: Vec<ManifestEntry> =
        serde_json::from_value(fixture["installed"].clone()).unwrap();
    validate_installed_inventory(&REVIEWED[0], "Old Desk.exe", &source, &installed).unwrap();
    let clean_source = &source[..2];
    let clean_target = &installed[..installed.len() - 1];
    validate_installed_inventory(&REVIEWED[0], "Old Desk.exe", clean_source, clean_target).unwrap();
}

// 每个测量输出缺失、内容摘要变化或大小变化都必须被拒绝。
#[test]
fn HistoryPayload_OutputMismatch_003() {
    let fixture: serde_json::Value = serde_json::from_str(MEASURED).unwrap();
    let source: Vec<ManifestEntry> = serde_json::from_value(fixture["source"].clone()).unwrap();
    let installed: Vec<ManifestEntry> =
        serde_json::from_value(fixture["installed"].clone()).unwrap();
    for file in REVIEWED[0].installed_inventory {
        let index = installed
            .iter()
            .position(|entry| entry.metadata.path == file.path)
            .unwrap();
        for change in 0..5 {
            let mut changed = installed.clone();
            match change {
                0 => {
                    changed.remove(index);
                }
                1 => changed[index].sha256 = Some("a".repeat(64)),
                2 => changed[index].metadata.size += 1,
                3 => changed[index].metadata.kind = EntryType::Directory,
                _ => {
                    changed[index].metadata.permissions = PermissionRecord::Windows {
                        descriptor: vec![1, 2, 3],
                        attributes: 33,
                    }
                }
            }
            assert!(
                validate_installed_inventory(&REVIEWED[0], "Old Desk.exe", &source, &changed)
                    .is_err(),
                "measured output {} alteration {change} must be rejected",
                file.path
            );
        }
    }
}

// 来源附属文件必须保留同一内容、对象身份、链接数、权限及属性；不能仅检查文件名。
#[test]
fn HistoryPayload_CompanionMismatch_004() {
    let fixture: serde_json::Value = serde_json::from_str(MEASURED).unwrap();
    let source: Vec<ManifestEntry> = serde_json::from_value(fixture["source"].clone()).unwrap();
    let installed: Vec<ManifestEntry> =
        serde_json::from_value(fixture["installed"].clone()).unwrap();
    let index = installed.len() - 1;
    for change in 0..8 {
        let mut changed = installed.clone();
        match change {
            0 => {
                changed.remove(index);
            }
            1 => changed[index].sha256 = Some("a".repeat(64)),
            2 => changed[index].metadata.size += 1,
            3 => changed[index].metadata.object_identity = "b".repeat(64),
            4 => changed[index].metadata.link_count = 2,
            5 => {
                changed[index].metadata.permissions = PermissionRecord::Windows {
                    descriptor: vec![4, 5, 6],
                    attributes: 32,
                }
            }
            6 => {
                changed[index].metadata.permissions = PermissionRecord::Windows {
                    descriptor: vec![1, 2, 3],
                    attributes: 33,
                }
            }
            _ => changed[index].metadata.path = "other-companion.txt".into(),
        }
        assert!(
            validate_installed_inventory(&REVIEWED[0], "Old Desk.exe", &source, &changed).is_err(),
            "preserved companion alteration {change} must be rejected"
        );
    }
}

// 新增文件、目录、旧主程序副本及重复路径都不能被当作保留附属文件接受。
#[test]
fn HistoryPayload_RejectExtraEntries_005() {
    let fixture: serde_json::Value = serde_json::from_str(MEASURED).unwrap();
    let source: Vec<ManifestEntry> = serde_json::from_value(fixture["source"].clone()).unwrap();
    let installed: Vec<ManifestEntry> =
        serde_json::from_value(fixture["installed"].clone()).unwrap();
    for change in 0..4 {
        let mut changed = installed.clone();
        let mut extra = source[1].clone();
        match change {
            0 => extra.metadata.path = "unexpected.dll".into(),
            1 => {
                extra.metadata.path = "unexpected-directory".into();
                extra.metadata.kind = EntryType::Directory;
                extra.metadata.size = 0;
                extra.sha256 = None;
            }
            2 => {}
            _ => extra = installed[1].clone(),
        }
        changed.push(extra);
        assert!(
            validate_installed_inventory(&REVIEWED[0], "Old Desk.exe", &source, &changed).is_err(),
            "extra entry case {change} must be rejected"
        );
    }
}

// 大小写别名、路径分隔符、硬链接、重解析类型和对象别名全部拒绝。
#[test]
fn HistoryPayload_RejectAliases_006() {
    let fixture: serde_json::Value = serde_json::from_str(MEASURED).unwrap();
    let source: Vec<ManifestEntry> = serde_json::from_value(fixture["source"].clone()).unwrap();
    let installed: Vec<ManifestEntry> =
        serde_json::from_value(fixture["installed"].clone()).unwrap();
    for change in 0..8 {
        let mut changed = installed.clone();
        match change {
            0 => changed[1].metadata.path = "license-microsoft-conpty.txt".into(),
            1 => changed[1].metadata.path = "../LICENSE-Microsoft-ConPTY.txt".into(),
            2 => changed[1].metadata.path = "x\\LICENSE-Microsoft-ConPTY.txt".into(),
            3 => changed[1].metadata.path = "LICENSE-Microsoft-ConPTY.txt:stream".into(),
            4 => changed[1].metadata.link_count = 2,
            5 => changed[1].metadata.kind = EntryType::LinkOrReparse,
            6 => changed[1].metadata.object_identity = changed[2].metadata.object_identity.clone(),
            _ => changed[1].metadata.path.push('.'),
        }
        assert!(
            validate_installed_inventory(&REVIEWED[0], "Old Desk.exe", &source, &changed).is_err(),
            "alias case {change} must be rejected"
        );
    }
}

// 安装根必须保留原始对象及权限，缺失来源主程序不得建立保留集合。
#[test]
fn HistoryPayload_RejectSourceChanges_007() {
    let fixture: serde_json::Value = serde_json::from_str(MEASURED).unwrap();
    let source: Vec<ManifestEntry> = serde_json::from_value(fixture["source"].clone()).unwrap();
    let installed: Vec<ManifestEntry> =
        serde_json::from_value(fixture["installed"].clone()).unwrap();
    let mut replaced_root = installed.clone();
    replaced_root[0].metadata.object_identity = "a".repeat(64);
    assert!(
        validate_installed_inventory(&REVIEWED[0], "Old Desk.exe", &source, &replaced_root)
            .is_err()
    );
    assert!(
        validate_installed_inventory(&REVIEWED[0], "missing.exe", &source, &installed).is_err()
    );
    let mut source_alias = source.clone();
    source_alias[2].metadata.path = "CC-DESK.EXE".into();
    assert!(
        validate_installed_inventory(&REVIEWED[0], "Old Desk.exe", &source_alias, &installed)
            .is_err()
    );
}

// 已存在的目录及其文件必须整体保留，不能只保留目录名称而丢失子文件。
#[test]
fn HistoryPayload_PreserveDirectory_008() {
    let fixture: serde_json::Value = serde_json::from_str(MEASURED).unwrap();
    let mut source: Vec<ManifestEntry> = serde_json::from_value(fixture["source"].clone()).unwrap();
    let mut installed: Vec<ManifestEntry> =
        serde_json::from_value(fixture["installed"].clone()).unwrap();
    let mut directory = source[0].clone();
    directory.metadata.path = "retained".into();
    directory.metadata.object_identity = "9".repeat(64);
    let mut child = source[2].clone();
    child.metadata.path = "retained/child.txt".into();
    child.metadata.object_identity = "a".repeat(64);
    source.extend([directory.clone(), child.clone()]);
    installed.extend([directory, child]);
    validate_installed_inventory(&REVIEWED[0], "Old Desk.exe", &source, &installed).unwrap();
    installed.pop();
    assert!(
        validate_installed_inventory(&REVIEWED[0], "Old Desk.exe", &source, &installed).is_err()
    );
}

// 摘要、权限、目录完整性及清单容量无效时，拒绝整个清单而不是跳过条目。
#[test]
fn HistoryPayload_RejectMalformed_009() {
    let fixture: serde_json::Value = serde_json::from_str(MEASURED).unwrap();
    let source: Vec<ManifestEntry> = serde_json::from_value(fixture["source"].clone()).unwrap();
    let installed: Vec<ManifestEntry> =
        serde_json::from_value(fixture["installed"].clone()).unwrap();
    for change in 0..8 {
        let mut changed = installed.clone();
        match change {
            0 => changed[1].sha256 = None,
            1 => changed[1].metadata.object_identity = "unvalidated-id".into(),
            2 => changed[1].metadata.permissions = PermissionRecord::Unix { mode: 0o644 },
            3 => {
                changed[1].metadata.permissions = PermissionRecord::Windows {
                    descriptor: vec![],
                    attributes: 32,
                }
            }
            4 => changed[1].metadata.path = "absent-parent/child.txt".into(),
            5 => changed[1].metadata.path = "CON.txt".into(),
            6 => changed[1].metadata.size = SnapshotLimits::default().max_file_bytes + 1,
            _ => {
                changed.remove(0);
            }
        }
        assert!(
            validate_installed_inventory(&REVIEWED[0], "Old Desk.exe", &source, &changed).is_err(),
            "malformed entry case {change} must be rejected"
        );
    }
}

// 九个版本只接受独立审查的精确包与清单摘要，生产入口仍显示协调器不可用。
#[test]
fn HistoryPayload_ReviewedMatrix_010() {
    use crate::cli::{snapshot::CallerIdentity, types::WireU64};
    let fixtures: serde_json::Value = serde_json::from_str(FIXTURE_CATALOG).unwrap();
    let measurements: serde_json::Value = serde_json::from_str(REVIEWED_MEASUREMENTS).unwrap();
    let expected = measurements["payloads"].as_array().unwrap();
    assert_eq!(expected.len(), 9);
    assert_eq!(REVIEWED.len(), expected.len());
    let mut digests = BTreeSet::new();
    for fixture in fixtures["fixtures"].as_array().unwrap() {
        let version = fixture["version"].as_str().unwrap();
        let measured = expected
            .iter()
            .find(|row| row["version"].as_str() == Some(version))
            .unwrap();
        let release: ReleaseMetadata =
            serde_json::from_value(fixture["selection"].clone()).unwrap();
        let catalog = CatalogService::new(Arc::new(Source(release)), HostPlatform::WindowsX64);
        let caller = CallerIdentity {
            instance_id: "payload-matrix-test".into(),
            window_label: "main".into(),
            webview_epoch: WireU64::parse("1").unwrap(),
        };
        let page = catalog.list(&caller, None).unwrap();
        let row = &page.rows[0];
        let selected = catalog
            .select(&caller, &row.release_id, row.asset_id.as_deref().unwrap())
            .unwrap();
        let selection = catalog
            .resolve_selection(&caller, &selected.selection_token)
            .unwrap();
        let admission = PayloadAdmission::for_selection(&selection).unwrap();
        admission.verify_selection(&selection).unwrap();
        assert_eq!(admission.measured.version, version);
        assert_eq!(
            admission.installed_bytes(),
            measured["installed_bytes"].as_u64().unwrap(),
            "{version} must retain only target-produced bytes"
        );
        assert_eq!(
            serde_json::to_value(admission.measured.installed_inventory).unwrap(),
            measured["installed_inventory"],
            "{version} must retain every reviewed path, size, digest and attribute"
        );
        assert_eq!(
            admission.inventory_digest(),
            measured["inventory_digest"].as_str().unwrap(),
            "{version} inventory digest must match the reviewed canonical bytes"
        );
        assert!(digests.insert(admission.inventory_digest().to_string()));
        assert!(!roundtrip_enabled(&selection));
        assert!(matches!(
            review_block(&selection),
            Some(super::super::manager::SwitchReviewBlock::CoordinatorUnavailable)
        ));
    }
    assert_eq!(digests.len(), expected.len());
}

// 各版本的大小、摘要及借用相邻版本包身份都必须拒绝，不能只按版本放行。
#[test]
fn HistoryPayload_MatrixMismatch_011() {
    use crate::cli::{snapshot::CallerIdentity, types::WireU64};
    let fixtures: serde_json::Value = serde_json::from_str(FIXTURE_CATALOG).unwrap();
    let fixtures = fixtures["fixtures"].as_array().unwrap();
    for (index, fixture) in fixtures.iter().enumerate() {
        let mut held: Option<PayloadAdmission> = None;
        for variant in 0..4 {
            let mut release: ReleaseMetadata =
                serde_json::from_value(fixture["selection"].clone()).unwrap();
            match variant {
                1 => release.assets[0].size += 1,
                2 => release.assets[0].digest = Some(format!("sha256:{}", "a".repeat(64))),
                3 => {
                    let other: ReleaseMetadata = serde_json::from_value(
                        fixtures[(index + 1) % fixtures.len()]["selection"].clone(),
                    )
                    .unwrap();
                    release.assets[0].size = other.assets[0].size;
                    release.assets[0].digest = other.assets[0].digest.clone();
                }
                _ => {}
            }
            let catalog = CatalogService::new(Arc::new(Source(release)), HostPlatform::WindowsX64);
            let caller = CallerIdentity {
                instance_id: "payload-mismatch-test".into(),
                window_label: "main".into(),
                webview_epoch: WireU64::parse("1").unwrap(),
            };
            let page = catalog.list(&caller, None).unwrap();
            let row = &page.rows[0];
            let selected = catalog
                .select(&caller, &row.release_id, row.asset_id.as_deref().unwrap())
                .unwrap();
            let selection = catalog
                .resolve_selection(&caller, &selected.selection_token)
                .unwrap();
            let result = PayloadAdmission::for_selection(&selection);
            if variant == 0 {
                held = Some(result.unwrap());
            } else {
                assert_eq!(
                    result.err().unwrap().code,
                    "HISTORY_PAYLOAD_UNVERIFIED",
                    "{} alteration {variant} must not borrow reviewed authority",
                    selection.version()
                );
                assert_eq!(
                    held.as_ref()
                        .unwrap()
                        .verify_selection(&selection)
                        .unwrap_err()
                        .code,
                    "HISTORY_TARGET_CHANGED"
                );
                assert!(matches!(
                    review_block(&selection),
                    Some(super::super::manager::SwitchReviewBlock::PayloadUnverified)
                ));
            }
        }
    }
}

// 两个分别已审查的版本也不能互换已持有的选择身份，覆盖全部 72 个跨版本方向。
#[test]
fn HistoryPayload_CrossVersion_012() {
    use crate::cli::{snapshot::CallerIdentity, types::WireU64};
    let fixtures: serde_json::Value = serde_json::from_str(FIXTURE_CATALOG).unwrap();
    let mut selections = Vec::new();
    for fixture in fixtures["fixtures"].as_array().unwrap() {
        let release: ReleaseMetadata =
            serde_json::from_value(fixture["selection"].clone()).unwrap();
        let catalog = CatalogService::new(Arc::new(Source(release)), HostPlatform::WindowsX64);
        let caller = CallerIdentity {
            instance_id: "payload-cross-version-test".into(),
            window_label: "main".into(),
            webview_epoch: WireU64::parse("1").unwrap(),
        };
        let page = catalog.list(&caller, None).unwrap();
        let row = &page.rows[0];
        let selected = catalog
            .select(&caller, &row.release_id, row.asset_id.as_deref().unwrap())
            .unwrap();
        selections.push(
            catalog
                .resolve_selection(&caller, &selected.selection_token)
                .unwrap(),
        );
    }
    for selected in &selections {
        let admission = PayloadAdmission::for_selection(selected).unwrap();
        for other in &selections {
            if selected.version() == other.version() {
                admission.verify_selection(other).unwrap();
            } else {
                assert_eq!(
                    admission.verify_selection(other).unwrap_err().code,
                    "HISTORY_TARGET_CHANGED",
                    "{} must not adopt {}",
                    selected.version(),
                    other.version()
                );
            }
        }
    }
}

// 九个版本的清洁及种子清单只接受精确输出，跨版本清单、丢失附属文件和额外输出均拒绝。
#[test]
fn HistoryPayload_InventoryMatrix_013() {
    let fixture: serde_json::Value = serde_json::from_str(MEASURED).unwrap();
    let source: Vec<ManifestEntry> = serde_json::from_value(fixture["source"].clone()).unwrap();
    let measurements: serde_json::Value = serde_json::from_str(REVIEWED_MEASUREMENTS).unwrap();
    for payload in measurements["payloads"].as_array().unwrap() {
        let version = payload["version"].as_str().unwrap();
        let measured = REVIEWED
            .iter()
            .find(|entry| entry.version == version)
            .unwrap();
        let mut installed = vec![source[0].clone()];
        for (index, file) in payload["installed_inventory"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            // 仅字段比较测试使用合成身份和描述符，不能构造 held-tree 执行权限。
            let mut entry = source[1].clone();
            entry.metadata.path = file["path"].as_str().unwrap().into();
            entry.metadata.size = file["size"].as_u64().unwrap();
            entry.metadata.object_identity = format!("{:064x}", index + 16);
            entry.metadata.permissions = PermissionRecord::Windows {
                descriptor: vec![1, 2, 3],
                attributes: file["attributes"].as_u64().unwrap().try_into().unwrap(),
            };
            entry.sha256 = Some(file["sha256"].as_str().unwrap().into());
            installed.push(entry);
        }
        validate_installed_inventory(measured, "Old Desk.exe", &source[..2], &installed).unwrap();
        let clean = installed.clone();
        installed.push(source[2].clone());
        validate_installed_inventory(measured, "Old Desk.exe", &source, &installed).unwrap();
        assert!(validate_installed_inventory(measured, "Old Desk.exe", &source, &clean).is_err());
        for (index, file) in measured.installed_inventory.iter().enumerate() {
            let index = index + 1;
            for change in 0..8 {
                let mut changed = installed.clone();
                match change {
                    0 => {
                        changed.remove(index);
                    }
                    1 => changed[index].sha256 = Some("a".repeat(64)),
                    2 => changed[index].metadata.size += 1,
                    3 => {
                        changed[index].metadata.permissions = PermissionRecord::Windows {
                            descriptor: vec![1, 2, 3],
                            attributes: 33,
                        }
                    }
                    4 => changed[index].metadata.path = file.path.to_uppercase(),
                    5 => changed[index].metadata.link_count = 2,
                    6 => changed[index].metadata.kind = EntryType::LinkOrReparse,
                    _ => {
                        changed[index].metadata.object_identity =
                            changed[0].metadata.object_identity.clone();
                    }
                }
                assert!(
                    validate_installed_inventory(measured, "Old Desk.exe", &source, &changed)
                        .is_err(),
                    "{version} output {} alteration {change} must be rejected",
                    file.path
                );
            }
        }
        for other in REVIEWED.iter().filter(|other| other.version != version) {
            assert!(
                validate_installed_inventory(other, "Old Desk.exe", &source, &installed).is_err(),
                "{version} inventory cannot satisfy {}",
                other.version
            );
        }
        for change in 0..3 {
            let mut changed = installed.clone();
            match change {
                0 => changed.last_mut().unwrap().sha256 = Some("b".repeat(64)),
                1 => changed[0].metadata.object_identity = "c".repeat(64),
                _ => changed.push(source[1].clone()),
            }
            assert!(
                validate_installed_inventory(measured, "Old Desk.exe", &source, &changed).is_err(),
                "{version} source preservation alteration {change} must be rejected"
            );
        }
    }
}
