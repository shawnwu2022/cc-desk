//! Bounded official-repository metadata reads and document-owned selections.
//! This module cannot download package bytes, install, or authorize shared data.
use super::policy::{historical, version, HostPlatform, PRODUCT_IDENTIFIER};
use super::types::*;
use crate::cli::profiles::error;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::SafeError;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) const CATALOG_PAGE_SIZE: usize = 25;
pub(crate) const MAX_CATALOG_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_CATALOG_PAGES: u16 = 10;
pub(crate) const SELECTION_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_ASSETS: usize = 32;
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: u64 = 16 * 1024;
const MAX_OBSERVATIONS: usize = 1024;
const MAX_OWNER_OBSERVATIONS: usize = 256;
const MAX_CURSORS: usize = 64;
const MAX_OWNER_CURSORS: usize = 16;
const MAX_SELECTIONS: usize = 64;
const MAX_OWNER_SELECTIONS: usize = 16;
const API_ROOT: &str = "https://api.github.com/repos/shawnwu2022/cc-desk/releases";
const PUBLIC_ROOT: &str = "https://github.com/shawnwu2022/cc-desk/releases";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct AssetMetadata {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) state: String,
    pub(crate) size: u64,
    pub(crate) digest: Option<String>,
    pub(crate) browser_download_url: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct ReleaseMetadata {
    pub(crate) id: u64,
    pub(crate) tag_name: String,
    pub(crate) name: Option<String>,
    pub(crate) draft: bool,
    pub(crate) prerelease: bool,
    pub(crate) published_at: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) html_url: String,
    pub(crate) assets: Vec<AssetMetadata>,
}
/// No Deserialize/Serialize. Only an admitted catalogue selection can create it.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct BoundAsset {
    id: u64,
    name: String,
    size: u64,
    sha256: String,
    download_url: String,
    created_at: String,
    updated_at: String,
}
impl std::fmt::Debug for BoundAsset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoundAsset(<held>)")
    }
}
impl BoundAsset {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn size(&self) -> u64 {
        self.size
    }
    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }
    pub(crate) fn download_url(&self) -> &str {
        &self.download_url
    }
}
/// Authenticates selection metadata only, never the publisher or downloaded bytes.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SelectionMetadata {
    release_id: u64,
    tag: String,
    version: String,
    published_at: String,
    created_at: String,
    updated_at: String,
    installer: BoundAsset,
    signature: BoundAsset,
}
impl std::fmt::Debug for SelectionMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SelectionMetadata(<held; unverified bytes>)")
    }
}
impl SelectionMetadata {
    pub(crate) fn release_id(&self) -> u64 {
        self.release_id
    }
    pub(crate) fn tag(&self) -> &str {
        &self.tag
    }
    pub(crate) fn version(&self) -> &str {
        &self.version
    }
    pub(crate) fn product_identifier(&self) -> &'static str {
        PRODUCT_IDENTIFIER
    }
    pub(crate) fn installer(&self) -> &BoundAsset {
        &self.installer
    }
    pub(crate) fn signature(&self) -> &BoundAsset {
        &self.signature
    }
}

fn timestamp(value: &str) -> bool {
    value.len() <= 40 && chrono::DateTime::parse_from_rfc3339(value).is_ok()
}
fn bounded(bytes: &[u8]) -> Result<(), SafeError> {
    if bytes.len() > MAX_CATALOG_BYTES {
        Err(error("HISTORY_METADATA_TOO_LARGE"))
    } else {
        Ok(())
    }
}
fn validate_release(value: &ReleaseMetadata) -> Result<(), SafeError> {
    if value.id == 0
        || value.tag_name.len() > 128
        || value.html_url.len() > 512
        || value
            .name
            .as_ref()
            .is_some_and(|name| name.len() > 256 || name.chars().any(char::is_control))
        || value.assets.len() > MAX_ASSETS
        || !timestamp(&value.created_at)
        || !timestamp(&value.updated_at)
        || value
            .published_at
            .as_ref()
            .is_some_and(|date| !timestamp(date))
    {
        return Err(error("HISTORY_METADATA_INVALID"));
    }
    let mut ids = HashSet::new();
    for asset in &value.assets {
        if asset.id == 0
            || !ids.insert(asset.id)
            || asset.name.len() > 256
            || asset.state.len() > 32
            || asset.browser_download_url.len() > 1024
            || asset
                .digest
                .as_ref()
                .is_some_and(|digest| digest.len() > 128)
            || !timestamp(&asset.created_at)
            || !timestamp(&asset.updated_at)
        {
            return Err(error("HISTORY_METADATA_INVALID"));
        }
    }
    Ok(())
}
fn public_release(value: &ReleaseMetadata) -> bool {
    if version(&value.tag_name).is_none() {
        return false;
    }
    if value.draft
        || value.prerelease
        || value.published_at.is_none()
        || value.html_url != format!("{PUBLIC_ROOT}/tag/{}", value.tag_name)
    {
        return false;
    }
    // A workflow/test artifact re-labelled with a stable tag does not become public history.
    // Only the canonical tag is projected; release title is never displayed.
    !value
        .name
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|word| {
            [
                "candidate",
                "test",
                "testing",
                "preview",
                "nightly",
                "workflow",
                "artifact",
                "snapshot",
            ]
            .contains(&word)
        })
}
pub(crate) fn parse_catalog_page(bytes: &[u8]) -> Result<Vec<ReleaseMetadata>, SafeError> {
    bounded(bytes)?;
    let values: Vec<ReleaseMetadata> =
        serde_json::from_slice(bytes).map_err(|_| error("HISTORY_METADATA_INVALID"))?;
    if values.len() > CATALOG_PAGE_SIZE {
        return Err(error("HISTORY_METADATA_INVALID"));
    }
    let mut ids = HashSet::new();
    for value in &values {
        validate_release(value)?;
        if !ids.insert(value.id) {
            return Err(error("HISTORY_METADATA_INVALID"));
        }
    }
    Ok(values)
}
pub(crate) fn parse_release(bytes: &[u8]) -> Result<ReleaseMetadata, SafeError> {
    bounded(bytes)?;
    let value = serde_json::from_slice(bytes).map_err(|_| error("HISTORY_METADATA_INVALID"))?;
    validate_release(&value)?;
    Ok(value)
}
fn digest(value: &Option<String>) -> Option<&str> {
    let value = value.as_ref()?.strip_prefix("sha256:")?;
    (value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(value)
}
fn bind_asset(
    asset: &AssetMetadata,
    tag: &str,
    limit: u64,
) -> Result<BoundAsset, HistoryBlockReason> {
    if asset.state != "uploaded"
        || asset.size == 0
        || asset.size > limit
        || asset.browser_download_url != format!("{PUBLIC_ROOT}/download/{tag}/{}", asset.name)
    {
        return Err(HistoryBlockReason::AssetMetadataInvalid);
    }
    let sha256 = digest(&asset.digest).ok_or(HistoryBlockReason::DigestUnavailable)?;
    Ok(BoundAsset {
        id: asset.id,
        name: asset.name.clone(),
        size: asset.size,
        sha256: sha256.to_owned(),
        download_url: asset.browser_download_url.clone(),
        created_at: asset.created_at.clone(),
        updated_at: asset.updated_at.clone(),
    })
}
impl ReleaseMetadata {
    fn bind(&self, host: HostPlatform) -> Result<SelectionMetadata, HistoryBlockReason> {
        let value = version(&self.tag_name).ok_or(HistoryBlockReason::AssetMetadataInvalid)?;
        if !public_release(self) {
            return Err(HistoryBlockReason::AssetMetadataInvalid);
        }
        if !historical(value) {
            return Err(HistoryBlockReason::ReleaseNotHistorical);
        }
        if host != HostPlatform::WindowsX64 {
            return Err(HistoryBlockReason::PlatformUnsupported);
        }
        let filename = format!("CC.Desk_{value}_x64-setup.exe");
        let installers: Vec<_> = self
            .assets
            .iter()
            .filter(|asset| asset.name == filename)
            .collect();
        if installers.is_empty() {
            return Err(
                if self.assets.iter().any(|asset| asset.name.ends_with(".msi")) {
                    HistoryBlockReason::PackageFormatUnsupported
                } else {
                    HistoryBlockReason::PlatformAssetMissing
                },
            );
        }
        if installers.len() != 1 {
            return Err(HistoryBlockReason::AssetAmbiguous);
        }
        let signature_name = format!("{filename}.sig");
        let signatures: Vec<_> = self
            .assets
            .iter()
            .filter(|asset| asset.name == signature_name)
            .collect();
        if signatures.is_empty() {
            return Err(HistoryBlockReason::SignatureMissing);
        }
        if signatures.len() != 1 {
            return Err(HistoryBlockReason::AssetAmbiguous);
        }
        Ok(SelectionMetadata {
            release_id: self.id,
            tag: self.tag_name.clone(),
            version: value.to_owned(),
            published_at: self
                .published_at
                .clone()
                .expect("public release has a timestamp"),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            installer: bind_asset(installers[0], &self.tag_name, MAX_PACKAGE_BYTES)?,
            signature: bind_asset(signatures[0], &self.tag_name, MAX_SIGNATURE_BYTES)?,
        })
    }
    pub(crate) fn project(&self, host: HostPlatform, observation_id: String) -> HistoryRelease {
        let value = version(&self.tag_name).expect("only canonical public releases are projected");
        let binding = self.bind(host);
        let mut available_platforms = Vec::new();
        for (platform, name) in [
            (
                HistoryPlatform::WindowsX86_64,
                format!("CC.Desk_{value}_x64-setup.exe"),
            ),
            (
                HistoryPlatform::DarwinAarch64,
                "CC.Desk.app.tar.gz".to_owned(),
            ),
            (
                HistoryPlatform::LinuxX86_64,
                format!("CC.Desk_{value}_amd64.AppImage"),
            ),
        ] {
            if self
                .assets
                .iter()
                .any(|asset| asset.name == name && asset.state == "uploaded" && asset.size > 0)
            {
                available_platforms.push(platform);
            }
        }
        let selectable = binding.is_ok();
        HistoryRelease {
            release_id: observation_id,
            asset_id: binding
                .as_ref()
                .ok()
                .map(|selection| selection.installer.id.to_string()),
            version: value.to_owned(),
            published_at: self.published_at.clone().expect("public timestamp"),
            platform: host.wire(),
            available_platforms,
            package_format: selectable.then_some(HistoryPackageFormat::Nsis),
            verification: HistoryVerification::AwaitingVerification,
            select_allowed: selectable,
            install_ready: false,
            data_modes: HistoryDataModes {
                fresh_settings: if selectable {
                    HistoryDataMode::Available
                } else {
                    HistoryDataMode::Unavailable
                },
                keep_current_data: HistoryDataMode::Unavailable,
            },
            blocked_reason: binding.err(),
        }
    }
}

pub(crate) fn classify_http_status(status: u16) -> Result<(), SafeError> {
    match status {
        200 => Ok(()),
        403 | 429 => Err(error("HISTORY_RATE_LIMITED")),
        404 => Err(error("HISTORY_RELEASE_UNAVAILABLE")),
        _ => Err(error("HISTORY_NETWORK_UNAVAILABLE")),
    }
}
/// Only the external HTTP boundary is replaceable in deterministic policy tests.
pub(crate) trait CatalogSource: Send + Sync {
    fn list(&self, page: u16) -> Result<Vec<ReleaseMetadata>, SafeError>;
    fn release(&self, release_id: u64) -> Result<ReleaseMetadata, SafeError>;
}
pub(crate) struct OfficialGitHub {
    client: reqwest::blocking::Client,
}
impl OfficialGitHub {
    pub(crate) fn new() -> Result<Self, SafeError> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("CC-Desk-Historical-Catalog")
            .build()
            .map_err(|_| error("HISTORY_NETWORK_UNAVAILABLE"))?;
        Ok(Self { client })
    }
    fn get(&self, url: &str) -> Result<Vec<u8>, SafeError> {
        let response = self
            .client
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .map_err(|_| error("HISTORY_NETWORK_UNAVAILABLE"))?;
        classify_http_status(response.status().as_u16())?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_CATALOG_BYTES as u64)
        {
            return Err(error("HISTORY_METADATA_TOO_LARGE"));
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_CATALOG_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| error("HISTORY_NETWORK_UNAVAILABLE"))?;
        bounded(&bytes)?;
        Ok(bytes)
    }
}
impl CatalogSource for OfficialGitHub {
    fn list(&self, page: u16) -> Result<Vec<ReleaseMetadata>, SafeError> {
        if page == 0 || page > MAX_CATALOG_PAGES {
            return Err(error("HISTORY_CURSOR_INVALID"));
        }
        parse_catalog_page(&self.get(&format!(
            "{API_ROOT}?per_page={CATALOG_PAGE_SIZE}&page={page}"
        ))?)
    }
    fn release(&self, release_id: u64) -> Result<ReleaseMetadata, SafeError> {
        if release_id == 0 {
            return Err(error("HISTORY_SELECTION_UNKNOWN"));
        }
        parse_release(&self.get(&format!("{API_ROOT}/{release_id}"))?)
    }
}
struct Observation {
    caller: CallerIdentity,
    expires: Instant,
    release: ReleaseMetadata,
}
struct Cursor {
    caller: CallerIdentity,
    expires: Instant,
    page: u16,
    seen: HashSet<u64>,
}
struct Selection {
    caller: CallerIdentity,
    expires: Instant,
    metadata: SelectionMetadata,
    release: ReleaseMetadata,
}
/// Private transfer material, never an IPC response or a live capability. The
/// full observed release is retained because unrelated asset/timestamp changes
/// must still invalidate the original selection after the source process exits.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedSelectionObservation {
    schema: u32,
    release: ReleaseMetadata,
    installer_id: u64,
    signature_id: u64,
}
#[derive(Default)]
struct Held {
    observations: HashMap<String, Observation>,
    cursors: HashMap<String, Cursor>,
    selections: HashMap<String, Selection>,
}
impl Held {
    fn prune(&mut self, now: Instant) {
        self.observations.retain(|_, item| now < item.expires);
        self.cursors.retain(|_, item| now < item.expires);
        self.selections.retain(|_, item| now < item.expires);
    }
}
pub(crate) struct CatalogService {
    source: Arc<dyn CatalogSource>,
    host: HostPlatform,
    held: Mutex<Held>,
    clock: Arc<dyn Fn() -> Instant + Send + Sync>,
}
impl CatalogService {
    pub(crate) fn new(source: Arc<dyn CatalogSource>, host: HostPlatform) -> Self {
        Self::with_clock(source, host, Arc::new(Instant::now))
    }
    pub(crate) fn with_clock(
        source: Arc<dyn CatalogSource>,
        host: HostPlatform,
        clock: Arc<dyn Fn() -> Instant + Send + Sync>,
    ) -> Self {
        Self {
            source,
            host,
            held: Mutex::new(Held::default()),
            clock,
        }
    }
    pub(crate) fn production() -> Result<Self, SafeError> {
        Ok(Self::new(
            Arc::new(OfficialGitHub::new()?),
            HostPlatform::current(),
        ))
    }
    pub(crate) fn list(
        &self,
        caller: &CallerIdentity,
        cursor: Option<&str>,
    ) -> Result<HistoryCatalogPage, SafeError> {
        self.list_at(caller, cursor, (self.clock)())
    }
    pub(crate) fn list_at(
        &self,
        caller: &CallerIdentity,
        cursor: Option<&str>,
        now: Instant,
    ) -> Result<HistoryCatalogPage, SafeError> {
        let (page, mut seen, cursor_expiry) = if let Some(token) = cursor {
            if !opaque(token) {
                return Err(error("HISTORY_CURSOR_INVALID"));
            }
            let held = self.held.lock();
            let cursor = held
                .cursors
                .get(token)
                .filter(|item| &item.caller == caller)
                .ok_or_else(|| error("HISTORY_CURSOR_INVALID"))?;
            if now >= cursor.expires {
                return Err(error("HISTORY_CURSOR_EXPIRED"));
            }
            (cursor.page, cursor.seen.clone(), Some(cursor.expires))
        } else {
            (1, HashSet::new(), None)
        };
        let releases = self.source.list(page)?;
        let completed = now.max((self.clock)());
        if cursor_expiry.is_some_and(|expires| completed >= expires) {
            return Err(error("HISTORY_CURSOR_EXPIRED"));
        }
        // The real parser bounds this; enforce it for every source before holding data.
        if releases.len() > CATALOG_PAGE_SIZE {
            return Err(error("HISTORY_METADATA_INVALID"));
        }
        let full = releases.len() == CATALOG_PAGE_SIZE;
        for release in &releases {
            validate_release(release)?;
            if !seen.insert(release.id) {
                return Err(error("HISTORY_CATALOG_CHANGED"));
            }
        }
        let releases: Vec<_> = releases.into_iter().filter(public_release).collect();
        let mut held = self.held.lock();
        held.prune(completed);
        if held.observations.len() + releases.len() > MAX_OBSERVATIONS
            || held
                .observations
                .values()
                .filter(|item| &item.caller == caller)
                .count()
                + releases.len()
                > MAX_OWNER_OBSERVATIONS
        {
            return Err(error("HISTORY_CAPACITY"));
        }
        let next_cursor = if full && page < MAX_CATALOG_PAGES {
            if held.cursors.len() >= MAX_CURSORS
                || held
                    .cursors
                    .values()
                    .filter(|item| &item.caller == caller)
                    .count()
                    >= MAX_OWNER_CURSORS
            {
                return Err(error("HISTORY_CAPACITY"));
            }
            let token = uuid::Uuid::new_v4().simple().to_string();
            held.cursors.insert(
                token.clone(),
                Cursor {
                    caller: caller.clone(),
                    expires: completed + SELECTION_TTL,
                    page: page + 1,
                    seen,
                },
            );
            Some(token)
        } else {
            None
        };
        let mut rows = Vec::with_capacity(releases.len());
        for release in releases {
            let alias = uuid::Uuid::new_v4().simple().to_string();
            rows.push(release.project(self.host, alias.clone()));
            held.observations.insert(
                alias,
                Observation {
                    caller: caller.clone(),
                    expires: completed + SELECTION_TTL,
                    release,
                },
            );
        }
        Ok(HistoryCatalogPage {
            rows,
            next_cursor,
            truncated: full && page == MAX_CATALOG_PAGES,
        })
    }
    pub(crate) fn select(
        &self,
        caller: &CallerIdentity,
        release_id: &str,
        asset_id: &str,
    ) -> Result<HistorySelection, SafeError> {
        self.select_at(caller, release_id, asset_id, (self.clock)())
    }
    pub(crate) fn select_at(
        &self,
        caller: &CallerIdentity,
        release_id: &str,
        asset_id: &str,
        now: Instant,
    ) -> Result<HistorySelection, SafeError> {
        if !opaque(release_id) || !canonical_id(asset_id) {
            return Err(error("HISTORY_SELECTION_UNKNOWN"));
        }
        let observed = {
            let held = self.held.lock();
            let observation = held
                .observations
                .get(release_id)
                .filter(|item| &item.caller == caller)
                .ok_or_else(|| error("HISTORY_SELECTION_UNKNOWN"))?;
            if now >= observation.expires {
                return Err(error("HISTORY_SELECTION_EXPIRED"));
            }
            observation.release.clone()
        };
        let metadata = observed
            .bind(self.host)
            .map_err(|_| error("HISTORY_RELEASE_BLOCKED"))?;
        if metadata.installer.id.to_string() != asset_id {
            return Err(error("HISTORY_SELECTION_UNKNOWN"));
        }
        let current = self.source.release(metadata.release_id)?;
        if current != observed || current.bind(self.host).as_ref() != Ok(&metadata) {
            return Err(error("HISTORY_SELECTION_CHANGED"));
        }
        let completed = now.max((self.clock)());
        let mut held = self.held.lock();
        if held
            .observations
            .get(release_id)
            .is_none_or(|item| completed >= item.expires)
        {
            return Err(error("HISTORY_SELECTION_EXPIRED"));
        }
        held.prune(completed);
        if held.selections.len() >= MAX_SELECTIONS
            || held
                .selections
                .values()
                .filter(|item| &item.caller == caller)
                .count()
                >= MAX_OWNER_SELECTIONS
        {
            return Err(error("HISTORY_CAPACITY"));
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        let selected = HistorySelection {
            selection_token: token.clone(),
            release_id: release_id.to_owned(),
            asset_id: asset_id.to_owned(),
            version: metadata.version.clone(),
            expires_at: (chrono::Utc::now()
                + chrono::Duration::seconds(SELECTION_TTL.as_secs() as i64))
            .to_rfc3339(),
            verification: HistoryVerification::AwaitingVerification,
            install_ready: false,
        };
        held.selections.insert(
            token,
            Selection {
                caller: caller.clone(),
                expires: completed + SELECTION_TTL,
                metadata,
                release: observed,
            },
        );
        Ok(selected)
    }
    /// Call only after document admission, then recheck its liveness after external IO.
    pub(crate) fn resolve_selection(
        &self,
        caller: &CallerIdentity,
        token: &str,
    ) -> Result<SelectionMetadata, SafeError> {
        self.resolve_selection_at(caller, token, (self.clock)())
    }
    pub(crate) fn resolve_selection_at(
        &self,
        caller: &CallerIdentity,
        token: &str,
        now: Instant,
    ) -> Result<SelectionMetadata, SafeError> {
        if !opaque(token) {
            return Err(error("HISTORY_SELECTION_UNKNOWN"));
        }
        let held = self.held.lock();
        let selected = held
            .selections
            .get(token)
            .filter(|item| &item.caller == caller)
            .ok_or_else(|| error("HISTORY_SELECTION_UNKNOWN"))?;
        if now >= selected.expires {
            return Err(error("HISTORY_SELECTION_EXPIRED"));
        }
        Ok(selected.metadata.clone())
    }
    pub(crate) fn revalidate_selection(
        &self,
        caller: &CallerIdentity,
        token: &str,
    ) -> Result<SelectionMetadata, SafeError> {
        self.revalidate_selection_at(caller, token, (self.clock)())
    }
    pub(crate) fn revalidate_selection_at(
        &self,
        caller: &CallerIdentity,
        token: &str,
        now: Instant,
    ) -> Result<SelectionMetadata, SafeError> {
        let expected = self.resolve_selection_at(caller, token, now)?;
        let observed = self
            .held
            .lock()
            .selections
            .get(token)
            .filter(|item| &item.caller == caller)
            .map(|item| item.release.clone())
            .ok_or_else(|| error("HISTORY_SELECTION_UNKNOWN"))?;
        let current = self.source.release(expected.release_id)?;
        if current != observed || current.bind(self.host).as_ref() != Ok(&expected) {
            return Err(error("HISTORY_SELECTION_CHANGED"));
        }
        self.resolve_selection_at(caller, token, now.max((self.clock)()))
    }
    pub(super) fn retain_selection_observation(
        &self,
        caller: &CallerIdentity,
        token: &str,
    ) -> Result<Vec<u8>, SafeError> {
        let expected = self.revalidate_selection(caller, token)?;
        let held = self.held.lock();
        let selected = held
            .selections
            .get(token)
            .filter(|selected| &selected.caller == caller)
            .ok_or_else(|| error("HISTORY_SELECTION_UNKNOWN"))?;
        if (self.clock)() >= selected.expires || selected.metadata != expected {
            return Err(error("HISTORY_SELECTION_EXPIRED"));
        }
        let bytes = serde_json::to_vec(&RetainedSelectionObservation {
            schema: 1,
            release: selected.release.clone(),
            installer_id: expected.installer.id,
            signature_id: expected.signature.id,
        })
        .map_err(|_| error("HISTORY_METADATA_INVALID"))?;
        bounded(&bytes)?;
        Ok(bytes)
    }
    /// Rehydration always consults the official source again. Parsing a private
    /// record alone never constructs a held selection or execution authority.
    pub(crate) fn revalidate_retained_observation(
        &self,
        bytes: &[u8],
    ) -> Result<SelectionMetadata, SafeError> {
        let (observation, expected) = parse_retained_observation(bytes, self.host)?;
        let current = self.source.release(expected.release_id)?;
        validate_release(&current)?;
        if current != observation.release || current.bind(self.host).as_ref() != Ok(&expected) {
            return Err(error("HISTORY_SELECTION_CHANGED"));
        }
        Ok(expected)
    }
}
/// Display-only local facts. This type exposes neither a selection token nor a
/// SelectionMetadata owner, so restart inspection cannot use it to install.
#[derive(PartialEq, Eq)]
pub(crate) struct RetainedSelectionDiagnostic {
    version: String,
    installer_digest: String,
    installer_size: u64,
    signature_digest: String,
    signature_size: u64,
}
impl RetainedSelectionDiagnostic {
    pub(crate) fn version(&self) -> &str {
        &self.version
    }
    pub(crate) fn installer_digest(&self) -> &str {
        &self.installer_digest
    }
    pub(crate) fn installer_size(&self) -> u64 {
        self.installer_size
    }
    pub(crate) fn signature_digest(&self) -> &str {
        &self.signature_digest
    }
    pub(crate) fn signature_size(&self) -> u64 {
        self.signature_size
    }
}
fn parse_retained_observation(
    bytes: &[u8],
    host: HostPlatform,
) -> Result<(RetainedSelectionObservation, SelectionMetadata), SafeError> {
    bounded(bytes)?;
    let observation: RetainedSelectionObservation =
        serde_json::from_slice(bytes).map_err(|_| error("HISTORY_METADATA_INVALID"))?;
    if observation.schema != 1 {
        return Err(error("HISTORY_METADATA_INVALID"));
    }
    // Stored observations use our canonical projection. Reject extra keys,
    // including nested keys that the public GitHub parser may safely ignore.
    let supplied: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| error("HISTORY_METADATA_INVALID"))?;
    if serde_json::to_value(&observation).map_err(|_| error("HISTORY_METADATA_INVALID"))?
        != supplied
    {
        return Err(error("HISTORY_METADATA_INVALID"));
    }
    validate_release(&observation.release)?;
    let expected = observation
        .release
        .bind(host)
        .map_err(|_| error("HISTORY_RELEASE_BLOCKED"))?;
    if expected.installer.id != observation.installer_id
        || expected.signature.id != observation.signature_id
    {
        return Err(error("HISTORY_SELECTION_CHANGED"));
    }
    Ok((observation, expected))
}
/// Offline inspection of previously retained metadata. The caller must still
/// verify its protected record and actual publisher-signed package bytes. This
/// does not claim that an official asset is still available or unchanged.
pub(crate) fn inspect_retained_observation(
    bytes: &[u8],
) -> Result<RetainedSelectionDiagnostic, SafeError> {
    let (_, selected) = parse_retained_observation(bytes, HostPlatform::WindowsX64)?;
    Ok(RetainedSelectionDiagnostic {
        version: selected.version,
        installer_digest: selected.installer.sha256,
        installer_size: selected.installer.size,
        signature_digest: selected.signature.sha256,
        signature_size: selected.signature.size,
    })
}

fn opaque(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn canonical_id(value: &str) -> bool {
    !value.starts_with('0')
        && value
            .parse::<u64>()
            .is_ok_and(|number| number > 0 && number.to_string() == value)
}
