//! Deliberately narrow historical package policy; ordinary updater policy is unrelated.
use super::types::HistoryPlatform;

pub(crate) const PRODUCT_IDENTIFIER: &str = "io.github.shawnwu2022.ccdesk";
pub(crate) const OBSERVED_NSIS_VERSIONS: &[&str] = &[
    "0.14.0", "0.15.0", "0.16.0", "0.17.0", "0.17.1", "0.17.2", "0.17.5", "0.17.6", "0.17.7",
];
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostPlatform {
    WindowsX64,
    Unsupported,
}
impl HostPlatform {
    pub(crate) fn current() -> Self {
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Self::WindowsX64
        } else {
            Self::Unsupported
        }
    }
    pub(crate) fn wire(self) -> HistoryPlatform {
        match self {
            Self::WindowsX64 => HistoryPlatform::WindowsX86_64,
            Self::Unsupported => HistoryPlatform::Unsupported,
        }
    }
}
/// Stable canonical semver only. No guessed compatibility or version ordering by strings.
pub(crate) fn version(tag: &str) -> Option<&str> {
    let value = tag.strip_prefix('v')?;
    if value.len() > 32 {
        return None;
    }
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
                || part.parse::<u32>().is_err()
        })
    {
        return None;
    }
    Some(value)
}
pub(crate) fn historical(value: &str) -> bool {
    let numbers = |text: &str| -> Option<[u32; 3]> {
        let parts: Vec<_> = text
            .split('.')
            .map(str::parse::<u32>)
            .collect::<Result<_, _>>()
            .ok()?;
        parts.try_into().ok()
    };
    numbers(value)
        .zip(numbers(env!("CARGO_PKG_VERSION")))
        .is_some_and(|(target, current)| target < current)
}
/// Existing trusted key is read from committed application configuration, never a release.
pub(crate) fn trusted_public_key() -> String {
    let config: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json"))
        .expect("committed Tauri config must parse");
    config["plugins"]["updater"]["pubkey"]
        .as_str()
        .expect("committed publisher key must exist")
        .to_owned()
}
