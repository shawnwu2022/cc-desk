//! Shared update HTTP transport; never imports CLI environment configuration.
use crate::updater_policy::{failure, validated_proxy, UpdateFailure, REPOSITORY};
use serde::Serialize;
use std::time::{Duration, Instant};

pub(crate) fn http_client(proxy: Option<&url::Url>) -> Result<reqwest::Client, UpdateFailure> {
    let mut builder = reqwest::Client::builder()
        .user_agent("CC-Desk-Updater")
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() >= 10
                || url.scheme() != "https"
                || !url.username().is_empty()
                || url.password().is_some()
                || url.port_or_known_default() != Some(443)
                || !matches!(
                    url.host_str(),
                    Some(
                        "github.com"
                            | "api.github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                attempt.error("official update redirect refused")
            } else {
                attempt.follow()
            }
        }));
    // None preserves reqwest's inherited environment/system proxy behavior.
    if let Some(proxy) = proxy {
        builder = builder.proxy(
            reqwest::Proxy::all(proxy.as_str())
                .map_err(|_| failure("UPDATER_PROXY_INVALID", "proxy"))?,
        );
    }
    builder
        .build()
        .map_err(|_| failure("UPDATER_CONFIGURATION_INVALID", "configuration"))
}
pub(crate) async fn bytes(
    client: &reqwest::Client,
    url: &str,
    maximum: usize,
) -> Result<Vec<u8>, UpdateFailure> {
    let mut response = client.get(url).send().await.map_err(|e| {
        failure(
            if e.is_timeout() {
                "UPDATER_TIMEOUT"
            } else {
                "UPDATER_REQUEST_FAILED"
            },
            "provenance",
        )
    })?;
    if !response.status().is_success() {
        let limited = response.status().as_u16() == 429
            || response
                .headers()
                .get("x-ratelimit-remaining")
                .is_some_and(|h| h == "0");
        return Err(failure(
            if limited {
                "UPDATER_RATE_LIMITED"
            } else {
                "UPDATER_HTTP_REJECTED"
            },
            "provenance",
        ));
    }
    let mut data = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        failure(
            if error.is_timeout() {
                "UPDATER_TIMEOUT"
            } else {
                "UPDATER_REQUEST_FAILED"
            },
            "provenance",
        )
    })? {
        if data.len().saturating_add(chunk.len()) > maximum {
            return Err(failure("UPDATER_MANIFEST_INVALID", "provenance"));
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyProbe {
    elapsed_ms: u64,
    mode: &'static str,
}

pub(crate) fn validate_probe_manifest(data: &[u8]) -> Result<(), UpdateFailure> {
    let invalid = || failure("UPDATER_MANIFEST_INVALID", "provenance");
    let manifest: serde_json::Value = serde_json::from_slice(data).map_err(|_| invalid())?;
    let version = manifest["version"].as_str().ok_or_else(invalid)?;
    if semver::Version::parse(version).is_err()
        || !manifest["platforms"]
            .as_object()
            .is_some_and(|platforms| !platforms.is_empty())
    {
        return Err(invalid());
    }
    Ok(())
}

/// Read only the configured official manifest. Never follows package URLs or
/// creates an update/install admission, and never changes the saved proxy.
pub(crate) async fn probe_proxy(value: Option<&str>) -> Result<ProxyProbe, UpdateFailure> {
    let started = Instant::now();
    let proxy = validated_proxy(value)?;
    let client = http_client(proxy.as_ref())?;
    let data = bytes(
        &client,
        &format!("https://github.com/{REPOSITORY}/releases/latest/download/latest.json"),
        1024 * 1024,
    )
    .await?;
    validate_probe_manifest(&data)?;
    Ok(ProxyProbe {
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        mode: if proxy.is_some() {
            "custom"
        } else {
            "inherited"
        },
    })
}
