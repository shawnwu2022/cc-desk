//! Compiled, test-only evidence identities. Never a production admission policy.
use super::{blocked, bounded_read, safe};
use crate::cli::{profiles::error, types::SafeError};
use crate::version_history::{
    catalog::{parse_catalog_page, parse_release, AssetMetadata, ReleaseMetadata},
    verified_package::{sha256, VerifiedPackage},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{io, path::Path};

// The runtime catalog and environment constructors below are exercised by these
// pure tests; no installer, token manipulation, registry or user data is used.
#[test]
fn HistoryPayload_FixtureCatalog_020() {
    for version in VERSIONS {
        let fixture = load(version).unwrap();
        assert_eq!(fixture.version, version);
        assert_eq!(fixture.selection.assets.len(), 2);
        fixture.check_release(&fixture.selection).unwrap();
    }
    for version in ["", "v0.17.7", "0.17.8", "0.18.0", "../0.17.7"] {
        assert!(load(version).is_err());
    }
}

#[test]
fn HistoryPayload_FixtureMutation_021() {
    let expected = load("0.14.0").unwrap();
    for mutation in 0..15 {
        let mut bad = expected.clone();
        match mutation {
            0 => bad.version = "0.17.7".into(),
            1 => bad.selection.assets[1] = load("0.17.7").unwrap().signature().clone(),
            2 => bad.selection.assets[0].id += 1,
            3 => bad.selection.assets[1].digest = Some(format!("sha256:{}", "0".repeat(64))),
            4 => bad.selection.assets[0].size += 1,
            5 => bad.selection.assets[1].updated_at = "2026-10-04T00:00:00Z".into(),
            6 => bad.selection.assets[0].name = "other.exe".into(),
            7 => bad.selection.assets[0]
                .browser_download_url
                .push_str("?other"),
            8 => bad.selection.assets[1] = bad.selection.assets[0].clone(),
            9 => bad.provenance.source_commit = "0".repeat(40),
            10 => bad.provenance.sources[0].sha256 = "0".repeat(64),
            11 => bad.provenance.sources[0].source.push_str("?other"),
            12 => bad.provenance.windows_override_absent = false,
            13 => bad.provenance.tauri_cli.version = "2.10.2".into(),
            _ => bad.provenance.sources.swap(0, 1),
        }
        assert!(bad.validate().is_err(), "mutation {mutation} accepted");
    }
    let mut value: Value = serde_json::from_str(CATALOG).unwrap();
    value["fixtures"][0]["unexpected"] = json!(true);
    assert!(serde_json::from_value::<Catalog>(value).is_err());
}

#[test]
fn HistoryPayload_Binding_022() {
    let source = "a".repeat(40);
    for version in VERSIONS {
        for case in ["clean", "seeded-existing"] {
            let binding = Binding::new(version, case, &source, &source, "123", "1").unwrap();
            assert_eq!(binding.record.fixture_version, version);
            assert_eq!(binding.record.fixture_case, case);
        }
    }
    for case in ["", "seeded", "CLEAN", "clean/../seeded-existing"] {
        assert!(Binding::new("0.17.7", case, &source, &source, "1", "1").is_err());
    }
    for invalid in ["local", "", &"A".repeat(40), &"0".repeat(40)] {
        assert!(Binding::new("0.17.7", "clean", invalid, invalid, "1", "1").is_err());
    }
    assert!(Binding::new("0.17.7", "clean", &source, &"b".repeat(40), "1", "1").is_err());
    for invalid in ["", "0", "01", "-1", "1/2", "18446744073709551616"] {
        assert!(Binding::new("0.17.7", "clean", &source, &source, invalid, "1").is_err());
        assert!(Binding::new("0.17.7", "clean", &source, &source, "1", invalid).is_err());
    }
}

#[test]
fn HistoryPayload_BindingRecord_023() {
    let source = "a".repeat(40);
    let binding = Binding::new("0.17.7", "clean", &source, &source, "123", "1").unwrap();
    binding
        .check_record(&serde_json::to_vec(&binding.record).unwrap())
        .unwrap();
    for (version, case, run, attempt) in [
        ("0.17.6", "clean", "123", "1"),
        ("0.17.7", "seeded-existing", "123", "1"),
        ("0.17.7", "clean", "124", "1"),
        ("0.17.7", "clean", "123", "2"),
    ] {
        let other = Binding::new(version, case, &source, &source, run, attempt).unwrap();
        assert!(binding
            .check_record(&serde_json::to_vec(&other.record).unwrap())
            .is_err());
    }
    let mut other = serde_json::to_value(&binding.record).unwrap();
    other["fixtureFingerprint"] = json!("0".repeat(64));
    assert!(binding
        .check_record(&serde_json::to_vec(&other).unwrap())
        .is_err());
    assert!(binding.check_record(b"{}").is_err());
}

const CATALOG: &str =
    include_str!("../../../../tests/fixtures/version-history-payload/catalog.json");
const RELEASES: &[u8] = include_bytes!("../../../../tests/fixtures/version-history-releases.json");
const HOOKS: &[u8] =
    include_bytes!("../../../../tests/fixtures/version-history-payload/v0.17.7-installer.nsh");
const WINDOWS_CONFIG: &[u8] = include_bytes!(
    "../../../../tests/fixtures/version-history-payload/v0.17.7-tauri.windows.conf.json"
);
const NSIS_TEMPLATE: &[u8] =
    include_bytes!("../../../../tests/fixtures/version-history-payload/tauri-2.10.1-installer.nsi");
const VERSIONS: [&str; 9] = [
    "0.14.0", "0.15.0", "0.16.0", "0.17.0", "0.17.1", "0.17.2", "0.17.5", "0.17.6", "0.17.7",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Catalog {
    schema: u32,
    release_catalog_sha256: String,
    fixtures: Vec<Fixture>,
    nsis_template: TemplateSource,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateSource {
    file: String,
    sha256: String,
    source: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Fixture {
    pub(super) version: String,
    pub(super) selection: ReleaseMetadata,
    pub(super) provenance: Provenance,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Provenance {
    pub(super) source_commit: String,
    windows_override_absent: bool,
    windows_override_source: String,
    tauri_cli: TauriCli,
    sources: Vec<Source>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TauriCli {
    version: String,
    package_lock_sha256: String,
    source: String,
}
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Source {
    file: String,
    path: String,
    sha256: String,
    source: String,
}
fn source(commit: &str, file: &str, path: &str, bytes: &[u8]) -> Source {
    Source {
        file: file.into(),
        path: path.into(),
        sha256: sha256(bytes),
        source: format!("https://github.com/shawnwu2022/cc-desk/blob/{commit}/src-tauri/{path}"),
    }
}

fn snapshot(version: &str) -> io::Result<(&'static str, &'static str, &'static [u8])> {
    match version {
        "0.14.0" => Ok((
            "ee62c337f5ef9d98144f5c01ad9c3676f2622fab",
            "8faab7f4ad639a2866d87fa61ef05a1b19b2171bac6a8ed5a9c896bc4804ba78",
            include_bytes!(
                "../../../../tests/fixtures/version-history-payload/v0.14.0-tauri.conf.json"
            ),
        )),
        "0.15.0" => Ok((
            "e0036735a1dcb5b5297c9f528518bca6dc22501b",
            "0ba10a475e2294d2da8345da5ca2238566f1375ce1b391a519c312f1d9d15dcf",
            include_bytes!(
                "../../../../tests/fixtures/version-history-payload/v0.15.0-tauri.conf.json"
            ),
        )),
        "0.16.0" => Ok((
            "4e7b3e9cddfe219b198e9b3a3d9541bc2fb30515",
            "d906148186a3dcff454dfe9281178a3c2ddce041ff830d37d43560906241ba28",
            include_bytes!(
                "../../../../tests/fixtures/version-history-payload/v0.16.0-tauri.conf.json"
            ),
        )),
        "0.17.0" => Ok((
            "af068629c79095b9dc7d1935a9b6397975e98ae0",
            "64252fa5f7a377c64a1a4fe21eb41360e060364aebb56abd0afe18f747d94bfe",
            include_bytes!(
                "../../../../tests/fixtures/version-history-payload/v0.17.0-tauri.conf.json"
            ),
        )),
        "0.17.1" => Ok((
            "83ee6e03d62a425f375c43841b35ae56fa7fbba6",
            "0f9dc39d01f20c058863f04e6d73e1b6e66aaefac1b7c5cb01ccdf22946c0ca9",
            include_bytes!(
                "../../../../tests/fixtures/version-history-payload/v0.17.1-tauri.conf.json"
            ),
        )),
        "0.17.2" => Ok((
            "a3776147a89703a2880e734b951d071c29afe68c",
            "2af29bfb62b37479a4f3e9abe2fc175c5b04ce150b9bd2d943d7012cc1a7fbcf",
            include_bytes!(
                "../../../../tests/fixtures/version-history-payload/v0.17.2-tauri.conf.json"
            ),
        )),
        "0.17.5" => Ok((
            "b4f7658c847a070cf64e9dba978c4e325a09bdce",
            "d77051aab63bdbcba45b00679d9d71448b6f287464013aa4b26bc000e7c04617",
            include_bytes!(
                "../../../../tests/fixtures/version-history-payload/v0.17.5-tauri.conf.json"
            ),
        )),
        "0.17.6" => Ok((
            "e67fcd87cc282ad1de7f3de693b8693485b9abf5",
            "a93d1aceec4cd47574fd1452c7949f9fcd9c7e324fc25ac2d3d35d6ce75151b4",
            include_bytes!(
                "../../../../tests/fixtures/version-history-payload/v0.17.6-tauri.conf.json"
            ),
        )),
        "0.17.7" => Ok((
            "77707e3b03187aa2ed96f5ab780f62f14c1e4ffc",
            "30572dfd43cff9a9c5f468394e96e5fc838f01ad5eb7eba270e78a5127f4ab9a",
            include_bytes!(
                "../../../../tests/fixtures/version-history-payload/v0.17.7-tauri.conf.json"
            ),
        )),
        _ => Err(blocked("unknown fixed payload version")),
    }
}

pub(super) fn catalog_bytes() -> &'static [u8] {
    CATALOG.as_bytes()
}

pub(super) fn load(version: &str) -> io::Result<Fixture> {
    let catalog: Catalog = serde_json::from_str(CATALOG)?;
    if catalog.nsis_template.file != "tauri-2.10.1-installer.nsi"
        || catalog.nsis_template.sha256 != sha256(NSIS_TEMPLATE)
        || catalog.nsis_template.source != "https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.10.1/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi"
        || catalog.schema != 1
        || catalog.release_catalog_sha256 != sha256(RELEASES)
        || catalog
            .fixtures
            .iter()
            .map(|f| f.version.as_str())
            .collect::<Vec<_>>()
            != VERSIONS
    {
        return Err(blocked(
            "compiled evidence catalog differs from its fixed release source",
        ));
    }
    // Validate all nine entries so malformed/mixed compiled fixtures never become
    // an alternative runtime input or an incomplete source catalog.
    for fixture in &catalog.fixtures {
        fixture.validate()?;
    }
    catalog
        .fixtures
        .into_iter()
        .find(|f| f.version == version)
        .ok_or_else(|| blocked("unknown fixed payload version"))
}
impl Fixture {
    fn validate(&self) -> io::Result<()> {
        let (commit, lock_hash, config_bytes) = snapshot(&self.version)?;
        let config: Value = serde_json::from_slice(config_bytes)?;
        let mut expected = safe(parse_catalog_page(RELEASES))?
            .into_iter()
            .find(|r| r.tag_name == format!("v{}", self.version))
            .ok_or_else(|| blocked("missing fixed release tuple"))?;
        let installer_name = format!("CC.Desk_{}_x64-setup.exe", self.version);
        let signature_name = format!("{installer_name}.sig");
        let assets = [&installer_name, &signature_name]
            .into_iter()
            .map(|name| {
                expected
                    .assets
                    .iter()
                    .find(|a| &a.name == name)
                    .cloned()
                    .ok_or_else(|| blocked("missing exact official asset"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        expected.assets = assets;
        let mut sources = vec![
            source(commit, "v0.17.7-installer.nsh", "installer.nsh", HOOKS),
            source(
                commit,
                &format!("v{}-tauri.conf.json", self.version),
                "tauri.conf.json",
                config_bytes,
            ),
        ];
        if self.has_conpty() {
            sources.push(source(
                commit,
                "v0.17.7-tauri.windows.conf.json",
                "tauri.windows.conf.json",
                WINDOWS_CONFIG,
            ));
        }
        if self.selection != expected
            || safe(parse_release(&serde_json::to_vec(&self.selection)?))? != expected
            || self.provenance.source_commit != commit
            || self.provenance.sources != sources
            || self.provenance.windows_override_absent == self.has_conpty()
            || self.provenance.windows_override_source
                != format!("https://github.com/shawnwu2022/cc-desk/tree/{commit}/src-tauri")
            || self.provenance.tauri_cli.version != "2.10.1"
            || self.provenance.tauri_cli.package_lock_sha256 != lock_hash
            || self.provenance.tauri_cli.source
                != format!("https://github.com/shawnwu2022/cc-desk/blob/{commit}/package-lock.json")
            || config["version"] != self.version
            || config["productName"] != "CC Desk"
        {
            return Err(blocked("fixed fixture tuple or source provenance differs"));
        }
        for asset in &self.selection.assets {
            if asset.state != "uploaded"
                || asset.size == 0
                || asset.size > 256 * 1024 * 1024
                || !asset
                    .digest
                    .as_deref()
                    .and_then(|d| d.strip_prefix("sha256:"))
                    .is_some_and(|h| hex(h, 64))
            {
                return Err(blocked("invalid fixed asset digest or size"));
            }
        }
        if self.signature().size != 420 {
            return Err(blocked("unsupported fixed signature size"));
        }
        Ok(())
    }
    pub(super) fn has_conpty(&self) -> bool {
        self.version == "0.17.7"
    }
    pub(super) fn installer(&self) -> &AssetMetadata {
        &self.selection.assets[0]
    }
    pub(super) fn signature(&self) -> &AssetMetadata {
        &self.selection.assets[1]
    }
    pub(super) fn installer_hash(&self) -> &str {
        self.installer()
            .digest
            .as_deref()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap()
    }
    pub(super) fn signature_hash(&self) -> &str {
        self.signature()
            .digest
            .as_deref()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap()
    }
    pub(super) fn check_release(&self, actual: &ReleaseMetadata) -> io::Result<()> {
        let mut selected = actual.clone();
        selected
            .assets
            .retain(|a| [self.installer().id, self.signature().id].contains(&a.id));
        selected.assets.sort_by_key(|a| a.id);
        let mut expected = self.selection.clone();
        expected.assets.sort_by_key(|a| a.id);
        if selected != expected {
            return Err(blocked(
                "official release or full selected asset tuple changed",
            ));
        }
        Ok(())
    }
    pub(super) fn check_package(&self, package: &VerifiedPackage) -> Result<(), SafeError> {
        let selected = package.selection();
        if package.sha256() != self.installer_hash()
            || package.size() != self.installer().size
            || selected.release_id() != self.selection.id
            || selected.version() != self.version
            || selected.tag() != self.selection.tag_name
            || selected.installer().id() != self.installer().id
            || selected.installer().name() != self.installer().name
            || selected.installer().sha256() != self.installer_hash()
            || selected.signature().id() != self.signature().id
            || selected.signature().name() != self.signature().name
            || selected.signature().sha256() != self.signature_hash()
            || selected.signature().size() != self.signature().size
            || package.payload_identity_authenticated()
        {
            return Err(error("HISTORY_FIXTURE_SELECTION_CHANGED"));
        }
        Ok(())
    }
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && value.bytes().any(|b| b != b'0')
}
fn positive_id(value: &str) -> bool {
    value
        .parse::<u64>()
        .is_ok_and(|n| n != 0 && n.to_string() == value)
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BindingRecord {
    schema: u32,
    fixture_version: String,
    pub(super) fixture_case: String,
    fixture_source_commit: String,
    compiled_source_commit: String,
    release_source_commit: String,
    fixture_catalog_sha256: String,
    release_catalog_sha256: String,
    fixture_fingerprint: String,
    run_id: String,
    run_attempt: String,
}
pub(super) struct Binding {
    pub(super) fixture: Fixture,
    pub(super) record: BindingRecord,
}
impl Binding {
    fn new(
        version: &str,
        case: &str,
        source: &str,
        compiled: &str,
        run: &str,
        attempt: &str,
    ) -> io::Result<Self> {
        if !matches!(case, "clean" | "seeded-existing")
            || !hex(source, 40)
            || source != compiled
            || !positive_id(run)
            || !positive_id(attempt)
        {
            return Err(blocked(
                "unknown or mismatched version/case/source/run binding",
            ));
        }
        let fixture = load(version)?;
        let record = BindingRecord {
            schema: 1,
            fixture_version: version.into(),
            fixture_case: case.into(),
            fixture_source_commit: source.into(),
            compiled_source_commit: compiled.into(),
            release_source_commit: fixture.provenance.source_commit.clone(),
            fixture_catalog_sha256: sha256(CATALOG.as_bytes()),
            release_catalog_sha256: sha256(RELEASES),
            fixture_fingerprint: sha256(&serde_json::to_vec(&fixture)?),
            run_id: run.into(),
            run_attempt: attempt.into(),
        };
        Ok(Self { fixture, record })
    }
    pub(super) fn from_environment() -> io::Result<Self> {
        let value =
            |name| std::env::var(name).map_err(|_| blocked("missing fixed fixture binding"));
        Self::new(
            &value("CC_DESK_PAYLOAD_VERSION")?,
            &value("CC_DESK_PAYLOAD_CASE")?,
            &value("CC_DESK_PAYLOAD_SOURCE_SHA")?,
            env!("CC_DESK_BUILD_SHA"),
            &value("GITHUB_RUN_ID")?,
            &value("GITHUB_RUN_ATTEMPT")?,
        )
    }
    pub(super) fn root_name(&self) -> String {
        format!(
            "ccdesk-v{}-{}-{}-{}-payload-evidence",
            self.record.fixture_version,
            self.record.fixture_case,
            self.record.run_id,
            self.record.run_attempt
        )
    }
    fn check_record(&self, bytes: &[u8]) -> io::Result<()> {
        let record: BindingRecord = serde_json::from_slice(bytes)?;
        if record != self.record {
            return Err(blocked("controller/worker fixture binding changed"));
        }
        Ok(())
    }
    pub(super) fn verify_record(&self, root: &Path) -> io::Result<()> {
        self.check_record(&bounded_read(&root.join("fixture-binding.json"), 65536)?)?;
        if Self::from_environment()?.record != self.record {
            return Err(blocked("frozen fixture environment binding changed"));
        }
        Ok(())
    }
}
