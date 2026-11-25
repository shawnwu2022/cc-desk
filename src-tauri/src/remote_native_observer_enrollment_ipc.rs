//! Verified desktop setup IPC, independently authenticated to the existing
//! privileged native document. No session selection and no credentials cross
//! the renderer boundary. A successful setup proof isn't a data capability.
use super::{
    control::{verify_configured_host, ControlFailure},
    decode,
    owner::{DeviceTarget, Operation, OwnerService, Pending, Snapshot},
    setup::{OperatorHostProfileInput, PublicRegistrationMetadata, TrustedHostProfile},
    HostEpochs,
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
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Action {
    Available,
    Verify {
        endpoint: String,
    },
    PublicRegistration {
        endpoint: String,
    },
    Current {
        ticket: String,
    },
    Create {
        ticket: String,
        minutes: u16,
    },
    Cancel {
        ticket: String,
        invite_id: String,
    },
    Reject {
        ticket: String,
        target: Pending,
    },
    Approve {
        ticket: String,
        target: Pending,
        scope_choice_id: Option<String>,
    },
    Revoke {
        ticket: String,
        target: DeviceTarget,
    },
    StartSharing {
        ticket: String,
        target: DeviceTarget,
        runs: Vec<ShareRun>,
    },
    StopSharing {
        ticket: String,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ShareRun {
    run_id: String,
    generation: u32,
    label: String,
    role: Option<cc_desk_remote_record::grant::DataRole>,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum Reply {
    Available,
    ProfileRequired,
    RelayAuthorizationRequired,
    Verified {
        ticket: String,
        endpoint: String,
    },
    Snapshot {
        snapshot: Snapshot,
    },
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
    host_key_epoch: u64,
    client_key_epoch: u64,
    policy_epoch: u64,
    issuer_epoch: u64,
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
            epochs: HostEpochs {
                host_key: selected.host_key_epoch,
                client_key: selected.client_key_epoch,
                policy: selected.policy_epoch,
                issuer: selected.issuer_epoch,
            },
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
    owner_service: State<'_, OwnerService>,
    host_service: State<'_, Arc<crate::remote_host::RemoteHostService>>,
) -> Result<Reply, SafeError> {
    let (binding, caller, action) =
        runtime.admit_host_document::<Action, _>(&webview, request.headers(), request.body())?;
    let app = webview.app_handle();
    let window = webview.window();
    let action = match action {
        Action::Available => return Ok(Reply::Available),
        Action::Current { ticket } => {
            return run_owner(
                &owner_service,
                &ticket,
                &caller,
                &binding,
                app,
                &window,
                Operation::Current,
            )
            .await
        }
        Action::Create { ticket, minutes } if matches!(minutes, 1 | 5 | 10) => {
            return run_owner(
                &owner_service,
                &ticket,
                &caller,
                &binding,
                app,
                &window,
                Operation::Create(minutes * 60),
            )
            .await
        }
        Action::Cancel { ticket, invite_id } => {
            return run_owner(
                &owner_service,
                &ticket,
                &caller,
                &binding,
                app,
                &window,
                Operation::Cancel(invite_id),
            )
            .await
        }
        Action::Reject { ticket, target } => {
            return run_owner(
                &owner_service,
                &ticket,
                &caller,
                &binding,
                app,
                &window,
                Operation::Reject(target),
            )
            .await
        }
        Action::Approve {
            ticket,
            target,
            scope_choice_id: None,
        } => {
            return run_owner(
                &owner_service,
                &ticket,
                &caller,
                &binding,
                app,
                &window,
                Operation::Approve(target),
            )
            .await
        }
        Action::StartSharing {
            ticket,
            target,
            runs,
        } => {
            if runs.is_empty() || runs.len() > 4 {
                return Err(error("REMOTE_PAIRING_LOCKED"));
            }
            let owner = owner_service.get(&ticket, &caller, &binding)?;
            let targets = runs
                .into_iter()
                .map(|r| {
                    Ok(super::shares::Target {
                        run: runtime.admit_owner_share_run(&caller, r.run_id, r.generation)?,
                        label: r.label,
                        role: r
                            .role
                            .unwrap_or(cc_desk_remote_record::grant::DataRole::Observer),
                    })
                })
                .collect::<Result<Vec<_>, SafeError>>()?;
            return owner
                .start_sharing(target, targets, app, &window)
                .await
                .map(|snapshot| Reply::Snapshot { snapshot });
        }
        Action::StopSharing { ticket } => {
            let owner = owner_service.get(&ticket, &caller, &binding)?;
            return owner
                .stop_sharing()
                .await
                .map(|snapshot| Reply::Snapshot { snapshot });
        }
        Action::Revoke { ticket, target } => {
            return run_owner(
                &owner_service,
                &ticket,
                &caller,
                &binding,
                app,
                &window,
                Operation::Revoke(target),
            )
            .await
        }
        action @ (Action::Verify { .. } | Action::PublicRegistration { .. }) => action,
        _ => return Err(error("REMOTE_PAIRING_LOCKED")),
    };
    let endpoint = match &action {
        Action::Verify { endpoint } | Action::PublicRegistration { endpoint } => endpoint.clone(),
        _ => return Err(error("REMOTE_PAIRING_LOCKED")),
    };
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|_| error("REMOTE_PAIRING_SETUP_REQUIRED"))?;
    let install_dir = dir.clone();
    let selected = tokio::task::spawn_blocking(move || configured_profile(dir, &endpoint))
        .await
        .map_err(|_| error("REMOTE_PAIRING_SETUP_REQUIRED"))?;
    let (profile, address) = match selected {
        Ok(Some(profile)) => profile,
        Ok(None) | Err(_) => return Ok(Reply::ProfileRequired),
    };
    binding.check_admitted(&caller)?;
    let wait_end = Instant::now() + Duration::from_secs(30);
    match action {
        Action::Verify { .. } => {
            let approval = profile
                .clone()
                .show_setup(app, &window)
                .map_err(|_| error("REMOTE_PAIRING_SETUP_BUSY"))?;
            while Instant::now() < wait_end && approval.pending() {
                binding.check_admitted(&caller)?;
                match verify_configured_host(&approval, address).await {
                    Ok(None) => tokio::time::sleep(Duration::from_millis(25)).await,
                    Ok(Some(HostEnrollmentResponse::List { .. })) => {
                        binding.check_admitted(&caller)?;
                        let (ticket, endpoint) = owner_service
                            .install(
                                &binding,
                                caller,
                                profile,
                                address,
                                install_dir,
                                runtime.inner().clone(),
                                host_service.inner().clone(),
                            )
                            .await?;
                        return Ok(Reply::Verified { ticket, endpoint });
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
        _ => Err(error("REMOTE_PAIRING_LOCKED")),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_owner(
    service: &OwnerService,
    ticket: &str,
    caller: &crate::cli::snapshot::CallerIdentity,
    binding: &Arc<crate::cli::document::DocumentBinding<crate::cli::launch_service::NativeRun>>,
    app: &tauri::AppHandle,
    window: &tauri::Window,
    operation: Operation,
) -> Result<Reply, SafeError> {
    let owner = service.get(ticket, caller, binding)?;
    owner
        .execute(operation, app, window)
        .await
        .map(|snapshot| Reply::Snapshot { snapshot })
}
