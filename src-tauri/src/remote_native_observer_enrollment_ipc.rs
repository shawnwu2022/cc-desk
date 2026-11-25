//! Verified desktop setup IPC, independently authenticated to the existing
//! privileged native document. No session selection and no credentials cross
//! the renderer boundary. A successful setup proof isn't a data capability.
use super::{
    control::{verify_configured_host, ControlFailure},
    decode,
    setup::{OperatorHostProfileInput, PublicRegistrationMetadata, TrustedHostProfile},
};
use crate::cli::{native_runtime::NativeRuntime, profiles::error, types::SafeError};
use cc_desk_remote_record::appbind::CanonicalOrigin;
use cc_desk_remote_transport::wss::{ConnectError, EnrollmentError, HostEnrollmentResponse};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Read,
    net::SocketAddr,
    os::windows::fs::MetadataExt,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{ipc::Request, Manager, State, Webview};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
enum Action {
    Available,
    Verify { endpoint: String },
    PublicRegistration { endpoint: String },
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum Reply {
    Available,
    ProfileRequired,
    RelayAuthorizationRequired,
    SetupVerified,
    Declined,
    PublicRegistration {
        metadata: PublicRegistrationMetadata,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileProfile {
    version: u8,
    profile_id: String,
    profile_revision: u64,
    endpoint_name: String,
    endpoint_spki: String,
    relay_name: String,
    relay_spki: String,
    relay_id: String,
    relay_origin: CanonicalOrigin,
    socket_address: SocketAddr,
}

fn configured_profile(
    dir: PathBuf,
    endpoint: &str,
) -> Result<Option<(Arc<TrustedHostProfile>, SocketAddr)>, SafeError> {
    let missing = || error("REMOTE_PAIRING_SETUP_REQUIRED");
    let load = || -> Result<Option<(Arc<TrustedHostProfile>, SocketAddr)>, SafeError> {
        const MAX_BYTES: u64 = 4096;
        const REPARSE_POINT: u32 = 0x400;
        let path = dir.join("native-observer-relay-v1.json");
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(missing()),
        };
        if !meta.is_file() || meta.file_attributes() & REPARSE_POINT != 0 || meta.len() > MAX_BYTES
        {
            return Err(missing());
        }
        let mut bytes = Vec::new();
        let file = File::open(&path).map_err(|_| missing())?;
        if !file.metadata().map_err(|_| missing())?.is_file() {
            return Err(missing());
        }
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| missing())?;
        if bytes.len() > MAX_BYTES as usize {
            return Err(missing());
        }
        let selected: FileProfile = serde_json::from_slice(&bytes).map_err(|_| missing())?;
        let url = url::Url::parse(selected.relay_origin.as_str()).map_err(|_| missing())?;
        if selected.version != 1
            || selected.socket_address.ip().is_unspecified()
            || url.port_or_known_default() != Some(selected.socket_address.port())
        {
            return Err(missing());
        }
        let input = OperatorHostProfileInput {
            profile_id: decode(&selected.profile_id).map_err(|_| missing())?,
            revision: selected.profile_revision,
            endpoint_name: selected.endpoint_name,
            relay_name: selected.relay_name,
            endpoint_spki: decode(&selected.endpoint_spki).map_err(|_| missing())?,
            relay_spki: decode(&selected.relay_spki).map_err(|_| missing())?,
            relay_id: decode(&selected.relay_id).map_err(|_| missing())?,
            origin: selected.relay_origin,
        };
        let profile = TrustedHostProfile::from_operator(input, endpoint).map_err(|_| missing())?;
        Ok(Some((Arc::new(profile), selected.socket_address)))
    };
    load()
}

#[tauri::command]
pub(crate) async fn remote_native_observer_setup(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<Reply, SafeError> {
    let (binding, caller, action) =
        runtime.admit_host_document::<Action, _>(&webview, request.headers(), request.body())?;
    if matches!(action, Action::Available) {
        return Ok(Reply::Available);
    }
    let endpoint = match &action {
        Action::Available => return Ok(Reply::Available),
        Action::Verify { endpoint } | Action::PublicRegistration { endpoint } => endpoint.clone(),
    };
    let app = webview.app_handle();
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|_| error("REMOTE_PAIRING_SETUP_REQUIRED"))?;
    let selected = tokio::task::spawn_blocking(move || configured_profile(dir, &endpoint))
        .await
        .map_err(|_| error("REMOTE_PAIRING_SETUP_REQUIRED"))?;
    let (profile, address) = match selected {
        Ok(Some(profile)) => profile,
        Ok(None) | Err(_) => return Ok(Reply::ProfileRequired),
    };
    binding.check_admitted(&caller)?;
    let window = webview.window();
    let wait_end = Instant::now() + Duration::from_secs(30);
    match action {
        Action::Available => Ok(Reply::Available),
        Action::Verify { .. } => {
            let approval = profile
                .show_setup(app, &window)
                .map_err(|_| error("REMOTE_PAIRING_SETUP_BUSY"))?;
            while Instant::now() < wait_end && approval.pending() {
                binding.check_admitted(&caller)?;
                match verify_configured_host(&approval, address).await {
                    Ok(None) => tokio::time::sleep(Duration::from_millis(25)).await,
                    Ok(Some(HostEnrollmentResponse::List { .. })) => {
                        binding.check_admitted(&caller)?;
                        return Ok(Reply::SetupVerified);
                    }
                    Err(ControlFailure::Relay(EnrollmentError::Connection(
                        ConnectError::Admission,
                    )))
                    | Err(ControlFailure::Relay(EnrollmentError::Rejected)) => {
                        binding.check_admitted(&caller)?;
                        return Ok(Reply::RelayAuthorizationRequired);
                    }
                    _ => return Err(error("REMOTE_PAIRING_SETUP_FAILED")),
                }
            }
            approval.cancel();
            Ok(Reply::Declined)
        }
        Action::PublicRegistration { .. } => {
            let approval = profile
                .show_public_metadata(app, &window)
                .map_err(|_| error("REMOTE_PAIRING_SETUP_BUSY"))?;
            while Instant::now() < wait_end && approval.pending() {
                binding.check_admitted(&caller)?;
                if let Some(metadata) = approval.take() {
                    return Ok(Reply::PublicRegistration { metadata });
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            approval.cancel();
            Ok(Reply::Declined)
        }
    }
}
