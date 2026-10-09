//! Public upstream vectors exercise our verification and held-package boundary.
//! They are not CC Desk installers; actual official installers remain Windows CI evidence.
use crate::cli::profiles::error;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{SafeError, WireU64};
use crate::version_history::catalog::{
    parse_release, BoundAsset, CatalogService, CatalogSource, ReleaseMetadata, SELECTION_TTL,
};
use crate::version_history::download::{
    validate_redirect, AssetSource, DownloadResponse, PrepareService, MAX_REDIRECTS,
    PREPARATION_TTL,
};
use crate::version_history::policy::HostPlatform;
use crate::version_history::verified_package::{sha256, verify_fixture_payload};
use base64::{engine::general_purpose::STANDARD, Engine};
use cap_std::fs::Dir;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::io::Cursor;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;

const PAYLOAD: &[u8] =
    include_bytes!("../../../tests/fixtures/version-history-minisign/payload.bin");
const SIGNATURE: &[u8] =
    include_bytes!("../../../tests/fixtures/version-history-minisign/tauri-signature.sig");
const KEY: &str =
    include_str!("../../../tests/fixtures/version-history-minisign/tauri-public-key.txt");

// 上游真实签名通过生产使用的Minisign、Tauri封装和SHA256检查，不声明安装包身份。
#[test]
fn HistoryDownload_RealSignature_001() {
    verify_fixture_payload(
        PAYLOAD,
        SIGNATURE,
        KEY,
        &sha256(PAYLOAD),
        PAYLOAD.len() as u64,
    )
    .unwrap();
    let legacy = STANDARD.encode(include_bytes!(
        "../../../tests/fixtures/version-history-minisign/legacy.minisig"
    ));
    verify_fixture_payload(
        PAYLOAD,
        legacy.as_bytes(),
        KEY,
        &sha256(PAYLOAD),
        PAYLOAD.len() as u64,
    )
    .unwrap();
}

// 修改正文并更新校验和仍不能绕过发布者签名。
#[test]
fn HistoryDownload_ModifiedBytes_002() {
    assert_eq!(
        verify_fixture_payload(b"Test", SIGNATURE, KEY, &sha256(b"Test"), 4)
            .unwrap_err()
            .code,
        "HISTORY_SIGNATURE_INVALID"
    );
}

// 修改签名或受信注释必须失败，不能只检查文件校验和。
#[test]
fn HistoryDownload_ModifiedSignature_003() {
    let mut signature = STANDARD.decode(SIGNATURE).unwrap();
    let position = signature.iter().position(|byte| *byte == b'5').unwrap();
    signature[position] = b'6';
    let modified = STANDARD.encode(signature);
    assert_eq!(
        verify_fixture_payload(PAYLOAD, modified.as_bytes(), KEY, &sha256(PAYLOAD), 4)
            .unwrap_err()
            .code,
        "HISTORY_SIGNATURE_INVALID"
    );
}

// 同一key ID但不同公开密钥不能验证，生产配置密钥也不能接受测试密钥的包。
#[test]
fn HistoryDownload_WrongKey_004() {
    let key_text = String::from_utf8(STANDARD.decode(KEY).unwrap()).unwrap();
    let mut public = STANDARD.decode(key_text.lines().nth(1).unwrap()).unwrap();
    public[41] ^= 1;
    let wrong = STANDARD.encode(format!(
        "untrusted comment: wrong public key\n{}\n",
        STANDARD.encode(public)
    ));
    assert_eq!(
        verify_fixture_payload(PAYLOAD, SIGNATURE, &wrong, &sha256(PAYLOAD), 4)
            .unwrap_err()
            .code,
        "HISTORY_SIGNATURE_INVALID"
    );
    assert_eq!(
        verify_fixture_payload(
            PAYLOAD,
            SIGNATURE,
            &crate::version_history::policy::trusted_public_key(),
            &sha256(PAYLOAD),
            4
        )
        .unwrap_err()
        .code,
        "HISTORY_SIGNATURE_INVALID"
    );
}

// 官方SHA256或大小不一致时即使发布者签名正确也拒绝。
#[test]
fn HistoryDownload_DigestSize_005() {
    assert_eq!(
        verify_fixture_payload(PAYLOAD, SIGNATURE, KEY, &"0".repeat(64), 4)
            .unwrap_err()
            .code,
        "HISTORY_DIGEST_MISMATCH"
    );
    assert_eq!(
        verify_fixture_payload(PAYLOAD, SIGNATURE, KEY, &sha256(PAYLOAD), 5)
            .unwrap_err()
            .code,
        "HISTORY_SIZE_MISMATCH"
    );
}

// 不接受裸Minisign文本、额外行或损坏Tauri封装。
#[test]
fn HistoryDownload_SignatureEncoding_006() {
    for signature in [
        b"%%%".to_vec(),
        include_bytes!("../../../tests/fixtures/version-history-minisign/prehashed.minisig")
            .to_vec(),
        STANDARD
            .encode(format!(
                "{}extra\n",
                String::from_utf8(STANDARD.decode(SIGNATURE).unwrap()).unwrap()
            ))
            .into_bytes(),
    ] {
        assert_eq!(
            verify_fixture_payload(PAYLOAD, &signature, KEY, &sha256(PAYLOAD), 4)
                .unwrap_err()
                .code,
            "HISTORY_SIGNATURE_INVALID"
        );
    }
}

// 只允许精确GitHub官方资源主机和资源路径，拒绝凭据、明文、混淆主机及循环。
#[test]
fn HistoryDownload_RedirectPolicy_007() {
    validate_redirect("https://release-assets.githubusercontent.com/github-production-release-asset/123/abc?sig=opaque", 1).unwrap();
    validate_redirect("https://objects.githubusercontent.com/github-production-release-asset-2e65be/123/abc?sig=opaque", 2).unwrap();
    for url in ["http://release-assets.githubusercontent.com/github-production-release-asset/123/abc", "https://release-assets.githubusercontent.com.evil.example/github-production-release-asset/123/abc", "https://user@release-assets.githubusercontent.com/github-production-release-asset/123/abc", "https://release-assets.githubusercontent.com:444/github-production-release-asset/123/abc", "https://release-assets.githubusercontent.com/other/abc", "https://github.com/shawnwu2022/cc-desk/releases/download/v0.17.7/CC.Desk_0.17.7_x64-setup.exe", "https://release-assets.githubusercontent.com/github-production-release-asset/123/abc#fragment", "https://127.0.0.1/github-production-release-asset/123/abc"] {
        assert_eq!(validate_redirect(url, 1).unwrap_err().code, "HISTORY_REDIRECT_BLOCKED", "URL should not be admitted: {url}");
    }
    assert_eq!(
        validate_redirect(
            "https://release-assets.githubusercontent.com/github-production-release-asset/123/abc",
            MAX_REDIRECTS + 1
        )
        .unwrap_err()
        .code,
        "HISTORY_REDIRECT_BLOCKED"
    );
}

// 测试专用外部边界；包、事务和签名验证仍使用真实生产实现。
struct Catalog {
    release: Mutex<ReleaseMetadata>,
}
impl CatalogSource for Catalog {
    fn list(&self, _page: u16) -> Result<Vec<ReleaseMetadata>, SafeError> {
        Ok(vec![self.release.lock().clone()])
    }
    fn release(&self, _id: u64) -> Result<ReleaseMetadata, SafeError> {
        Ok(self.release.lock().clone())
    }
}
struct Assets {
    payload: Vec<u8>,
    length: Option<u64>,
    on_open: Mutex<Option<Box<dyn Fn() + Send>>>,
}
impl AssetSource for Assets {
    fn open(
        &self,
        asset: &BoundAsset,
        check: &dyn Fn() -> Result<(), SafeError>,
    ) -> Result<DownloadResponse, SafeError> {
        check()?;
        if asset.name().ends_with(".sig") {
            Ok(DownloadResponse {
                content_length: Some(SIGNATURE.len() as u64),
                body: Box::new(Cursor::new(SIGNATURE.to_vec())),
            })
        } else {
            if let Some(action) = self.on_open.lock().take() {
                action();
            }
            Ok(DownloadResponse {
                content_length: self.length,
                body: Box::new(Cursor::new(self.payload.clone())),
            })
        }
    }
}
struct Fixture {
    metadata: Arc<Catalog>,
    service: Arc<PrepareService>,
    caller: CallerIdentity,
    selection: String,
    assets: Arc<Assets>,
    clock: Arc<Mutex<Instant>>,
    live: Arc<AtomicBool>,
    _directory: tempfile::TempDir,
}
fn fixture(payload: Vec<u8>, length: Option<u64>) -> Fixture {
    let mut release: Value = serde_json::from_slice::<Vec<Value>>(include_bytes!(
        "../../../tests/fixtures/version-history-releases.json"
    ))
    .unwrap()
    .remove(0);
    release["assets"].as_array_mut().unwrap().retain(|asset| {
        asset["name"] == "CC.Desk_0.17.7_x64-setup.exe"
            || asset["name"] == "CC.Desk_0.17.7_x64-setup.exe.sig"
    });
    for asset in release["assets"].as_array_mut().unwrap() {
        let bytes = if asset["name"].as_str().unwrap().ends_with(".sig") {
            SIGNATURE
        } else {
            PAYLOAD
        };
        asset["size"] = json!(bytes.len());
        asset["digest"] = json!(format!("sha256:{}", sha256(bytes)));
    }
    let metadata = Arc::new(Catalog {
        release: Mutex::new(parse_release(&serde_json::to_vec(&release).unwrap()).unwrap()),
    });
    let clock = Arc::new(Mutex::new(Instant::now()));
    let clock_read = clock.clone();
    let catalog = Arc::new(CatalogService::with_clock(
        metadata.clone(),
        HostPlatform::WindowsX64,
        Arc::new(move || *clock_read.lock()),
    ));
    let caller = CallerIdentity {
        instance_id: "desk".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let row = catalog.list(&caller, None).unwrap().rows.remove(0);
    let selection = catalog
        .select(&caller, &row.release_id, row.asset_id.as_ref().unwrap())
        .unwrap()
        .selection_token;
    let directory = tempfile::tempdir().unwrap();
    let parent = Dir::open_ambient_dir(directory.path(), cap_std::ambient_authority()).unwrap();
    let assets = Arc::new(Assets {
        payload,
        length,
        on_open: Mutex::new(None),
    });
    let live = Arc::new(AtomicBool::new(true));
    let live_read = live.clone();
    let owner_check = Arc::new(move |_caller: &CallerIdentity| {
        if live_read.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(error("FORBIDDEN"))
        }
    });
    let clock_read = clock.clone();
    let service = Arc::new(
        PrepareService::with_test_boundaries(
            catalog.clone(),
            assets.clone(),
            parent,
            owner_check,
            Arc::new(move || *clock_read.lock()),
            KEY,
        )
        .unwrap(),
    );
    Fixture {
        _directory: directory,
        metadata,
        service,
        caller,
        selection,
        assets,
        clock,
        live,
    }
}

// 准备ID与切换UUID分离；同一owner重复点击只能观察原预约，不能拿到第二个执行能力。
#[test]
fn HistoryHandoff_IdempotentReservation_001() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let first = f
        .service
        .reserve_handoff(&f.caller, &ticket.transaction_id)
        .unwrap();
    assert!(first.transfer.is_some());
    assert_ne!(first.transaction_id, ticket.transaction_id);
    let parsed = uuid::Uuid::parse_str(&first.transaction_id).unwrap();
    assert!(!parsed.is_nil());
    assert_eq!(parsed.hyphenated().to_string(), first.transaction_id);
    let duplicate = f
        .service
        .reserve_handoff(&f.caller, &ticket.transaction_id)
        .unwrap();
    assert_eq!(first.transaction_id, duplicate.transaction_id);
    assert!(duplicate.transfer.is_none());
    first.transfer.unwrap().check().unwrap();
}

// 复制期间取消能撤销继续写入和提交；已经持有的原文件不会在复制读操作中消失。
#[test]
fn HistoryHandoff_CancelBeforeCommit_002() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let transfer = f
        .service
        .reserve_handoff(&f.caller, &ticket.transaction_id)
        .unwrap()
        .transfer
        .unwrap();
    transfer
        .with_material(|bytes, signature, observation, identity| {
            assert_eq!(bytes, PAYLOAD);
            assert_eq!(signature, SIGNATURE);
            assert!(!observation.is_empty());
            assert!(!identity.is_empty());
            f.service
                .cancel_prepare(&f.caller, &ticket.transaction_id)
                .unwrap();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(
        transfer.check().unwrap_err().code,
        "HISTORY_PREPARE_CANCELLED"
    );
    assert_eq!(
        f.service
            .reserve_handoff(&f.caller, &ticket.transaction_id)
            .err()
            .unwrap()
            .code,
        "HISTORY_PREPARE_CANCELLED"
    );
}

// 原文档失效或替换后，不能用旧预约取得复制材料或由新owner接管。
#[test]
fn HistoryHandoff_OriginalOwner_003() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let transfer = f
        .service
        .reserve_handoff(&f.caller, &ticket.transaction_id)
        .unwrap()
        .transfer
        .unwrap();
    let mut replacement = f.caller.clone();
    replacement.webview_epoch = WireU64::parse("2").unwrap();
    assert_eq!(
        f.service
            .reserve_handoff(&replacement, &ticket.transaction_id)
            .err()
            .unwrap()
            .code,
        "HISTORY_PREPARE_UNKNOWN"
    );
    f.live.store(false, Ordering::SeqCst);
    assert_eq!(
        transfer
            .with_material(|_, _, _, _| Ok(()))
            .unwrap_err()
            .code,
        "FORBIDDEN"
    );
}

// 预约重新校验完整release观察，资产之外的时间或其他metadata变化也拒绝。
#[test]
fn HistoryHandoff_MetadataChanged_004() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    f.metadata.release.lock().updated_at = "2026-10-02T12:00:00Z".into();
    assert_eq!(
        f.service
            .reserve_handoff(&f.caller, &ticket.transaction_id)
            .err()
            .unwrap()
            .code,
        "HISTORY_SELECTION_CHANGED"
    );
}

// 复制能力到期后仍保留预约账目；不得清掉旧owner然后同时产生另一个切换。
#[test]
fn HistoryHandoff_ExpiredReservation_005() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let transfer = f
        .service
        .reserve_handoff(&f.caller, &ticket.transaction_id)
        .unwrap()
        .transfer
        .unwrap();
    *f.clock.lock() += PREPARATION_TTL;
    assert_eq!(
        transfer.check().unwrap_err().code,
        "HISTORY_PREPARE_EXPIRED"
    );
    assert!(f
        .service
        .reserve_handoff(&f.caller, &ticket.transaction_id)
        .is_err());
    // 显式取消仍能终止未提交的准备预约，不谎称上下文已回滚。
    assert!(
        f.service
            .cancel_prepare(&f.caller, &ticket.transaction_id)
            .unwrap()
            .cancelled
    );
}

// selection早于准备创建，复制和提交必须继续遵守原selection的较早到期点。
#[test]
fn HistoryHandoff_SelectionExpiresDuringCopy_008() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    *f.clock.lock() += std::time::Duration::from_secs(240);
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let transfer = f
        .service
        .reserve_handoff(&f.caller, &ticket.transaction_id)
        .unwrap()
        .transfer
        .unwrap();
    *f.clock.lock() += std::time::Duration::from_secs(61);
    assert_eq!(
        transfer.check().unwrap_err().code,
        "HISTORY_SELECTION_EXPIRED"
    );
    assert_eq!(
        transfer
            .with_material(|_, _, _, _| Ok(()))
            .unwrap_err()
            .code,
        "HISTORY_SELECTION_EXPIRED"
    );
}

// 取消只撤销复制能力；已发出的切换必须经后端证明未开始或已安全abort，不能以drop代替。
#[test]
fn HistoryHandoff_CancelRetainsCapacity_009() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let transfer = f
        .service
        .reserve_handoff(&f.caller, &ticket.transaction_id)
        .unwrap()
        .transfer
        .unwrap();
    transfer
        .with_material(|_, _, _, _| {
            f.service
                .cancel_prepare(&f.caller, &ticket.transaction_id)
                .unwrap();
            assert_eq!(
                f.service
                    .begin_prepare(&f.caller, &f.selection)
                    .unwrap_err()
                    .code,
                "HISTORY_PREPARE_BUSY"
            );
            Ok(())
        })
        .unwrap_err();
    assert_eq!(
        f.service
            .begin_prepare(&f.caller, &f.selection)
            .unwrap_err()
            .code,
        "HISTORY_PREPARE_BUSY"
    );
    drop(transfer);
    assert_eq!(
        f.service
            .begin_prepare(&f.caller, &f.selection)
            .unwrap_err()
            .code,
        "HISTORY_PREPARE_BUSY"
    );
}

// 实际NTFS私有独立副本保留全部已验证字节；完成后取消不得撤销manager数据。
#[cfg(windows)]
#[test]
fn HistoryHandoff_PrivateCopyOwnership_006() {
    use crate::version_history::windows::{
        files::{ComponentName, Directory, PrivateDirectory},
        package::RetainedPackage,
        security::CurrentUser,
    };
    use std::ffi::OsStr;
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let transfer = f
        .service
        .reserve_handoff(&f.caller, &ticket.transaction_id)
        .unwrap()
        .transfer
        .unwrap();
    let switch_id = transfer.transaction_id().to_owned();
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = Arc::new(
        PrivateDirectory::create_new(
            Directory::open_absolute(temporary.path()).unwrap(),
            ComponentName::new(OsStr::new("retained")).unwrap(),
            &user,
        )
        .unwrap(),
    );
    let retained = RetainedPackage::retain(&transfer, root).unwrap();
    retained.verify_transfer(&transfer).unwrap();
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(temporary.path().join("retained/official-installer.exe"))
        .is_err());
    assert_eq!(
        std::fs::read(temporary.path().join("retained/official-installer.exe")).unwrap(),
        PAYLOAD
    );
    let retained = transfer.complete(retained).unwrap();
    assert_eq!(retained.record_digest().len(), 64);
    assert_eq!(
        f.service
            .cancel_prepare(&f.caller, &ticket.transaction_id)
            .unwrap_err()
            .code,
        "HISTORY_RECOVERY_REQUIRED"
    );
    *f.clock.lock() += PREPARATION_TTL;
    let repeat = f
        .service
        .reserve_handoff(&f.caller, &ticket.transaction_id)
        .unwrap();
    assert_eq!(repeat.transaction_id, switch_id);
    assert!(repeat.transfer.is_none());
}

// 同名目的文件碰撞只拒绝，不覆盖、不删除原来字节，也不能提交复制能力。
#[cfg(windows)]
#[test]
fn HistoryHandoff_PrivateCopyCollision_007() {
    use crate::version_history::windows::{
        files::{ComponentName, Directory, PrivateDirectory},
        package::RetainedPackage,
        security::CurrentUser,
    };
    use std::ffi::OsStr;
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let transfer = f
        .service
        .reserve_handoff(&f.caller, &ticket.transaction_id)
        .unwrap()
        .transfer
        .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = Arc::new(
        PrivateDirectory::create_new(
            Directory::open_absolute(temporary.path()).unwrap(),
            ComponentName::new(OsStr::new("retained")).unwrap(),
            &user,
        )
        .unwrap(),
    );
    let destination = temporary.path().join("retained/official-installer.exe");
    std::fs::write(&destination, b"existing evidence").unwrap();
    assert!(RetainedPackage::retain(&transfer, root).is_err());
    assert_eq!(std::fs::read(destination).unwrap(), b"existing evidence");
    transfer.check().unwrap();
}

// 发布者验证成功只产生Rust持有的未认证负载身份，不会宣称可安装。
#[test]
fn HistoryDownload_PreparedSummary_008() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    let prepared = f
        .service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    assert!(!prepared.install_ready);
    assert_eq!(prepared.blocked_reason, "PACKAGE_IDENTITY_UNVERIFIED");
    let wire = serde_json::to_string(&prepared).unwrap();
    assert!(!wire.contains("https://"));
    assert!(!wire.contains("path"));
    assert!(!wire.contains("pubkey"));
    f.service
        .with_verified_package(&f.caller, &ticket.transaction_id, |package| {
            assert_eq!(package.bytes(), PAYLOAD);
            assert_eq!(package.selection().version(), "0.17.7");
            assert_eq!(package.sha256(), sha256(PAYLOAD));
            assert!(!package.payload_identity_authenticated());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.service
            .prepare_history(&f.caller, &ticket.transaction_id)
            .unwrap_err()
            .code,
        "HISTORY_PREPARE_ALREADY_STARTED"
    );
}

// 同一文档只保留一个准备事务，另一个文档不能使用或取消该事务。
#[test]
fn HistoryDownload_ExactOwner_009() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    assert_eq!(
        f.service
            .begin_prepare(&f.caller, &f.selection)
            .unwrap_err()
            .code,
        "HISTORY_PREPARE_BUSY"
    );
    for caller in [
        CallerIdentity {
            instance_id: "other".into(),
            ..f.caller.clone()
        },
        CallerIdentity {
            window_label: "manager".into(),
            ..f.caller.clone()
        },
        CallerIdentity {
            webview_epoch: WireU64::parse("2").unwrap(),
            ..f.caller.clone()
        },
    ] {
        assert_eq!(
            f.service
                .prepare_history(&caller, &ticket.transaction_id)
                .unwrap_err()
                .code,
            "HISTORY_PREPARE_UNKNOWN"
        );
        assert_eq!(
            f.service
                .cancel_prepare(&caller, &ticket.transaction_id)
                .unwrap_err()
                .code,
            "HISTORY_PREPARE_UNKNOWN"
        );
    }
}

// 预备完成之前取消会成为终态，重复取消只返回取消状态。
#[test]
fn HistoryDownload_CancelReserved_010() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .cancel_prepare(&f.caller, &ticket.transaction_id)
        .unwrap();
    f.service
        .cancel_prepare(&f.caller, &ticket.transaction_id)
        .unwrap();
    assert_eq!(
        f.service
            .prepare_history(&f.caller, &ticket.transaction_id)
            .unwrap_err()
            .code,
        "HISTORY_PREPARE_CANCELLED"
    );
}

// 取消在网络返回期间获胜时不会晚到发布已验证包。
#[test]
fn HistoryDownload_CancelDuringIo_011() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    let service = f.service.clone();
    let caller = f.caller.clone();
    let transaction = ticket.transaction_id.clone();
    *f.assets.on_open.lock() = Some(Box::new(move || {
        service.cancel_prepare(&caller, &transaction).unwrap();
    }));
    assert_eq!(
        f.service
            .prepare_history(&f.caller, &ticket.transaction_id)
            .unwrap_err()
            .code,
        "HISTORY_PREPARE_CANCELLED"
    );
    assert_eq!(
        f.service
            .with_verified_package(&f.caller, &ticket.transaction_id, |_| Ok(()))
            .unwrap_err()
            .code,
        "HISTORY_PREPARE_CANCELLED"
    );
}

// 已验证包被取消后不能继续交付，也不保留该事务的安装包缓存。
#[test]
fn HistoryDownload_CancelReady_012() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    f.service
        .cancel_prepare(&f.caller, &ticket.transaction_id)
        .unwrap();
    assert_eq!(
        f.service
            .with_verified_package(&f.caller, &ticket.transaction_id, |_| Ok(()))
            .unwrap_err()
            .code,
        "HISTORY_PREPARE_CANCELLED"
    );
    let files = std::fs::read_dir(f._directory.path())
        .unwrap()
        .flat_map(|entry| std::fs::read_dir(entry.unwrap().path()).unwrap())
        .count();
    assert_eq!(
        files, 0,
        "cancelled ready package must remove only its private transaction cache"
    );
}

// 提示大小过大、流超出已绑定大小或短流均拒绝，不能截断后当成成功。
#[test]
fn HistoryDownload_StreamBounds_013() {
    for (bytes, length, expected) in [
        (b"test!".to_vec(), None, "HISTORY_SIZE_MISMATCH"),
        (b"tes".to_vec(), None, "HISTORY_SIZE_MISMATCH"),
        (PAYLOAD.to_vec(), Some(5), "HISTORY_SIZE_MISMATCH"),
    ] {
        let f = fixture(bytes, length);
        let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
        assert_eq!(
            f.service
                .prepare_history(&f.caller, &ticket.transaction_id)
                .unwrap_err()
                .code,
            expected
        );
        assert_eq!(
            f.service
                .with_verified_package(&f.caller, &ticket.transaction_id, |_| Ok(()))
                .unwrap_err()
                .code,
            "HISTORY_PREPARE_FAILED"
        );
    }
}

// 安装包或签名ID、版本、平台名或摘要被替换时必须重新选择。
#[test]
fn HistoryDownload_MetadataChanged_014() {
    for mutation in ["asset", "signature", "version", "platform", "digest"] {
        let f = fixture(PAYLOAD.to_vec(), Some(4));
        let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
        {
            let mut release = f.metadata.release.lock();
            match mutation {
                "asset" => release.assets[0].id += 1,
                "signature" => release.assets[1].id += 1,
                "version" => release.tag_name = "v0.17.6".into(),
                "platform" => release.assets[0].name = "CC.Desk_0.17.7_arm64-setup.exe".into(),
                "digest" => release.assets[0].digest = Some(format!("sha256:{}", "0".repeat(64))),
                _ => unreachable!(),
            }
        }
        assert_eq!(
            f.service
                .prepare_history(&f.caller, &ticket.transaction_id)
                .unwrap_err()
                .code,
            "HISTORY_SELECTION_CHANGED",
            "changed {mutation} must invalidate the original selection"
        );
    }
}

// 下载完成后元数据发生变化也不能交付之前的已验证缓存。
#[test]
fn HistoryDownload_ChangedAtHandoff_015() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    f.metadata.release.lock().assets[0].updated_at = "2026-10-02T06:00:00Z".into();
    assert_eq!(
        f.service
            .with_verified_package(&f.caller, &ticket.transaction_id, |_| Ok(()))
            .unwrap_err()
            .code,
        "HISTORY_SELECTION_CHANGED"
    );
}

// IO跨过原选择期限时不能更新为新选择或发布已验证包。
#[test]
fn HistoryDownload_ExpiryDuringIo_016() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    let clock = f.clock.clone();
    *f.assets.on_open.lock() = Some(Box::new(move || {
        *clock.lock() += SELECTION_TTL;
    }));
    let failure = f
        .service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap_err();
    assert!(
        failure.code == "HISTORY_SELECTION_EXPIRED" || failure.code == "HISTORY_PREPARE_EXPIRED",
        "expired transaction must not publish: {}",
        failure.code
    );
}

// 下载后文档已撤销时返回鉴权失败，不能把取消身份借给新文档。
#[test]
fn HistoryDownload_RevokedDuringIo_017() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    let live = f.live.clone();
    *f.assets.on_open.lock() = Some(Box::new(move || live.store(false, Ordering::SeqCst)));
    assert_eq!(
        f.service
            .prepare_history(&f.caller, &ticket.transaction_id)
            .unwrap_err()
            .code,
        "FORBIDDEN"
    );
}

// 准备事务期限在交付时继续有效，过期后不能因缓存存在而安装。
#[test]
fn HistoryDownload_ExpiredReady_018() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    *f.clock.lock() += PREPARATION_TTL;
    assert_eq!(
        f.service
            .with_verified_package(&f.caller, &ticket.transaction_id, |_| Ok(()))
            .unwrap_err()
            .code,
        "HISTORY_PREPARE_EXPIRED"
    );
}

// 前端附加URL、路径、公钥或构造包字段均不能通过请求DTO。
#[test]
fn HistoryDownload_RejectAuthority_019() {
    use crate::version_history::download::{BeginPrepareRequest, PrepareTransactionRequest};
    assert!(serde_json::from_value::<BeginPrepareRequest>(
        json!({"selectionToken":"a".repeat(32),"url":"https://evil.example/setup.exe"})
    )
    .is_err());
    for field in ["path", "pubkey", "bytes", "owner"] {
        let mut request = json!({"transactionId":"a".repeat(32)});
        request[field] = json!("untrusted");
        assert!(serde_json::from_value::<PrepareTransactionRequest>(request).is_err());
    }
}

// Linux外部同长文件改写不能绕过交付前对保留文件和签名的重复验证。
#[cfg(unix)]
#[test]
fn HistoryDownload_ChangedHeldFile_020() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let session = std::fs::read_dir(f._directory.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(
        session.join(&ticket.transaction_id).join("package.bin"),
        b"Test",
    )
    .unwrap();
    assert_eq!(
        f.service
            .with_verified_package(&f.caller, &ticket.transaction_id, |_| Ok(()))
            .unwrap_err()
            .code,
        "HISTORY_PACKAGE_CHANGED"
    );
}

// Windows准备完成后的同一文件对象允许读共享但禁止写入、删除或替换。
#[cfg(windows)]
#[test]
fn HistoryDownload_WindowsGuard_021() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let session = std::fs::read_dir(f._directory.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let path = session.join(&ticket.transaction_id).join("package.bin");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        PAYLOAD,
        "retained file allows read sharing"
    );
    assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
    assert!(std::fs::remove_file(&path).is_err());
    f.service
        .with_verified_package(&f.caller, &ticket.transaction_id, |_| Ok(()))
        .unwrap();
}

// IO尚未释放时取消不能释放全局或该文档的容量预算。
#[test]
fn HistoryDownload_CancelKeepsBudget_022() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    let service = f.service.clone();
    let caller = f.caller.clone();
    let transaction = ticket.transaction_id.clone();
    let selection = f.selection.clone();
    *f.assets.on_open.lock() = Some(Box::new(move || {
        service.cancel_prepare(&caller, &transaction).unwrap();
        assert_eq!(
            service.begin_prepare(&caller, &selection).unwrap_err().code,
            "HISTORY_PREPARE_BUSY"
        );
    }));
    assert_eq!(
        f.service
            .prepare_history(&f.caller, &ticket.transaction_id)
            .unwrap_err()
            .code,
        "HISTORY_PREPARE_CANCELLED"
    );
    f.service.begin_prepare(&f.caller, &f.selection).unwrap();
}

// 受信注释被更改时必须检查全局签名，不能仅检查正文签名。
#[test]
fn HistoryDownload_TrustedComment_023() {
    let text = String::from_utf8(STANDARD.decode(SIGNATURE).unwrap()).unwrap();
    let signature = STANDARD.encode(text.replace("timestamp:1556193335", "timestamp:1556193336"));
    assert_eq!(
        verify_fixture_payload(PAYLOAD, signature.as_bytes(), KEY, &sha256(PAYLOAD), 4)
            .unwrap_err()
            .code,
        "HISTORY_SIGNATURE_INVALID"
    );
}

// 共享JSON样本必须与生产摘要序列化一致，避免前端臆造已验证或安装资格字段。
#[test]
fn HistoryDownload_WireFixture_024() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    let prepared = f
        .service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let cancelled = f
        .service
        .cancel_prepare(&f.caller, &ticket.transaction_id)
        .unwrap();
    let mut expected: Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/version-history-preparation-wire.json"
    ))
    .unwrap();
    for name in ["ticket", "prepared", "cancelled"] {
        expected[name]["transactionId"] = json!(ticket.transaction_id);
    }
    assert_eq!(
        json!({"ticket": ticket, "prepared": prepared, "cancelled": cancelled}),
        expected
    );
}

// 同一事务下载期间重复执行不得启动第二条下载或再次创建私有文件。
#[test]
fn HistoryDownload_DuplicateDuringIo_025() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    let service = f.service.clone();
    let caller = f.caller.clone();
    let transaction = ticket.transaction_id.clone();
    *f.assets.on_open.lock() = Some(Box::new(move || {
        assert_eq!(
            service
                .prepare_history(&caller, &transaction)
                .unwrap_err()
                .code,
            "HISTORY_PREPARE_ALREADY_STARTED"
        );
    }));
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
}

// 下载在进行时被替换的元数据不能在完成时借用原选择身份。
#[test]
fn HistoryDownload_ReplacedDuringIo_026() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    let metadata = f.metadata.clone();
    *f.assets.on_open.lock() = Some(Box::new(move || metadata.release.lock().assets[0].id += 1));
    assert_eq!(
        f.service
            .prepare_history(&f.caller, &ticket.transaction_id)
            .unwrap_err()
            .code,
        "HISTORY_SELECTION_CHANGED"
    );
}

// 未测量payload拒绝发生在UUID/转移前，原已验证准备仍可查看和取消。
#[test]
fn HistoryHandoff_UnmeasuredPolicyPreservesPreparation_010() {
    use crate::version_history::payload_policy::PayloadAdmission;
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let failure = f
        .service
        .reserve_handoff_admitted(&f.caller, &ticket.transaction_id, PayloadAdmission::admit)
        .err()
        .unwrap();
    assert_eq!(failure.code, "HISTORY_PAYLOAD_UNVERIFIED");
    f.service
        .with_verified_package(&f.caller, &ticket.transaction_id, |package| {
            assert_eq!(package.bytes(), PAYLOAD);
            Ok(())
        })
        .unwrap();
    assert!(f
        .service
        .cancel_prepare(&f.caller, &ticket.transaction_id)
        .is_ok());
}

// 重复命令只返回原UUID，不再次调用实际包准入或交付第二个transfer。
#[test]
fn HistoryHandoff_AdmissionRunsOnlyForWinner_011() {
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let (first, permit) = f
        .service
        .reserve_handoff_admitted(&f.caller, &ticket.transaction_id, |package| {
            assert_eq!(package.bytes(), PAYLOAD);
            Ok("fixture-only admission")
        })
        .unwrap();
    assert_eq!(permit, Some("fixture-only admission"));
    let (duplicate, permit) = f
        .service
        .reserve_handoff_admitted::<()>(&f.caller, &ticket.transaction_id, |_| {
            panic!("duplicate must not re-admit")
        })
        .unwrap();
    assert_eq!(first.transaction_id, duplicate.transaction_id);
    assert!(first.transfer.is_some());
    assert!(duplicate.transfer.is_none());
    assert!(permit.is_none());
}

// 发布者验证只开放review；缺少实际payload/完整coordinator准入仍不能begin。
#[test]
fn HistorySwitchReview_ExplicitActions_001() {
    use crate::version_history::manager::{
        SwitchReviewAction as A, SwitchReviewBlock as B, SwitchReviewPhase as P,
    };
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    let pending = f
        .service
        .inspect_switch(&f.caller, &ticket.transaction_id)
        .unwrap();
    assert_eq!(pending.phase, P::Preparing);
    assert!(!pending.allowed_actions.contains(&A::Review));
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let review = f
        .service
        .inspect_switch(&f.caller, &ticket.transaction_id)
        .unwrap();
    assert_eq!(review.phase, P::Verified);
    assert_eq!(review.block_reason, Some(B::PayloadUnverified));
    assert_eq!(
        review.allowed_actions,
        vec![A::Refresh, A::Review, A::CancelPreparation]
    );
    assert!(review.transaction_id.is_none());
}

// 已发出的UUID不会因取消、过期或消费者丢失而重新变成可安全重试的准备。
#[test]
fn HistorySwitchReview_IssuedOwnershipSurvivesFailure_002() {
    use crate::version_history::manager::{SwitchReviewAction as A, SwitchReviewPhase as P};
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let handoff = f
        .service
        .reserve_handoff(&f.caller, &ticket.transaction_id)
        .unwrap();
    f.service
        .cancel_prepare(&f.caller, &ticket.transaction_id)
        .unwrap();
    drop(handoff.transfer);
    *f.clock.lock() += PREPARATION_TTL;
    let review = f
        .service
        .inspect_switch(&f.caller, &ticket.transaction_id)
        .unwrap();
    assert_eq!(review.phase, P::HandoffIssued);
    assert_eq!(
        review.transaction_id.as_deref(),
        Some(handoff.transaction_id.as_str())
    );
    assert_eq!(review.allowed_actions, vec![A::Refresh]);
}

// 候选UUID只在真实zero-owner准入后发布；busy拒绝保持原准备和取消能力。
#[test]
fn HistorySwitchReview_SourceRefusalBeforeIssue_003() {
    use crate::version_history::{
        maintenance::{AdmissionGate, RuntimeKind},
        manager::{SwitchReviewAction as A, SwitchReviewPhase as P},
    };
    let f = fixture(PAYLOAD.to_vec(), Some(4));
    let ticket = f.service.begin_prepare(&f.caller, &f.selection).unwrap();
    f.service
        .prepare_history(&f.caller, &ticket.transaction_id)
        .unwrap();
    let gate = AdmissionGate::new();
    let child = gate
        .begin_start(RuntimeKind::Native)
        .unwrap()
        .child_created();
    let failure = f
        .service
        .reserve_handoff_with_source(&f.caller, &ticket.transaction_id, |_, transaction| {
            gate.freeze(transaction)
        })
        .err()
        .unwrap();
    assert_eq!(failure.code, "HISTORY_SESSIONS_NOT_QUIESCENT");
    let state = f
        .service
        .inspect_switch(&f.caller, &ticket.transaction_id)
        .unwrap();
    assert_eq!(state.phase, P::Verified);
    assert!(state.transaction_id.is_none());
    assert!(state.allowed_actions.contains(&A::CancelPreparation));
    child.reaped();
}
