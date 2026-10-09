//! Shared update HTTP transport; never imports CLI environment configuration.
use crate::updater_policy::{failure, UpdateFailure};
use std::time::Duration;

pub(crate) fn http_client(proxy: Option<&url::Url>) -> Result<reqwest::Client, UpdateFailure> {
    let mut builder = reqwest::Client::builder()
        .user_agent("CC-Desk-Updater")
        .timeout(Duration::from_secs(15));
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
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("UPDATER_REQUEST_FAILED", "provenance"))?
    {
        if data.len().saturating_add(chunk.len()) > maximum {
            return Err(failure("UPDATER_MANIFEST_INVALID", "provenance"));
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}
