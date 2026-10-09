//! Safe history wire DTOs. Held asset URLs and package metadata never serialize.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum HistoryPlatform {
    #[serde(rename = "windows-x86_64")]
    WindowsX86_64,
    #[serde(rename = "darwin-aarch64")]
    DarwinAarch64,
    #[serde(rename = "linux-x86_64")]
    LinuxX86_64,
    Unsupported,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum HistoryPackageFormat {
    Nsis,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum HistoryVerification {
    AwaitingVerification,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum HistoryDataMode {
    Available,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum HistoryBlockReason {
    PlatformUnsupported,
    PlatformAssetMissing,
    PackageFormatUnsupported,
    PackagingBoundaryUnknown,
    SignatureMissing,
    DigestUnavailable,
    AssetAmbiguous,
    AssetMetadataInvalid,
    ReleaseNotHistorical,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryDataModes {
    pub(crate) fresh_settings: HistoryDataMode,
    pub(crate) keep_current_data: HistoryDataMode,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryRelease {
    /// Expiring per-observation identity. Never a caller-provided GitHub URL.
    pub(crate) release_id: String,
    pub(crate) asset_id: Option<String>,
    pub(crate) version: String,
    pub(crate) published_at: String,
    pub(crate) platform: HistoryPlatform,
    pub(crate) available_platforms: Vec<HistoryPlatform>,
    pub(crate) package_format: Option<HistoryPackageFormat>,
    pub(crate) verification: HistoryVerification,
    /// Allows selection/preparation only. Metadata never authorizes installation.
    pub(crate) select_allowed: bool,
    pub(crate) install_ready: bool,
    pub(crate) data_modes: HistoryDataModes,
    pub(crate) blocked_reason: Option<HistoryBlockReason>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryCatalogPage {
    pub(crate) rows: Vec<HistoryRelease>,
    pub(crate) next_cursor: Option<String>,
    /// Reached the bounded catalogue window; do not claim the history is complete.
    pub(crate) truncated: bool,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistorySelection {
    pub(crate) selection_token: String,
    pub(crate) release_id: String,
    pub(crate) asset_id: String,
    pub(crate) version: String,
    pub(crate) expires_at: String,
    pub(crate) verification: HistoryVerification,
    pub(crate) install_ready: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ListHistoryRequest {
    pub(crate) cursor: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SelectHistoryRequest {
    pub(crate) release_id: String,
    pub(crate) asset_id: String,
}
