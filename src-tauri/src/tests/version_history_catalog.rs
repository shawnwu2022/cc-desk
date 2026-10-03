use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{SafeError, WireU64};
use crate::version_history::catalog::{
    classify_http_status, parse_catalog_page, parse_release, CatalogService, CatalogSource,
    ReleaseMetadata, CATALOG_PAGE_SIZE, MAX_CATALOG_BYTES, SELECTION_TTL,
};
use crate::version_history::policy::HostPlatform;
use crate::version_history::types::{HistoryBlockReason, HistoryDataMode, HistoryVerification};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

const OBSERVED: &[u8] = include_bytes!("../../../tests/fixtures/version-history-releases.json");

struct Source {
    list: Vec<ReleaseMetadata>,
    release: Mutex<ReleaseMetadata>,
}
impl CatalogSource for Source {
    fn list(&self, page: u16) -> Result<Vec<ReleaseMetadata>, SafeError> {
        let start = (usize::from(page) - 1) * CATALOG_PAGE_SIZE;
        Ok(self
            .list
            .get(start..self.list.len().min(start + CATALOG_PAGE_SIZE))
            .unwrap_or(&[])
            .to_vec())
    }
    fn release(&self, _release_id: u64) -> Result<ReleaseMetadata, SafeError> {
        Ok(self.release.lock().clone())
    }
}

// 九个真实形状的官方版本可选择，但元数据不能证明安装准备完成。
#[test]
fn HistoryCatalog_NineReleases_001() {
    let releases = parse_catalog_page(OBSERVED).unwrap();
    let source = Arc::new(Source {
        list: releases.clone(),
        release: Mutex::new(releases[0].clone()),
    });
    let service = CatalogService::new(source, HostPlatform::WindowsX64);
    let caller = CallerIdentity {
        instance_id: "desk".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let page = service.list_at(&caller, None, Instant::now()).unwrap();
    assert_eq!(page.rows.len(), 9);
    let versions: Vec<_> = page.rows.iter().map(|row| row.version.as_str()).collect();
    assert_eq!(
        versions,
        [
            "0.17.7", "0.17.6", "0.17.5", "0.17.2", "0.17.1", "0.17.0", "0.16.0", "0.15.0",
            "0.14.0"
        ]
    );
    for row in page.rows {
        assert!(row.select_allowed);
        assert!(!row.install_ready);
        assert_eq!(row.verification, HistoryVerification::AwaitingVerification);
        assert_eq!(
            row.data_modes.keep_current_data,
            HistoryDataMode::Unavailable
        );
        let wire = serde_json::to_string(&row).unwrap();
        assert!(!wire.contains("https://"));
        assert!(!wire.contains("sha256"));
    }
}

// 缺少分离签名时保留版本行，并明确阻止选择。
#[test]
fn HistoryCatalog_MissingSignature_002() {
    let mut fixture: Vec<Value> = serde_json::from_slice(OBSERVED).unwrap();
    fixture[0]["assets"]
        .as_array_mut()
        .unwrap()
        .retain(|asset| asset["name"] != "CC.Desk_0.17.7_x64-setup.exe.sig");
    let releases = parse_catalog_page(&serde_json::to_vec(&fixture).unwrap()).unwrap();
    let row = releases[0].project(HostPlatform::WindowsX64, "a".repeat(32));
    assert_eq!(
        row.blocked_reason,
        Some(HistoryBlockReason::SignatureMissing)
    );
    assert!(!row.select_allowed);
}

// 同名安装包重复、SHA256缺失及跨仓库下载地址都不能获得选择资格。
#[test]
fn HistoryCatalog_AssetAmbiguity_003() {
    let original: Vec<Value> = serde_json::from_slice(OBSERVED).unwrap();
    for (mutation, reason) in [
        ("duplicate", HistoryBlockReason::AssetAmbiguous),
        ("digest", HistoryBlockReason::DigestUnavailable),
        ("host", HistoryBlockReason::AssetMetadataInvalid),
    ] {
        let mut release = original[0].clone();
        let index = release["assets"]
            .as_array()
            .unwrap()
            .iter()
            .position(|asset| asset["name"] == "CC.Desk_0.17.7_x64-setup.exe")
            .unwrap();
        match mutation {
            "duplicate" => {
                let mut other = release["assets"][index].clone();
                other["id"] = json!(900);
                release["assets"].as_array_mut().unwrap().push(other);
            }
            "digest" => release["assets"][index]["digest"] = Value::Null,
            "host" => {
                release["assets"][index]["browser_download_url"] =
                    json!("https://github.com/attacker/cc-desk/releases/download/v0.17.7/setup.exe")
            }
            _ => unreachable!(),
        }
        let release = parse_release(&serde_json::to_vec(&release).unwrap()).unwrap();
        assert_eq!(
            release
                .project(HostPlatform::WindowsX64, "a".repeat(32))
                .blocked_reason,
            Some(reason)
        );
    }
}

// 草稿、预发布、工作流测试名称和非规范版本不会混入历史目录。
#[test]
fn HistoryCatalog_ReleaseFiltering_004() {
    let original: Vec<Value> = serde_json::from_slice(OBSERVED).unwrap();
    for (field, value) in [
        ("draft", json!(true)),
        ("prerelease", json!(true)),
        ("name", json!("CC Desk test candidate")),
        ("tag_name", json!("v0.17.7-beta.1")),
        ("tag_name", json!("v00.17.7")),
    ] {
        let mut release = original[0].clone();
        release[field] = value;
        let fixture = serde_json::to_vec(&vec![release]).unwrap();
        let releases = parse_catalog_page(&fixture).unwrap();
        let service = CatalogService::new(
            Arc::new(Source {
                list: releases.clone(),
                release: Mutex::new(releases[0].clone()),
            }),
            HostPlatform::WindowsX64,
        );
        let caller = CallerIdentity {
            instance_id: "desk".into(),
            window_label: "main".into(),
            webview_epoch: WireU64::parse("1").unwrap(),
        };
        assert!(service
            .list_at(&caller, None, Instant::now())
            .unwrap()
            .rows
            .is_empty());
    }
}

// macOS和未审查包装版本不能把Windows包当成可安装目标。
#[test]
fn HistoryCatalog_PlatformPolicy_005() {
    let releases = parse_catalog_page(OBSERVED).unwrap();
    assert_eq!(
        releases[0]
            .project(HostPlatform::Unsupported, "a".repeat(32))
            .blocked_reason,
        Some(HistoryBlockReason::PlatformUnsupported)
    );
    let mut release: Value = serde_json::from_slice::<Vec<Value>>(OBSERVED)
        .unwrap()
        .remove(0);
    release["tag_name"] = json!("v0.13.0");
    release["name"] = json!("CC Desk v0.13.0");
    release["html_url"] = json!("https://github.com/shawnwu2022/cc-desk/releases/tag/v0.13.0");
    let release = parse_release(&serde_json::to_vec(&release).unwrap()).unwrap();
    assert_eq!(
        release
            .project(HostPlatform::WindowsX64, "a".repeat(32))
            .blocked_reason,
        Some(HistoryBlockReason::PackagingBoundaryUnknown)
    );
}

// 超量、重复ID和损坏JSON元数据按安全错误码拒绝。
#[test]
fn HistoryCatalog_MetadataBounds_006() {
    assert_eq!(
        parse_catalog_page(&vec![b' '; MAX_CATALOG_BYTES + 1])
            .unwrap_err()
            .code,
        "HISTORY_METADATA_TOO_LARGE"
    );
    assert_eq!(
        parse_catalog_page(b"{}").unwrap_err().code,
        "HISTORY_METADATA_INVALID"
    );
    let release = serde_json::from_slice::<Vec<Value>>(OBSERVED)
        .unwrap()
        .remove(0);
    assert_eq!(
        parse_catalog_page(
            &serde_json::to_vec(&vec![release.clone(); CATALOG_PAGE_SIZE + 1]).unwrap()
        )
        .unwrap_err()
        .code,
        "HISTORY_METADATA_INVALID"
    );
    assert_eq!(
        parse_catalog_page(&serde_json::to_vec(&vec![release.clone(), release]).unwrap())
            .unwrap_err()
            .code,
        "HISTORY_METADATA_INVALID"
    );
}

// 限流、未找到与服务故障不会透出服务器正文或提供不可信替代目录。
#[test]
fn HistoryCatalog_HttpPolicy_007() {
    assert_eq!(
        classify_http_status(403).unwrap_err().code,
        "HISTORY_RATE_LIMITED"
    );
    assert_eq!(
        classify_http_status(429).unwrap_err().code,
        "HISTORY_RATE_LIMITED"
    );
    assert_eq!(
        classify_http_status(404).unwrap_err().code,
        "HISTORY_RELEASE_UNAVAILABLE"
    );
    assert_eq!(
        classify_http_status(302).unwrap_err().code,
        "HISTORY_NETWORK_UNAVAILABLE"
    );
}

// 选择绑定精确安装包和签名；重新发布元数据时必须重新选择。
#[test]
fn HistoryCatalog_SelectionChanged_008() {
    let releases = parse_catalog_page(OBSERVED).unwrap();
    let source = Arc::new(Source {
        list: releases.clone(),
        release: Mutex::new(releases[0].clone()),
    });
    let service = CatalogService::new(source.clone(), HostPlatform::WindowsX64);
    let caller = CallerIdentity {
        instance_id: "desk".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let now = Instant::now();
    let row = service.list_at(&caller, None, now).unwrap().rows.remove(0);
    let selected = service
        .select_at(
            &caller,
            &row.release_id,
            row.asset_id.as_ref().unwrap(),
            now,
        )
        .unwrap();
    let bound = service
        .resolve_selection_at(&caller, &selected.selection_token, now)
        .unwrap();
    assert_eq!(bound.release_id(), 392398817);
    assert_eq!(bound.installer().id(), 576637999);
    assert_eq!(bound.signature().id(), 576637991);
    source.release.lock().assets[5].size += 1;
    assert_eq!(
        service
            .revalidate_selection_at(&caller, &selected.selection_token, now)
            .unwrap_err()
            .code,
        "HISTORY_SELECTION_CHANGED"
    );
    assert_eq!(
        service
            .select_at(
                &caller,
                &row.release_id,
                row.asset_id.as_ref().unwrap(),
                now
            )
            .unwrap_err()
            .code,
        "HISTORY_SELECTION_CHANGED"
    );
}

// 到期或其他文档不能复用已选择的执行包身份。
#[test]
fn HistoryCatalog_SelectionLifetime_009() {
    let releases = parse_catalog_page(OBSERVED).unwrap();
    let now = Instant::now();
    let service = CatalogService::with_clock(
        Arc::new(Source {
            list: releases.clone(),
            release: Mutex::new(releases[0].clone()),
        }),
        HostPlatform::WindowsX64,
        Arc::new(move || now),
    );
    let caller = CallerIdentity {
        instance_id: "desk".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let other = CallerIdentity {
        instance_id: "desk".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("2").unwrap(),
    };
    let row = service.list_at(&caller, None, now).unwrap().rows.remove(0);
    let selected = service
        .select_at(
            &caller,
            &row.release_id,
            row.asset_id.as_ref().unwrap(),
            now,
        )
        .unwrap();
    assert_eq!(
        service
            .resolve_selection_at(&other, &selected.selection_token, now)
            .unwrap_err()
            .code,
        "HISTORY_SELECTION_UNKNOWN"
    );
    assert_eq!(
        service
            .resolve_selection_at(&caller, &selected.selection_token, now + SELECTION_TTL)
            .unwrap_err()
            .code,
        "HISTORY_SELECTION_EXPIRED"
    );
}

// 目录最多十个25行页面；游标属于原文档，不能由URL构造。
#[test]
fn HistoryCatalog_Pagination_010() {
    let base = parse_catalog_page(OBSERVED).unwrap().remove(0);
    let releases: Vec<_> = (0..250)
        .map(|index| {
            let mut release = base.clone();
            release.id = 1000 + index;
            release.tag_name = format!("v0.13.{index}");
            release.name = None;
            release.html_url = format!(
                "https://github.com/shawnwu2022/cc-desk/releases/tag/{}",
                release.tag_name
            );
            release
        })
        .collect();
    let now = Instant::now();
    let service = CatalogService::with_clock(
        Arc::new(Source {
            list: releases,
            release: Mutex::new(base),
        }),
        HostPlatform::WindowsX64,
        Arc::new(move || now),
    );
    let caller = CallerIdentity {
        instance_id: "desk".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let other = CallerIdentity {
        instance_id: "other".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    assert_eq!(
        service
            .list_at(
                &caller,
                Some("https://api.github.com/repos/other/releases?page=2"),
                now
            )
            .unwrap_err()
            .code,
        "HISTORY_CURSOR_INVALID"
    );
    let mut page = service.list_at(&caller, None, now).unwrap();
    let cursor = page.next_cursor.as_deref().unwrap();
    assert_eq!(
        service.list_at(&other, Some(cursor), now).unwrap_err().code,
        "HISTORY_CURSOR_INVALID"
    );
    assert_eq!(
        service
            .list_at(&caller, Some(cursor), now + SELECTION_TTL)
            .unwrap_err()
            .code,
        "HISTORY_CURSOR_EXPIRED"
    );
    for _ in 1..10 {
        assert_eq!(page.rows.len(), 25);
        assert!(!page.truncated);
        page = service
            .list_at(&caller, page.next_cursor.as_deref(), now)
            .unwrap();
    }
    assert_eq!(page.rows.len(), 25);
    assert!(page.truncated);
    assert!(page.next_cursor.is_none());
}

// 错版本、x86包、重复签名和未上传签名不得授权准备下载。
#[test]
fn HistoryCatalog_PackageIdentity_011() {
    let original = serde_json::from_slice::<Vec<Value>>(OBSERVED)
        .unwrap()
        .remove(0);
    for (mutation, reason) in [
        ("version", HistoryBlockReason::PlatformAssetMissing),
        ("architecture", HistoryBlockReason::PlatformAssetMissing),
        ("signature", HistoryBlockReason::AssetAmbiguous),
        ("state", HistoryBlockReason::AssetMetadataInvalid),
        ("digest", HistoryBlockReason::DigestUnavailable),
    ] {
        let mut release = original.clone();
        match mutation {
            "version" => release["assets"][5]["name"] = json!("CC.Desk_0.17.6_x64-setup.exe"),
            "architecture" => release["assets"][5]["name"] = json!("CC.Desk_0.17.7_x86-setup.exe"),
            "signature" => {
                let mut asset = release["assets"][6].clone();
                asset["id"] = json!(901);
                release["assets"].as_array_mut().unwrap().push(asset);
            }
            "state" => release["assets"][6]["state"] = json!("new"),
            "digest" => {
                release["assets"][5]["digest"] = json!(format!("sha256:{}", "G".repeat(64)))
            }
            _ => unreachable!(),
        }
        let release = parse_release(&serde_json::to_vec(&release).unwrap()).unwrap();
        assert_eq!(
            release
                .project(HostPlatform::WindowsX64, "a".repeat(32))
                .blocked_reason,
            Some(reason)
        );
    }
}

// 已选择的公开包元数据不能被更换ID、签名哈希、发布日期或时间戳代替。
#[test]
fn HistoryCatalog_MetadataMutation_012() {
    let releases = parse_catalog_page(OBSERVED).unwrap();
    let original = releases[0].clone();
    let source = Arc::new(Source {
        list: releases,
        release: Mutex::new(original.clone()),
    });
    let service = CatalogService::new(source.clone(), HostPlatform::WindowsX64);
    let caller = CallerIdentity {
        instance_id: "desk".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let now = Instant::now();
    let row = service.list_at(&caller, None, now).unwrap().rows.remove(0);
    let selected = service
        .select_at(
            &caller,
            &row.release_id,
            row.asset_id.as_ref().unwrap(),
            now,
        )
        .unwrap();
    for mutation in [
        "id",
        "signature",
        "published",
        "updated",
        "draft",
        "asset-date",
    ] {
        let mut current = original.clone();
        match mutation {
            "id" => current.assets[5].id += 1,
            "signature" => current.assets[6].digest = Some(format!("sha256:{}", "0".repeat(64))),
            "published" => current.published_at = Some("2026-09-21T00:00:00Z".into()),
            "updated" => current.updated_at = "2026-09-21T00:00:00Z".into(),
            "draft" => current.draft = true,
            "asset-date" => current.assets[5].updated_at = "2026-09-21T00:00:00Z".into(),
            _ => unreachable!(),
        }
        *source.release.lock() = current;
        assert_eq!(
            service
                .revalidate_selection_at(&caller, &selected.selection_token, now)
                .unwrap_err()
                .code,
            "HISTORY_SELECTION_CHANGED"
        );
    }
}

// 已刷新目录不会让先前观察身份采用新的安装包；新观察仍需单独选择。
#[test]
fn HistoryCatalog_ObservationIdentity_013() {
    let releases = parse_catalog_page(OBSERVED).unwrap();
    let service = CatalogService::new(
        Arc::new(Source {
            list: releases.clone(),
            release: Mutex::new(releases[0].clone()),
        }),
        HostPlatform::WindowsX64,
    );
    let caller = CallerIdentity {
        instance_id: "desk".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let now = Instant::now();
    let first = service.list_at(&caller, None, now).unwrap().rows.remove(0);
    let refreshed = service.list_at(&caller, None, now).unwrap().rows.remove(0);
    assert_ne!(first.release_id, refreshed.release_id);
    assert_eq!(
        service
            .select_at(&caller, &first.release_id, "557169459", now)
            .unwrap_err()
            .code,
        "HISTORY_SELECTION_UNKNOWN"
    );
    let selected = service
        .select_at(
            &caller,
            &first.release_id,
            first.asset_id.as_ref().unwrap(),
            now,
        )
        .unwrap();
    assert_eq!(selected.release_id, first.release_id);
}

// 元数据IO跨越选择到期边界时，返回前必须重新检查，不能交付已到期选择。
#[test]
fn HistoryCatalog_IoExpiry_014() {
    struct CrossingSource {
        release: ReleaseMetadata,
        clock: Arc<Mutex<Instant>>,
        completion: Mutex<Option<Instant>>,
    }
    impl CatalogSource for CrossingSource {
        fn list(&self, _page: u16) -> Result<Vec<ReleaseMetadata>, SafeError> {
            Ok(vec![self.release.clone()])
        }
        fn release(&self, _release_id: u64) -> Result<ReleaseMetadata, SafeError> {
            if let Some(completed) = *self.completion.lock() {
                *self.clock.lock() = completed;
            }
            Ok(self.release.clone())
        }
    }
    let now = Instant::now();
    let clock = Arc::new(Mutex::new(now));
    let source = Arc::new(CrossingSource {
        release: parse_catalog_page(OBSERVED).unwrap().remove(0),
        clock: clock.clone(),
        completion: Mutex::new(None),
    });
    let clock_read = clock.clone();
    let service = CatalogService::with_clock(
        source.clone(),
        HostPlatform::WindowsX64,
        Arc::new(move || *clock_read.lock()),
    );
    let caller = CallerIdentity {
        instance_id: "desk".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let row = service.list_at(&caller, None, now).unwrap().rows.remove(0);
    let selected = service
        .select_at(
            &caller,
            &row.release_id,
            row.asset_id.as_ref().unwrap(),
            now,
        )
        .unwrap();
    let before = now + SELECTION_TTL - std::time::Duration::from_nanos(1);
    *clock.lock() = before;
    assert!(service
        .resolve_selection_at(&caller, &selected.selection_token, before)
        .is_ok());
    *source.completion.lock() = Some(now + SELECTION_TTL);
    assert_eq!(
        service
            .revalidate_selection_at(&caller, &selected.selection_token, before)
            .unwrap_err()
            .code,
        "HISTORY_SELECTION_EXPIRED"
    );
}

// Offline diagnostics validate retained syntax and bindings, but mint no selection token.
#[test]
fn HistoryCatalog_RetainedDiagnostic_015() {
    use crate::version_history::catalog::inspect_retained_observation;
    let release = parse_catalog_page(OBSERVED).unwrap().remove(0);
    let installer = release
        .assets
        .iter()
        .find(|a| a.name.ends_with("-setup.exe"))
        .unwrap();
    let signature = release
        .assets
        .iter()
        .find(|a| a.name.ends_with("-setup.exe.sig"))
        .unwrap();
    let observation = json!({
        "schema": 1, "release": release,
        "installer_id": installer.id, "signature_id": signature.id,
    });
    let diagnostic =
        inspect_retained_observation(&serde_json::to_vec(&observation).unwrap()).unwrap();
    assert_eq!(diagnostic.version(), "0.17.7");
    assert_eq!(
        diagnostic.installer_digest(),
        installer
            .digest
            .as_ref()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap()
    );
    for mutation in ["schema", "extra", "asset", "nested"] {
        let mut changed = observation.clone();
        match mutation {
            "schema" => changed["schema"] = json!(2),
            "extra" => changed["sourceExited"] = json!(true),
            "asset" => changed["installer_id"] = json!(0),
            "nested" => changed["release"]["downloadUrl"] = json!("C:/foreign"),
            _ => unreachable!(),
        }
        assert!(inspect_retained_observation(&serde_json::to_vec(&changed).unwrap()).is_err());
    }
}
