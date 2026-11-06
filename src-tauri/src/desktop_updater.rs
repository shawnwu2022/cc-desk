//! Read-only discovery and one-shot, host-retained official update installation.
use crate::updater_http::{bytes, http_client, package_bytes, probe_proxy, ProxyProbe};
use crate::updater_policy::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;

#[derive(Default)]
pub(crate) struct UpdaterService {
    state: Mutex<UpdaterState>,
    cancellation: std::sync::Mutex<Option<(String, Arc<InstallCancellation>)>>,
}
impl UpdaterService {
    fn cancellation(&self, id: &str) -> Result<Arc<InstallCancellation>, UpdateFailure> {
        self.cancellation
            .lock()
            .map_err(|_| failure("UPDATER_FAILED", "admission"))?
            .as_ref()
            .filter(|(current, _)| current == id)
            .map(|(_, control)| control.clone())
            .ok_or_else(|| failure("UPDATER_ADMISSION_STALE", "admission"))
    }
    fn set_cancellation(&self, id: Option<&str>) -> Result<(), UpdateFailure> {
        *self
            .cancellation
            .lock()
            .map_err(|_| failure("UPDATER_FAILED", "admission"))? =
            id.map(|id| (id.to_owned(), Arc::new(InstallCancellation::default())));
        Ok(())
    }
}
#[derive(Default)]
struct UpdaterState {
    pending: Option<PendingUpdate>,
    install_started: bool,
}
struct PendingUpdate {
    id: String,
    update: Update,
    proof: OfficialRelease,
    proxy: Option<url::Url>,
    selected_channel: UpdateChannel,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateSummary {
    version: String,
    current_version: String,
    has_update: bool,
    release_notes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_notes_by_locale: Option<std::collections::BTreeMap<&'static str, String>>,
    channel: &'static str,
    selected_channel: UpdateChannel,
    install_eligible: bool,
    admission_id: Option<String>,
    official_release: Option<ReleaseSummary>,
    download_url: String,
    platform_asset: Option<AssetSummary>,
    eligibility_reason: Option<&'static str>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseSummary {
    id: u64,
    tag: String,
    source_sha: String,
}
#[derive(Serialize)]
struct AssetSummary {
    name: String,
    url: String,
    size: u64,
}
#[derive(Serialize)]
pub(crate) struct UpdaterSettings {
    proxy: Option<String>,
    channel: UpdateChannel,
}
fn main_window(window: &WebviewWindow) -> Result<(), UpdateFailure> {
    if window.label() != "main" {
        return Err(failure("UPDATER_CALLER_DENIED", "admission"));
    }
    Ok(())
}
fn report(error: UpdateFailure) -> UpdateFailure {
    log::warn!("update_diag code={} stage={}", error.code, error.stage);
    error
}
fn plugin_error(error: tauri_plugin_updater::Error, stage: &'static str) -> UpdateFailure {
    use tauri_plugin_updater::Error;
    let code = match error {
        Error::Reqwest(e) if e.is_timeout() => "UPDATER_TIMEOUT",
        Error::Reqwest(e) if e.is_decode() => "UPDATER_MANIFEST_INVALID",
        Error::Reqwest(_) | Error::Network(_) => "UPDATER_REQUEST_FAILED",
        Error::Serialization(_) | Error::Semver(_) => "UPDATER_MANIFEST_INVALID",
        Error::TargetsNotFound(_)
        | Error::TargetNotFound(_)
        | Error::UnsupportedArch
        | Error::UnsupportedOs => "UPDATER_PLATFORM_UNAVAILABLE",
        Error::ReleaseNotFound => "UPDATER_HTTP_REJECTED",
        Error::Minisign(_) | Error::Base64(_) | Error::SignatureUtf8(_) => {
            "UPDATER_SIGNATURE_INVALID"
        }
        _ => "UPDATER_FAILED",
    };
    report(failure(code, stage))
}
fn public_key() -> Result<String, UpdateFailure> {
    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .map_err(|_| failure("UPDATER_CONFIGURATION_INVALID", "configuration"))?;
    config["plugins"]["updater"]["pubkey"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| failure("UPDATER_CONFIGURATION_INVALID", "configuration"))
}
fn configured_proxy() -> Result<Option<url::Url>, UpdateFailure> {
    let config = crate::store::get_app_config()
        .map_err(|_| failure("UPDATER_CONFIGURATION_INVALID", "configuration"))?;
    validated_proxy(config.updater_proxy.as_deref())
}
fn configured_channel() -> Result<UpdateChannel, UpdateFailure> {
    let config = crate::store::get_app_config()
        .map_err(|_| failure("UPDATER_CONFIGURATION_INVALID", "configuration"))?;
    UpdateChannel::from_config(config.updater_channel.as_deref())
}
async fn release_selection(
    client: &reqwest::Client,
    current: &str,
    selected: UpdateChannel,
) -> Result<Option<(u64, String)>, UpdateFailure> {
    let mut releases = Vec::new();
    for page in 1..=10 {
        let value = api(client, &format!("releases?per_page=25&page={page}")).await?;
        let entries = value
            .as_array()
            .filter(|entries| entries.len() <= 25)
            .ok_or_else(|| failure("UPDATER_MANIFEST_INVALID", "check"))?;
        releases.extend(entries.iter().cloned());
        if entries.len() < 25 {
            return select_release(&releases, current, selected)?
                .map(|release| {
                    let id = release["id"]
                        .as_u64()
                        .filter(|id| *id > 0)
                        .ok_or_else(|| failure("UPDATER_NOT_OFFICIAL", "provenance"))?;
                    let version = release["tag_name"]
                        .as_str()
                        .and_then(|s| s.strip_prefix('v'))
                        .ok_or_else(|| failure("UPDATER_NOT_OFFICIAL", "provenance"))?;
                    Ok((id, version.to_owned()))
                })
                .transpose();
        }
    }
    Err(failure("UPDATER_CATALOG_LIMIT", "check"))
}
async fn api(client: &reqwest::Client, path: &str) -> Result<Value, UpdateFailure> {
    serde_json::from_slice(
        &bytes(
            client,
            &format!("https://api.github.com/repos/{REPOSITORY}/{path}"),
            1024 * 1024,
        )
        .await?,
    )
    .map_err(|_| failure("UPDATER_MANIFEST_INVALID", "provenance"))
}
async fn official_proof(
    client: &reqwest::Client,
    update: &Update,
    release_id: Option<u64>,
    selected: UpdateChannel,
) -> Result<OfficialRelease, UpdateFailure> {
    let path = release_id.map_or_else(|| "releases/latest".into(), |id| format!("releases/{id}"));
    let release = api(client, &path).await?;
    let platform = tauri_plugin_updater::target()
        .ok_or_else(|| failure("UPDATER_PLATFORM_UNAVAILABLE", "provenance"))?;
    let proof = validate_official_release(
        &release,
        &update.version,
        &platform,
        update.download_url.as_str(),
        &update.raw_json,
        selected,
    )?;
    if release_id.is_some_and(|id| id != proof.release_id) {
        return Err(failure("UPDATER_NOT_OFFICIAL", "provenance"));
    }
    let mut reference = api(client, &format!("git/ref/tags/v{}", proof.version)).await?;
    // Both official lightweight and annotated tags resolve to the exact source.
    for _ in 0..3 {
        if reference["object"]["type"].as_str() != Some("tag") {
            break;
        }
        let sha = reference["object"]["sha"]
            .as_str()
            .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| failure("UPDATER_NOT_OFFICIAL", "provenance"))?;
        let tag = api(client, &format!("git/tags/{sha}")).await?;
        if tag["sha"].as_str() != Some(sha) {
            return Err(failure("UPDATER_NOT_OFFICIAL", "provenance"));
        }
        reference["object"] = tag["object"].clone();
    }
    validate_tag_commit(&reference, &proof)?;
    let config = api(
        client,
        &format!(
            "contents/src-tauri/tauri.conf.json?ref={}",
            proof.source_sha
        ),
    )
    .await?;
    if config["encoding"].as_str() != Some("base64") {
        return Err(failure("UPDATER_NOT_OFFICIAL", "provenance"));
    }
    let encoded = config["content"]
        .as_str()
        .filter(|s| s.len() <= 128 * 1024)
        .ok_or_else(|| failure("UPDATER_NOT_OFFICIAL", "provenance"))?;
    let decoded = STANDARD
        .decode(encoded.split_whitespace().collect::<String>())
        .map_err(|_| failure("UPDATER_NOT_OFFICIAL", "provenance"))?;
    let source: Value = serde_json::from_slice(&decoded)
        .map_err(|_| failure("UPDATER_NOT_OFFICIAL", "provenance"))?;
    validate_source_config(&source, &proof, &public_key()?)?;
    let signature = bytes(client, &proof.signature.url, MAX_SIGNATURE_BYTES as usize).await?;
    verify_signature_asset(&signature, &proof.signature, &update.signature)?;
    Ok(proof)
}
#[tauri::command]
pub(crate) fn get_updater_settings(
    window: WebviewWindow,
) -> Result<UpdaterSettings, UpdateFailure> {
    main_window(&window)?;
    let proxy = configured_proxy()
        .map_err(report)?
        .map(|url| url.to_string());
    Ok(UpdaterSettings {
        proxy,
        channel: configured_channel().map_err(report)?,
    })
}
#[tauri::command]
pub(crate) async fn test_updater_proxy(
    window: WebviewWindow,
    proxy: Option<String>,
) -> Result<ProxyProbe, UpdateFailure> {
    main_window(&window)?;
    probe_proxy(proxy.as_deref()).await.map_err(report)
}
#[tauri::command]
pub(crate) async fn save_updater_settings(
    window: WebviewWindow,
    proxy: Option<String>,
    service: State<'_, UpdaterService>,
) -> Result<(), UpdateFailure> {
    main_window(&window)?;
    let mut state = service.state.lock().await;
    if state.install_started {
        return Err(report(failure(
            "UPDATER_INSTALL_OUTCOME_UNKNOWN",
            "install",
        )));
    }
    let value = validated_proxy(proxy.as_deref())
        .map_err(report)?
        .map(|url| url.to_string());
    state.pending = None;
    service.set_cancellation(None).map_err(report)?;
    crate::store::update_app_config(json!({ "updaterProxy": value }))
        .map_err(|_| report(failure("UPDATER_SETTINGS_SAVE_FAILED", "configuration")))
}
#[tauri::command]
pub(crate) async fn save_updater_channel(
    window: WebviewWindow,
    channel: UpdateChannel,
    service: State<'_, UpdaterService>,
) -> Result<(), UpdateFailure> {
    main_window(&window)?;
    let mut state = service.state.lock().await;
    if state.install_started {
        return Err(report(failure(
            "UPDATER_INSTALL_OUTCOME_UNKNOWN",
            "install",
        )));
    }
    state.pending = None;
    service.set_cancellation(None).map_err(report)?;
    crate::store::update_app_config(json!({ "updaterChannel": channel }))
        .map_err(|_| report(failure("UPDATER_SETTINGS_SAVE_FAILED", "configuration")))
}
#[tauri::command]
pub(crate) async fn check_desktop_update(
    app: AppHandle,
    window: WebviewWindow,
    service: State<'_, UpdaterService>,
) -> Result<UpdateSummary, UpdateFailure> {
    main_window(&window)?;
    let mut state = service.state.lock().await;
    if state.install_started {
        return Err(report(failure(
            "UPDATER_INSTALL_OUTCOME_UNKNOWN",
            "install",
        )));
    }
    state.pending = None;
    service.set_cancellation(None).map_err(report)?;
    let current = app.package_info().version.to_string();
    let proxy = configured_proxy().map_err(report)?;
    let selected = configured_channel().map_err(report)?;
    let client = http_client(proxy.as_ref()).map_err(report)?;
    let selection = if selected == UpdateChannel::Stable {
        None
    } else {
        release_selection(&client, &current, selected)
            .await
            .map_err(report)?
    };
    let mut builder = app.updater_builder().timeout(Duration::from_secs(20));
    if let Some((_, version)) = &selection {
        let endpoint =
            format!("https://github.com/{REPOSITORY}/releases/download/v{version}/latest.json")
                .parse()
                .map_err(|_| report(failure("UPDATER_NOT_OFFICIAL", "provenance")))?;
        builder = builder
            .endpoints(vec![endpoint])
            .map_err(|e| plugin_error(e, "check"))?;
    }
    if let Some(proxy) = &proxy {
        builder = builder.proxy(proxy.clone());
    }
    let update = if selected != UpdateChannel::Stable && selection.is_none() {
        None
    } else {
        builder
            .build()
            .map_err(|e| plugin_error(e, "check"))?
            .check()
            .await
            .map_err(|e| plugin_error(e, "check"))?
    };
    let Some(update) = update else {
        return Ok(UpdateSummary {
            version: current.clone(),
            current_version: current,
            has_update: false,
            release_notes: String::new(),
            release_notes_by_locale: None,
            channel: "stable",
            selected_channel: selected,
            install_eligible: false,
            admission_id: None,
            official_release: None,
            download_url: String::new(),
            platform_asset: None,
            eligibility_reason: None,
        });
    };
    if selection
        .as_ref()
        .is_some_and(|(_, version)| *version != update.version)
    {
        return Err(report(failure("UPDATER_NOT_OFFICIAL", "provenance")));
    }
    let mut summary = UpdateSummary {
        version: update.version.clone(),
        current_version: current,
        has_update: true,
        release_notes: update
            .body
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(4000)
            .collect(),
        release_notes_by_locale: localized_release_notes(&update.raw_json),
        channel: "unverified",
        selected_channel: selected,
        install_eligible: false,
        admission_id: None,
        official_release: None,
        download_url: String::new(),
        platform_asset: None,
        eligibility_reason: Some("UPDATER_NOT_OFFICIAL"),
    };
    match update.raw_json["channel"].as_str() {
        Some("candidate") => summary.channel = "candidate",
        Some("test-only") => summary.channel = "test-only",
        _ => {}
    }
    if summary.channel != "unverified"
        || update.raw_json["publishable"] == false
        || update.raw_json["updaterPublication"] == false
    {
        return Ok(summary);
    }
    let proof = match official_proof(
        &client,
        &update,
        selection.as_ref().map(|(id, _)| *id),
        selected,
    )
    .await
    {
        Ok(proof) => proof,
        Err(error)
            if error.code == "UPDATER_NOT_OFFICIAL"
                || error.code == "UPDATER_SIGNATURE_INVALID" =>
        {
            summary.eligibility_reason = Some(error.code);
            report(error);
            return Ok(summary);
        }
        Err(error) => return Err(report(error)),
    };
    let id = uuid::Uuid::new_v4().to_string();
    summary.channel = release_channel(&proof.version)
        .ok_or_else(|| report(failure("UPDATER_NOT_OFFICIAL", "provenance")))?
        .as_str();
    summary.install_eligible = true;
    summary.eligibility_reason = None;
    summary.admission_id = Some(id.clone());
    summary.official_release = Some(ReleaseSummary {
        id: proof.release_id,
        tag: format!("v{}", proof.version),
        source_sha: proof.source_sha.clone(),
    });
    summary.download_url = proof.package.url.clone();
    summary.platform_asset = Some(AssetSummary {
        name: proof.package.name.clone(),
        url: proof.package.url.clone(),
        size: proof.package.size,
    });
    state.pending = Some(PendingUpdate {
        id,
        update,
        proof,
        proxy,
        selected_channel: selected,
    });
    service
        .set_cancellation(summary.admission_id.as_deref())
        .map_err(report)?;
    Ok(summary)
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    admission_id: String,
    phase: &'static str,
    downloaded: u64,
    total: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CancelSummary {
    status: CancelStatus,
}
#[tauri::command]
pub(crate) fn cancel_desktop_update(
    window: WebviewWindow,
    admission_id: String,
    service: State<'_, UpdaterService>,
) -> Result<CancelSummary, UpdateFailure> {
    main_window(&window)?;
    Ok(CancelSummary {
        status: service
            .cancellation(&admission_id)
            .map_err(report)?
            .request(),
    })
}
#[tauri::command]
pub(crate) async fn install_desktop_update(
    app: AppHandle,
    window: WebviewWindow,
    admission_id: String,
    service: State<'_, UpdaterService>,
    gate: State<'_, crate::version_history::maintenance::AdmissionGate>,
) -> Result<(), UpdateFailure> {
    main_window(&window)?;
    let mut state = service.state.lock().await;
    if state.install_started {
        return Err(report(failure(
            "UPDATER_INSTALL_OUTCOME_UNKNOWN",
            "install",
        )));
    }
    if state
        .pending
        .as_ref()
        .is_none_or(|pending| pending.id != admission_id)
    {
        return Err(report(failure("UPDATER_ADMISSION_STALE", "admission")));
    }
    let cancellation = service.cancellation(&admission_id).map_err(report)?;
    cancellation.check().map_err(report)?;
    let runtime = app.state::<Arc<crate::cli::native_runtime::NativeRuntime>>();
    if runtime
        .binding()
        .map_or(true, |binding| binding.blocks_handoff_exit())
    {
        return Err(report(failure("UPDATER_SESSIONS_BUSY", "admission")));
    }
    let frozen = gate
        .freeze(&admission_id)
        .map_err(|_| report(failure("UPDATER_SESSIONS_BUSY", "admission")))?;
    if frozen.verify_quiescent(&admission_id).is_err() {
        frozen
            .release_review()
            .map_err(|_| report(failure("UPDATER_INSTALL_OUTCOME_UNKNOWN", "admission")))?;
        return Err(report(failure("UPDATER_SESSIONS_BUSY", "admission")));
    }
    let pending = state.pending.take().expect("checked under updater mutex");
    let prepare = async {
        let client = http_client(pending.proxy.as_ref())?;
        if configured_channel()? != pending.selected_channel {
            return Err(failure("UPDATER_ADMISSION_STALE", "admission"));
        }
        if official_proof(
            &client,
            &pending.update,
            Some(pending.proof.release_id),
            pending.selected_channel,
        )
        .await?
            != pending.proof
        {
            return Err(failure("UPDATER_ADMISSION_STALE", "provenance"));
        }
        cancellation.check()?;
        let data = package_bytes(
            &client,
            &pending.proof.package.url,
            pending.proof.package.size,
            |downloaded| {
                let _ = app.emit(
                    "desktop-update-progress",
                    UpdateProgress {
                        admission_id: admission_id.clone(),
                        phase: "downloading",
                        downloaded,
                        total: pending.proof.package.size,
                    },
                );
            },
        )
        .await?;
        let _ = app.emit(
            "desktop-update-progress",
            UpdateProgress {
                admission_id: admission_id.clone(),
                phase: "verifying",
                downloaded: data.len() as u64,
                total: pending.proof.package.size,
            },
        );
        verify_package(
            &data,
            &pending.update.signature,
            &public_key()?,
            &pending.proof.package,
        )?;
        cancellation.check()?;
        if configured_channel()? != pending.selected_channel {
            return Err(failure("UPDATER_ADMISSION_STALE", "admission"));
        }
        frozen
            .verify_quiescent(&admission_id)
            .map_err(|_| failure("UPDATER_SESSIONS_BUSY", "admission"))?;
        Ok(data)
    };
    let prepared: Result<Vec<u8>, UpdateFailure> = tokio::select! {
        _ = cancellation.cancelled() => Err(failure("UPDATER_CANCELLED", "download")),
        result = prepare => result,
    };
    let data = match prepared {
        Ok(data) => data,
        Err(error) => {
            let retired = service.set_cancellation(None);
            frozen
                .release_review()
                .map_err(|_| report(failure("UPDATER_INSTALL_OUTCOME_UNKNOWN", "admission")))?;
            retired.map_err(report)?;
            return Err(report(error));
        }
    };
    if let Err(error) = cancellation.commit() {
        let retired = service.set_cancellation(None);
        frozen
            .release_review()
            .map_err(|_| report(failure("UPDATER_INSTALL_OUTCOME_UNKNOWN", "admission")))?;
        retired.map_err(report)?;
        return Err(report(error));
    }
    // Consume the capability before entering the effectful installer. Unknown
    // receipts never reopen admission or invoke the installer a second time.
    state.install_started = true;
    let _ = app.emit(
        "desktop-update-progress",
        UpdateProgress {
            admission_id: admission_id.clone(),
            phase: "installing",
            downloaded: pending.proof.package.size,
            total: pending.proof.package.size,
        },
    );
    runtime.shutdown();
    if let Some(legacy) = crate::pty::get_pty_manager() {
        legacy.kill_all();
    }
    pending
        .update
        .install(data)
        .map_err(|_| report(failure("UPDATER_INSTALL_OUTCOME_UNKNOWN", "install")))?;
    let _ = app.emit(
        "desktop-update-progress",
        UpdateProgress {
            admission_id,
            phase: "restarting",
            downloaded: pending.proof.package.size,
            total: pending.proof.package.size,
        },
    );
    // Windows starts the official installer and exits in the Tauri plugin.
    // Other desktop platforms finish replacement before a normal app restart.
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(non_snake_case)]
    fn UpdateCancellation_RetiredAdmissionCannotAffectReplacement_009() {
        let service = UpdaterService::default();
        service.set_cancellation(Some("old-admission")).unwrap();
        let old = service.cancellation("old-admission").unwrap();
        service.set_cancellation(Some("new-admission")).unwrap();
        assert_eq!(
            service.cancellation("old-admission").err().unwrap().code,
            "UPDATER_ADMISSION_STALE"
        );
        assert_eq!(old.request(), CancelStatus::Requested);
        let replacement = service.cancellation("new-admission").unwrap();
        replacement.commit().unwrap();
        assert_eq!(replacement.request(), CancelStatus::TooLate);
        service.set_cancellation(None).unwrap();
        assert_eq!(
            service.cancellation("new-admission").err().unwrap().code,
            "UPDATER_ADMISSION_STALE"
        );
    }

    #[tokio::test]
    #[allow(non_snake_case)]
    async fn UpdateCancellation_DownloadStateLockDoesNotBlockCancel_010() {
        let service = UpdaterService::default();
        service.set_cancellation(Some("admission")).unwrap();
        let _held_download = service.state.lock().await;
        assert_eq!(
            service.cancellation("admission").unwrap().request(),
            CancelStatus::Requested
        );
    }
}
