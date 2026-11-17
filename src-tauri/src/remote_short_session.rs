//! 明确选择已确认设备的短会话产品入口；旧手动批准不迁移。
use crate::{
    cli::{
        document::DocumentBinding, launch_service::NativeRun, native_runtime::NativeRuntime,
        profiles::error, snapshot::CallerIdentity, types::SafeError,
    },
    remote_host::{unattended::preparing::PreparingRequest, RemoteHostService},
    remote_host_approval::unattended::{
        final_approval::FinalApproval, RequestedRole, UnattendedApproval, UnattendedPreparation,
    },
    remote_native_pairing_store::{
        Backend as PairingBackend, CurrentUserDpapi as PairingDpapi, ExpectedStore, PairingStore,
        Sealer as PairingSealer,
    },
    run_supervisor::NativeRunAdmission,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Weak},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    store_id: String,
    revision: String,
    device_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OperationRequest {
    pub(crate) operation_id: String,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum Phase {
    Empty,
    Preparing,
    Confirming,
    Starting,
    Accepted,
    Closing,
    Closed,
    Unknown,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Reply {
    phase: Phase,
    operation_id: Option<String>,
    limit_seconds: u8,
}
struct Flow<B: PairingBackend, P: PairingSealer> {
    request: Arc<PreparingRequest>,
    store: Arc<PairingStore<B, P>>,
    binding: Weak<DocumentBinding<NativeRun>>,
    caller: CallerIdentity,
    runtime: Arc<NativeRuntime>,
    preparation: Arc<UnattendedApproval>,
    final_approval: Option<Arc<FinalApproval>>,
    phase: Phase,
}
pub(crate) struct StartContext {
    pub(crate) runtime: Arc<NativeRuntime>,
    pub(crate) binding: Arc<DocumentBinding<NativeRun>>,
    pub(crate) caller: CallerIdentity,
    pub(crate) admission: NativeRunAdmission,
}
pub(crate) struct ShortSessionService<
    B: PairingBackend = crate::version_history::windows::pairing_store::WindowsPairingBackend,
    P: PairingSealer = PairingDpapi,
> {
    flow: Mutex<Option<Flow<B, P>>>,
    #[cfg(test)]
    network_fixture: Mutex<Option<TestNetwork>>,
}
impl<B: PairingBackend, P: PairingSealer> Default for ShortSessionService<B, P> {
    fn default() -> Self {
        Self {
            flow: Mutex::new(None),
            #[cfg(test)]
            network_fixture: Mutex::new(None),
        }
    }
}
fn failed() -> SafeError {
    error("REMOTE_SHORT_SESSION_UNAVAILABLE")
}
impl<B: PairingBackend + Send + Sync + 'static, P: PairingSealer + Send + Sync + 'static>
    ShortSessionService<B, P>
{
    fn request_with(
        &self,
        host: &Arc<RemoteHostService>,
        context: StartContext,
        text: &str,
        open: impl FnOnce() -> Result<Arc<PairingStore<B, P>>, SafeError>,
        show: impl FnOnce(
            UnattendedPreparation,
            &PairingStore<B, P>,
            &NativeRunAdmission,
        ) -> Result<Arc<UnattendedApproval>, SafeError>,
    ) -> Result<Reply, SafeError> {
        let StartContext {
            runtime,
            binding,
            caller,
            admission,
        } = context;
        let selection: Selection = serde_json::from_str(text).map_err(|_| failed())?;
        if text.len() > 1024 || selection.store_id.len() > 64 || selection.device_id.len() > 64 {
            return Err(failed());
        }
        let revision = selection.revision.parse::<u64>().map_err(|_| failed())?;
        if revision.to_string() != selection.revision {
            return Err(failed());
        }
        binding.check_admitted(&caller)?;
        admission.scope()?;
        let mut slot = self.flow.try_lock().ok_or_else(failed)?;
        if slot.is_some() {
            return Err(failed());
        }
        let request = host
            .unattended
            .reserve_preparation(runtime.clone(), admission)
            .map_err(|_| failed())?;
        let mut operation = request.begin().map_err(|_| failed())?;
        let admission = operation.admission().map_err(|_| failed())?;
        let store = open()?;
        let candidate = store
            .select_installed(
                &ExpectedStore {
                    store_id: selection.store_id,
                    revision,
                },
                &selection.device_id,
            )
            .map_err(|_| failed())?;
        let wall: u64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| failed())?
            .as_millis()
            .try_into()
            .map_err(|_| failed())?;
        let cutoff = wall.checked_add(60_000).ok_or_else(failed)?;
        let preparation = UnattendedPreparation::new(
            &store,
            candidate,
            admission,
            RequestedRole::Control,
            cutoff,
        )?;
        binding.check_admitted(&caller)?;
        if request.closed() {
            return Err(failed());
        }
        let approval = show(preparation, &store, admission)?;
        request
            .track_preparation(approval.clone())
            .map_err(|_| failed())?;
        *slot = Some(Flow {
            request,
            store,
            binding: Arc::downgrade(&binding),
            caller,
            runtime,
            preparation: approval,
            final_approval: None,
            phase: Phase::Preparing,
        });
        operation.complete();
        drop(operation);
        drop(slot);
        self.current(host)
    }
    fn advance_with<U: crate::remote_unattended_store::Sealer + Send + 'static>(
        &self,
        host: &Arc<RemoteHostService>,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        id: &str,
        show: impl FnOnce(
            crate::remote_host_approval::unattended::ApprovedPreparation,
            &PairingStore<B, P>,
            &NativeRunAdmission,
        ) -> Result<Arc<FinalApproval>, SafeError>,
        persist: impl FnOnce() -> Result<
            (
                crate::version_history::windows::unattended_store::WindowsUnattendedBackend,
                U,
            ),
            SafeError,
        >,
    ) -> Result<Reply, SafeError> {
        binding.check_admitted(caller)?;
        let mut slot = self.flow.try_lock().ok_or_else(failed)?;
        let flow = slot.as_mut().ok_or_else(failed)?;
        if flow.request.id() != id
            || &flow.caller != caller
            || !Weak::ptr_eq(&flow.binding, &Arc::downgrade(binding))
        {
            return Err(failed());
        }
        if matches!(flow.phase, Phase::Starting) {
            drop(slot);
            return self.current(host);
        }
        let mut operation = flow.request.begin().map_err(|_| failed())?;
        let admission = operation.admission().map_err(|_| failed())?;
        binding.check_admitted(caller)?;
        admission.scope()?;
        if flow.final_approval.is_none() {
            if let Some(approved) = flow.preparation.consume(&flow.store, admission)? {
                let final_approval = show(approved, &flow.store, admission)?;
                flow.request
                    .track_final(final_approval.clone())
                    .map_err(|_| failed())?;
                flow.final_approval = Some(final_approval);
                flow.phase = Phase::Confirming;
            } else if !flow.preparation.waiting() {
                flow.request.request_close();
                flow.phase = Phase::Closed;
            }
        } else {
            let approval = flow.final_approval.as_ref().ok_or_else(failed)?;
            if let Some(consent) = approval.consume(&flow.store, admission)? {
                binding.check_admitted(caller)?;
                if flow.request.closed() {
                    return Err(failed());
                }
                let (backend, sealer) = persist()?;
                let admission = operation.take_admission().map_err(|_| failed())?;
                let pending =
                    crate::remote_unattended_store::service::pending::PendingStart::new_network(
                        backend,
                        sealer,
                        consent,
                        admission,
                        flow.store.clone(),
                        host,
                    )
                    .map_err(|_| failed())?;
                #[cfg(test)]
                let pending = {
                    let mut pending = pending;
                    if let Some(fixture) = self.network_fixture.lock().take() {
                        pending.test_anonymous_network(
                            fixture.scalars,
                            fixture.opened,
                            fixture.accepted,
                        );
                    }
                    pending
                };
                host.unattended
                    .reserve_prepared(&flow.request, pending, flow.runtime.clone())
                    .map_err(|_| failed())?;
                flow.phase = Phase::Starting;
            } else if !approval.waiting() {
                flow.request.request_close();
                flow.phase = Phase::Closed;
            }
        }
        operation.complete();
        drop(operation);
        drop(slot);
        self.current(host)
    }
    pub(crate) fn status_for(
        &self,
        host: &RemoteHostService,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        runtime: &Arc<NativeRuntime>,
    ) -> Result<Reply, SafeError> {
        binding.check_admitted(caller)?;
        if !host.unattended.matches_exit_runtime(runtime) {
            return Err(failed());
        }
        let result = self.current(host)?;
        binding.check_admitted(caller)?;
        Ok(result)
    }
    pub(crate) fn close_for(
        &self,
        host: &RemoteHostService,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        runtime: &Arc<NativeRuntime>,
        id: &str,
    ) -> Result<(), SafeError> {
        binding.check_admitted(caller)?;
        if !host.unattended.matches_exit_runtime(runtime) {
            return Err(failed());
        }
        self.close(host, id)
    }
    pub(crate) fn current(&self, host: &RemoteHostService) -> Result<Reply, SafeError> {
        let slot = self.flow.try_lock();
        let Some(slot) = slot else {
            let (id, closed) = host.unattended.preparation_status().ok_or_else(failed)?;
            return Ok(Reply {
                phase: if closed {
                    Phase::Closing
                } else {
                    Phase::Preparing
                },
                operation_id: Some(id),
                limit_seconds: 10,
            });
        };
        let Some(flow) = slot.as_ref() else {
            if let Some((id, closed)) = host.unattended.preparation_status() {
                let phase = if closed {
                    if host.unattended.observe().known() {
                        Phase::Closed
                    } else {
                        Phase::Closing
                    }
                } else {
                    Phase::Preparing
                };
                return Ok(Reply {
                    phase,
                    operation_id: Some(id),
                    limit_seconds: 10,
                });
            }
            return Ok(Reply {
                phase: Phase::Empty,
                operation_id: None,
                limit_seconds: 10,
            });
        };
        let observation = host.unattended.observe();
        let phase =
            if observation.failed || observation.durable.is_some_and(|result| result.is_err()) {
                Phase::Unknown
            } else if flow.request.closed() {
                if observation.known() {
                    Phase::Closed
                } else {
                    Phase::Closing
                }
            } else if matches!(flow.phase, Phase::Starting) {
                if observation.known() {
                    Phase::Closed
                } else if matches!(host.unattended.start_result(), Some(Ok(()))) {
                    Phase::Accepted
                } else {
                    Phase::Starting
                }
            } else {
                flow.phase
            };
        Ok(Reply {
            phase,
            operation_id: Some(flow.request.id().to_owned()),
            limit_seconds: 10,
        })
    }
    pub(crate) fn close(&self, host: &RemoteHostService, id: &str) -> Result<(), SafeError> {
        host.unattended.close_preparation(id).map_err(|_| failed())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadRequest {}
impl OperationRequest {
    pub(crate) fn validate(&self) -> Result<(), SafeError> {
        if uuid::Uuid::parse_str(&self.operation_id)
            .ok()
            .is_none_or(|id| id.to_string() != self.operation_id)
        {
            return Err(failed());
        }
        Ok(())
    }
}

impl ShortSessionService {
    pub(crate) fn request<R: tauri::Runtime>(
        &self,
        host: &Arc<RemoteHostService>,
        context: StartContext,
        text: &str,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<Reply, SafeError> {
        self.request_with(
            host,
            context,
            text,
            || {
                crate::remote_pairing_product::open()
                    .map(Arc::new)
                    .map_err(|_| failed())
            },
            |preparation, store, admission| {
                preparation
                    .show(store, admission, app, window)
                    .map(Arc::new)
            },
        )
    }
    pub(crate) fn advance<R: tauri::Runtime>(
        &self,
        host: &Arc<RemoteHostService>,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        id: &str,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<Reply, SafeError> {
        self.advance_with(
            host,
            binding,
            caller,
            id,
            |approved, store, admission| {
                FinalApproval::show_network(approved, store, admission, app, window).map(Arc::new)
            },
            || {
                let parent = dirs::home_dir().ok_or_else(failed)?.join(".cc-box");
                use crate::version_history::windows::unattended_store::WindowsUnattendedBackend;
                let backend = match WindowsUnattendedBackend::open_existing(&parent) {
                    Ok(value) => value,
                    Err(crate::remote_unattended_store::StoreError::Missing) => {
                        WindowsUnattendedBackend::create_new(&parent).map_err(|_| failed())?
                    }
                    Err(_) => return Err(failed()),
                };
                Ok((backend, crate::remote_unattended_store::CurrentUserDpapi))
            },
        )
    }
}

#[cfg(test)]
pub(crate) struct TestNetwork {
    pub(crate) scalars: [[u8; 32]; 2],
    pub(crate) opened: Arc<[std::sync::atomic::AtomicUsize; 2]>,
    pub(crate) accepted: tokio::sync::oneshot::Sender<()>,
}
#[cfg(test)]
impl<B: PairingBackend + Send + Sync + 'static, P: PairingSealer + Send + Sync + 'static>
    ShortSessionService<B, P>
{
    pub(crate) fn test_request(
        &self,
        host: &Arc<RemoteHostService>,
        context: StartContext,
        text: &str,
        store: Arc<PairingStore<B, P>>,
    ) -> Result<String, SafeError> {
        let result = self.request_with(
            host,
            context,
            text,
            || Ok(store),
            |preparation, _, _| Ok(Arc::new(UnattendedApproval::test_pending(preparation))),
        )?;
        result.operation_id.ok_or_else(failed)
    }
    pub(crate) fn test_answer(&self, final_stage: bool, yes: bool) {
        let slot = self.flow.lock();
        let flow = slot.as_ref().unwrap();
        if final_stage {
            flow.final_approval.as_ref().unwrap().test_answer(yes)
        } else {
            flow.preparation.test_answer(yes, false)
        }
    }
    pub(crate) fn test_advance<U: crate::remote_unattended_store::Sealer + Send + 'static>(
        &self,
        host: &Arc<RemoteHostService>,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        id: &str,
        persist: impl FnOnce() -> Result<
            (
                crate::version_history::windows::unattended_store::WindowsUnattendedBackend,
                U,
            ),
            SafeError,
        >,
    ) -> Result<Reply, SafeError> {
        self.advance_with(
            host,
            binding,
            caller,
            id,
            |approved, store, admission| {
                FinalApproval::test_pending_network(approved, store, admission).map(Arc::new)
            },
            persist,
        )
    }
    pub(crate) fn test_network(&self, fixture: TestNetwork) {
        *self.network_fixture.lock() = Some(fixture);
    }
}
