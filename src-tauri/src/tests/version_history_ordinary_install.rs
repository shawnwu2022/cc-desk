use super::*;
use crate::cli::{snapshot::CallerIdentity, types::WireU64};
use crate::version_history::catalog::{
    parse_release, CatalogService, CatalogSource, ReleaseMetadata,
};
use crate::version_history::policy::HostPlatform;
use crate::version_history::verified_package::{
    sha256, DownloadedPayload, PrivatePackageStore, PublisherKey,
};
use cap_std::fs::Dir;
use std::io::Write;
use std::sync::Arc;

const BODY: &[u8] = include_bytes!("../../../tests/fixtures/version-history-minisign/payload.bin");
const SIGNATURE: &[u8] =
    include_bytes!("../../../tests/fixtures/version-history-minisign/tauri-signature.sig");
const KEY: &str =
    include_str!("../../../tests/fixtures/version-history-minisign/tauri-public-key.txt");
struct Source(ReleaseMetadata);
impl CatalogSource for Source {
    fn list(&self, _: u16) -> Result<Vec<ReleaseMetadata>, SafeError> {
        Ok(vec![self.0.clone()])
    }
    fn release(&self, _: u64) -> Result<ReleaseMetadata, SafeError> {
        Ok(self.0.clone())
    }
}
fn selection(version: &str, changed_signature: bool) -> SelectionMetadata {
    let mut release = parse_release(include_bytes!(
        "../../../tests/fixtures/version-history-payload/v0.17.7-selection.json"
    ))
    .unwrap();
    release.tag_name = format!("v{version}");
    release.name = Some(format!("CC Desk v{version}"));
    release.html_url = release.html_url.replace("v0.17.7", &release.tag_name);
    for asset in &mut release.assets {
        asset.name = asset.name.replace("0.17.7", version);
        asset.browser_download_url = asset.browser_download_url.replace("0.17.7", version);
        let bytes = if asset.name.ends_with(".sig") {
            SIGNATURE
        } else {
            BODY
        };
        asset.size = bytes.len() as u64;
        asset.digest = Some(format!("sha256:{}", sha256(bytes)));
        if changed_signature && asset.name.ends_with(".sig") {
            asset.id += 1;
        }
    }
    let service = CatalogService::new(Arc::new(Source(release)), HostPlatform::WindowsX64);
    let caller = CallerIdentity {
        instance_id: "ordinary-policy-fixture".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let row = service.list(&caller, None).unwrap().rows.remove(0);
    let selected = service
        .select(&caller, &row.release_id, row.asset_id.as_deref().unwrap())
        .unwrap();
    service
        .resolve_selection(&caller, &selected.selection_token)
        .unwrap()
}

// Public signed fixture bytes prove signature/retained-object checks, not a real installer.
#[test]
fn HistoryOrdinary_SignedIdentityWithoutVersionTable_001() {
    let selected = selection("0.18.3", false);
    let temp = tempfile::tempdir().unwrap();
    let dir = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
    let store = PrivatePackageStore::new(dir).unwrap();
    let storage = store
        .transaction("00000000-0000-4000-8000-000000000001")
        .unwrap();
    let mut file = storage.create_package().unwrap();
    file.write_all(BODY).unwrap();
    let package = VerifiedPackage::finish(
        DownloadedPayload {
            selection: selected.clone(),
            bytes: BODY.to_vec(),
            signature: SIGNATURE.to_vec(),
        },
        Arc::new(PublisherKey::fixture(KEY).unwrap()),
        storage,
        file,
        &|| Ok(()),
    )
    .unwrap();
    let admission = OrdinaryInstallAdmission::admit(&package).unwrap();
    admission.verify_selection(&selected).unwrap();
    assert_eq!(admission.policy_digest().len(), 64);
    assert!(!package.payload_identity_authenticated());
}

#[test]
fn HistoryOrdinary_ExactSelectionAndSignatureAsset_002() {
    let selected = selection("0.18.3", false);
    let admission = OrdinaryInstallAdmission::from_selection(&selected).unwrap();
    for other in [selection("0.18.2", false), selection("0.18.3", true)] {
        assert_eq!(
            admission.verify_selection(&other).unwrap_err().code,
            "HISTORY_TARGET_CHANGED"
        );
        assert_ne!(
            admission.policy_digest(),
            OrdinaryInstallAdmission::from_selection(&other)
                .unwrap()
                .policy_digest()
        );
    }
}
