//! Ordinary updater admission against the existing official GitHub release.
//! Candidate markers only exclude. The host retains the actual install capability.
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

pub(crate) const REPOSITORY: &str = "shawnwu2022/cc-desk";
pub(crate) const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_SIGNATURE_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct UpdateFailure {
    pub(crate) code: &'static str,
    pub(crate) stage: &'static str,
}
pub(crate) fn failure(code: &'static str, stage: &'static str) -> UpdateFailure {
    UpdateFailure { code, stage }
}
fn rejected() -> UpdateFailure {
    failure("UPDATER_NOT_OFFICIAL", "provenance")
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OfficialAsset {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OfficialRelease {
    pub(crate) release_id: u64,
    pub(crate) version: String,
    pub(crate) source_sha: String,
    pub(crate) package: OfficialAsset,
    pub(crate) signature: OfficialAsset,
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn validated_proxy(value: Option<&str>) -> Result<Option<Url>, UpdateFailure> {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let invalid = || failure("UPDATER_PROXY_INVALID", "proxy");
    if value.len() > 2048 {
        return Err(invalid());
    }
    let url = Url::parse(value).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(invalid());
    }
    Ok(Some(url))
}
fn asset(
    value: &Value,
    expected_name: &str,
    expected_url: &str,
    maximum: u64,
) -> Result<OfficialAsset, UpdateFailure> {
    let digest = value["digest"]
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .filter(|s| hex(s, 64))
        .ok_or_else(rejected)?;
    let id = value["id"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(rejected)?;
    let size = value["size"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= maximum)
        .ok_or_else(rejected)?;
    if value["name"].as_str() != Some(expected_name)
        || value["browser_download_url"].as_str() != Some(expected_url)
        || value["state"].as_str() != Some("uploaded")
    {
        return Err(rejected());
    }
    Ok(OfficialAsset {
        id,
        name: expected_name.into(),
        url: expected_url.into(),
        size,
        sha256: digest.into(),
    })
}
pub(crate) fn validate_official_release(
    release: &Value,
    version: &str,
    platform: &str,
    download_url: &str,
    manifest: &Value,
) -> Result<OfficialRelease, UpdateFailure> {
    let parsed = semver::Version::parse(version).map_err(|_| rejected())?;
    if !parsed.pre.is_empty()
        || parsed.to_string() != version
        || version.len() > 64
        || !manifest.is_object()
        || matches!(
            manifest["channel"].as_str(),
            Some("candidate" | "test-only")
        )
        || manifest["publishable"] == false
        || manifest["updaterPublication"] == false
        || manifest
            .get("product")
            .is_some_and(|p| p.as_str() != Some("CC Desk"))
    {
        return Err(rejected());
    }
    let tag = format!("v{version}");
    let source = release["target_commitish"]
        .as_str()
        .filter(|s| hex(s, 40))
        .ok_or_else(rejected)?;
    let body = release["body"].as_str().ok_or_else(rejected)?;
    let sources: Vec<_> = body
        .lines()
        .filter_map(|line| line.strip_prefix("Source: "))
        .collect();
    if release["draft"] != false
        || release["prerelease"] != false
        || release["published_at"].as_str().is_none()
        || release["tag_name"].as_str() != Some(tag.as_str())
        || release["html_url"].as_str()
            != Some(format!("https://github.com/{REPOSITORY}/releases/tag/{tag}").as_str())
        || sources != [source]
        || !body.lines().any(|line| {
            line == "Validation policy: required-checks-and-disclosed-host-unverified-v1"
        })
    {
        return Err(rejected());
    }
    let name = match platform {
        "windows-x86_64" => format!("CC.Desk_{version}_x64-setup.exe"),
        "linux-x86_64" => format!("CC.Desk_{version}_amd64.AppImage"),
        "darwin-aarch64" => "CC.Desk.app.tar.gz".into(),
        _ => return Err(failure("UPDATER_PLATFORM_UNAVAILABLE", "provenance")),
    };
    let expected_url = format!("https://github.com/{REPOSITORY}/releases/download/{tag}/{name}");
    if download_url != expected_url {
        return Err(rejected());
    }
    let assets = release["assets"].as_array().ok_or_else(rejected)?;
    let select = |name: &str| -> Result<&Value, UpdateFailure> {
        let matches: Vec<_> = assets
            .iter()
            .filter(|a| a["name"].as_str() == Some(name))
            .collect();
        if matches.len() != 1 {
            return Err(rejected());
        }
        Ok(matches[0])
    };
    let package = asset(select(&name)?, &name, &expected_url, MAX_PACKAGE_BYTES)?;
    let signature_name = format!("{name}.sig");
    let signature = asset(
        select(&signature_name)?,
        &signature_name,
        &format!("{expected_url}.sig"),
        MAX_SIGNATURE_BYTES,
    )?;
    if package.id == signature.id {
        return Err(rejected());
    }
    Ok(OfficialRelease {
        release_id: release["id"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(rejected)?,
        version: version.into(),
        source_sha: source.into(),
        package,
        signature,
    })
}
pub(crate) fn validate_tag_commit(
    reference: &Value,
    proof: &OfficialRelease,
) -> Result<(), UpdateFailure> {
    if reference["ref"].as_str() != Some(format!("refs/tags/v{}", proof.version).as_str())
        || reference["object"]["type"].as_str() != Some("commit")
        || reference["object"]["sha"].as_str() != Some(proof.source_sha.as_str())
    {
        return Err(rejected());
    }
    Ok(())
}
pub(crate) fn validate_source_config(
    config: &Value,
    proof: &OfficialRelease,
    public_key: &str,
) -> Result<(), UpdateFailure> {
    if config["version"].as_str() != Some(proof.version.as_str())
        || config["plugins"]["updater"]["pubkey"].as_str() != Some(public_key)
    {
        return Err(rejected());
    }
    Ok(())
}
pub(crate) fn verify_signature_asset(
    bytes: &[u8],
    expected: &OfficialAsset,
    manifest_signature: &str,
) -> Result<(), UpdateFailure> {
    if bytes.len() as u64 != expected.size
        || sha256(bytes) != expected.sha256
        || std::str::from_utf8(bytes).ok().map(str::trim) != Some(manifest_signature.trim())
    {
        return Err(failure("UPDATER_SIGNATURE_INVALID", "provenance"));
    }
    Ok(())
}
pub(crate) fn verify_package(
    bytes: &[u8],
    signature: &str,
    public_key: &str,
    expected: &OfficialAsset,
) -> Result<(), UpdateFailure> {
    if bytes.len() as u64 != expected.size
        || expected.size == 0
        || expected.size > MAX_PACKAGE_BYTES
        || sha256(bytes) != expected.sha256
    {
        return Err(failure("UPDATER_PACKAGE_MISMATCH", "download"));
    }
    let invalid = || failure("UPDATER_SIGNATURE_INVALID", "download");
    if public_key.len() > 2048 || signature.len() > MAX_SIGNATURE_BYTES as usize {
        return Err(invalid());
    }
    let key_text = STANDARD.decode(public_key.trim()).map_err(|_| invalid())?;
    let signature_text = STANDARD.decode(signature.trim()).map_err(|_| invalid())?;
    let key = PublicKey::decode(std::str::from_utf8(&key_text).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    let signed = Signature::decode(std::str::from_utf8(&signature_text).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    key.verify(bytes, &signed, true).map_err(|_| invalid())
}
