//! Original source controller ownership. UI-only COM observations remain on
//! the event-loop thread until matched browser exits have been journaled.
use super::{
    files::{ComponentName, Directory},
    lease::ExclusiveLease,
    manager_process::{launch_path, ManagerReadyReceipt},
    process::ExactProcess,
    security::CurrentUser,
    startup::{InstallationControl, TransactionDataRoot},
    webview::{
        actual_udf, SourceExitFenceEvidence, SourceWebViews, WebViewExitReceipt,
        WebViewExitReference,
    },
};
use crate::{
    cli::{
        document::{DocumentBinding, DocumentHandoffPin, DOCUMENT_HEADER},
        launch_service::NativeRun,
        profiles::error,
        snapshot::CallerIdentity,
        types::SafeError,
    },
    version_history::{
        journal::{JournalBinding, JournalEvent, JournalStore, ManifestRole},
        maintenance::FrozenAdmissions,
    },
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::{Manager, WebviewWindow};

thread_local! { static SOURCE_VIEWS: RefCell<Option<UiSource>> = const { RefCell::new(None) }; }
struct UiSource {
    owner: Arc<SourceHandoff>,
    views: SourceWebViews,
    ready: Option<ManagerReadyReceipt>,
    receipt: Option<WebViewExitReceipt>,
    failed: bool,
}
pub(crate) struct SourceHandoff {
    transaction: String,
    caller: CallerIdentity,
    binding: Arc<DocumentBinding<NativeRun>>,
    headers: tauri::http::HeaderMap,
    pin: Mutex<Option<DocumentHandoffPin<NativeRun>>>,
    frozen: Mutex<Option<FrozenAdmissions>>,
    source: ExactProcess,
    window: WebviewWindow,
    udf: Arc<Directory>,
    close_started: AtomicBool,
    committed: AtomicBool,
    driver_started: AtomicBool,
    transition: Mutex<()>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceHandoffExitManifest {
    schema: u32,
    binding: JournalBinding,
    browser: WebViewExitReference,
    udf_selector: String,
}
pub(crate) struct SourceHandoffTerminal {
    binding: JournalBinding,
    exit: SourceExitFenceEvidence,
    udf: Arc<Directory>,
}
impl SourceHandoffTerminal {
    pub(crate) fn verify(&self, binding: &JournalBinding) -> Result<(), SafeError> {
        if &self.binding != binding {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        self.exit.verify().map_err(blocked)?;
        self.udf.recheck().map_err(blocked)
    }
    pub(crate) fn exit(&self) -> &SourceExitFenceEvidence {
        &self.exit
    }
    pub(crate) fn udf(&self) -> &Arc<Directory> {
        &self.udf
    }
    pub(crate) fn admit(
        store: &mut JournalStore,
        installation: &InstallationControl,
        exclusive: &ExclusiveLease,
        binding: &JournalBinding,
        data: &TransactionDataRoot,
        host: ExactProcess,
    ) -> Result<Self, SafeError> {
        data.verify_installation(installation)?;
        exclusive
            .verify_root(installation.root())
            .map_err(blocked)?;
        if data.transaction_id() != binding.transaction_id {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let inspection = store.inspect(binding)?;
        if inspection.blocked {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        let journal = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        store.verify_windows_binding(installation.root(), binding, journal.generation())?;
        let digest = journal
            .manifest(ManifestRole::SourceHandoffExit)
            .ok_or_else(|| error("HISTORY_SOURCE_EXIT_UNCONFIRMED"))?;
        let manifest: SourceHandoffExitManifest =
            serde_json::from_slice(&store.read_manifest(digest)?).map_err(blocked)?;
        if manifest.schema != 1 || &manifest.binding != binding {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let receipt = WebViewExitReceipt::reopen(
            data.root().clone(),
            &manifest.browser,
            &CurrentUser::capture().map_err(blocked)?,
        )
        .map_err(blocked)?;
        let exit = receipt
            .after_host_exit(host)
            .map_err(blocked)?
            .release_image_for_fence()
            .map_err(blocked)?;
        // The locator was obtained from the source's actual controller UDF
        // handle. It only selects a slot: exact observed identity and DELETE-
        // capable no-delete-sharing admission must still succeed after exit.
        let selector = std::path::Path::new(&manifest.udf_selector);
        let parent = Directory::open_absolute(
            selector
                .parent()
                .ok_or_else(|| error("HISTORY_ROOT_CHANGED"))?,
        )
        .map_err(blocked)?;
        let udf = parent
            .open_for_rename(
                ComponentName::new(
                    selector
                        .file_name()
                        .ok_or_else(|| error("HISTORY_ROOT_CHANGED"))?,
                )
                .map_err(blocked)?,
                exit.udf_identity(),
            )
            .map_err(blocked)?;
        if udf.identity() != exit.udf_identity() {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        let evidence = Self {
            binding: binding.clone(),
            exit,
            udf,
        };
        exclusive
            .verify_root(installation.root())
            .map_err(blocked)?;
        evidence.verify(binding)?;
        Ok(evidence)
    }
}
/// Nonserializable permission exists only after the live frozen ledger and
/// matched browser receipt were durably bound to this source's journal.
pub(crate) struct SourceExitPermission {
    caller: CallerIdentity,
    transaction: String,
    _receipt: WebViewExitReceipt,
}
impl SourceExitPermission {
    pub(crate) fn verify_document(
        &self,
        caller: &CallerIdentity,
        transaction: &str,
    ) -> Result<(), SafeError> {
        if caller != &self.caller || transaction != self.transaction {
            return Err(error("FORBIDDEN"));
        }
        self._receipt.reference().map_err(blocked)?;
        Ok(())
    }
}
fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_SOURCE_EXIT_UNCONFIRMED")
}
struct CaptureFreeze(Option<FrozenAdmissions>);
impl Drop for CaptureFreeze {
    fn drop(&mut self) {
        if let Some(frozen) = self.0.take() {
            let _ = frozen.release_review();
        }
    }
}
impl SourceHandoff {
    pub(crate) fn start_exit_driver(
        self: &Arc<Self>,
        installation: Arc<InstallationControl>,
        data: Arc<TransactionDataRoot>,
        binding: JournalBinding,
    ) -> Result<std::thread::JoinHandle<Result<(), SafeError>>, SafeError> {
        if !self.close_started.load(Ordering::SeqCst) || binding.transaction_id != self.transaction
        {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        if self.driver_started.swap(true, Ordering::SeqCst) {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let owner = self.clone();
        let app = self.window.app_handle().clone();
        std::thread::Builder::new()
            .name("version-source-exit".into())
            .spawn(move || loop {
                let (send, receive) = std::sync::mpsc::sync_channel(1);
                let owner = owner.clone();
                let installation = installation.clone();
                let data = data.clone();
                let binding = binding.clone();
                app.run_on_main_thread(move || {
                    let _ = send.send(owner.persist_exit_on_ui(&installation, &data, &binding));
                })
                .map_err(blocked)?;
                match receive.recv().map_err(blocked)?? {
                    true => return Ok(()),
                    false => std::thread::sleep(std::time::Duration::from_millis(200)),
                }
            })
            .map_err(blocked)
    }
    pub(crate) async fn capture(
        window: WebviewWindow,
        binding: Arc<DocumentBinding<NativeRun>>,
        supplied_headers: &tauri::http::HeaderMap,
        transaction: String,
        frozen: FrozenAdmissions,
    ) -> Result<Arc<Self>, SafeError> {
        let mut frozen = CaptureFreeze(Some(frozen));
        let caller = binding.admit_window(&window, supplied_headers)?;
        frozen
            .0
            .as_ref()
            .expect("capture freeze")
            .verify_quiescent(&transaction)?;
        let pin = binding.pin_handoff(&caller, &transaction)?;
        let mut headers = tauri::http::HeaderMap::new();
        headers.insert(DOCUMENT_HEADER, supplied_headers[DOCUMENT_HEADER].clone());
        let retained = window.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        window
            .with_webview(move |platform| {
                let result = (|| {
                    if retained.app_handle().webviews().len() != 1 || retained.label() != "main" {
                        return Err(error("HISTORY_SOURCE_CONTROLLER_SET_CHANGED"));
                    }
                    if binding.admit_window(&retained, &headers)? != caller {
                        return Err(error("FORBIDDEN"));
                    }
                    pin.verify()?;
                    frozen
                        .0
                        .as_ref()
                        .expect("capture freeze")
                        .verify_quiescent(&transaction)?;
                    SOURCE_VIEWS.with(|cell| {
                        if cell.borrow().is_some() {
                            Err(error("HISTORY_HANDOFF_CHANGED"))
                        } else {
                            Ok(())
                        }
                    })?;
                    let environment = platform.environment();
                    let udf = actual_udf(&environment).map_err(blocked)?;
                    let views = SourceWebViews::capture(
                        vec![(environment, platform.controller())],
                        udf.clone(),
                    )
                    .map_err(blocked)?;
                    let source =
                        ExactProcess::capture_observed(std::process::id()).map_err(blocked)?;
                    let owner = Arc::new(Self {
                        transaction,
                        caller,
                        binding,
                        headers,
                        pin: Mutex::new(Some(pin)),
                        frozen: Mutex::new(frozen.0.take()),
                        source,
                        window: retained,
                        udf,
                        close_started: AtomicBool::new(false),
                        committed: AtomicBool::new(false),
                        driver_started: AtomicBool::new(false),
                        transition: Mutex::new(()),
                    });
                    SOURCE_VIEWS.with(|cell| {
                        *cell.borrow_mut() = Some(UiSource {
                            owner: owner.clone(),
                            views,
                            ready: None,
                            receipt: None,
                            failed: false,
                        })
                    });
                    Ok(owner)
                })();
                let _ = send.send(result);
            })
            .map_err(blocked)?;
        receive.await.map_err(blocked)?
    }
    fn verify_frozen(&self) -> Result<(), SafeError> {
        self.frozen
            .lock()
            .as_ref()
            .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?
            .verify_quiescent(&self.transaction)
    }
    pub(crate) fn commit_published(
        &self,
        published: &super::manager_handoff::PublishedManagerHandoff,
    ) -> Result<(), SafeError> {
        self.verify_before_close()?;
        let _transition = self.transition.lock();
        if published.transaction() != self.transaction {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        self.frozen
            .lock()
            .as_mut()
            .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?
            .mark_committed()?;
        self.pin
            .lock()
            .as_mut()
            .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?
            .commit_published(published)?;
        self.committed.store(true, Ordering::SeqCst);
        Ok(())
    }
    pub(crate) async fn cancel_review(self: &Arc<Self>) -> Result<(), SafeError> {
        if self.committed.load(Ordering::SeqCst) || self.close_started.load(Ordering::SeqCst) {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        let owner = self.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        self.window
            .run_on_main_thread(move || {
                let result = SOURCE_VIEWS.with(|cell| {
                    let _transition = owner.transition.lock();
                    let mut slot = cell.borrow_mut();
                    let held = slot
                        .as_ref()
                        .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?;
                    if !Arc::ptr_eq(&held.owner, &owner)
                        || held.ready.is_some()
                        || owner.committed.load(Ordering::SeqCst)
                    {
                        return Err(error("HISTORY_RECOVERY_REQUIRED"));
                    }
                    slot.take();
                    owner
                        .frozen
                        .lock()
                        .take()
                        .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?
                        .release_review()?;
                    owner
                        .pin
                        .lock()
                        .take()
                        .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?
                        .release_review()
                });
                let _ = send.send(result);
            })
            .map_err(blocked)?;
        receive.await.map_err(blocked)?
    }
    pub(crate) fn udf(&self) -> &Arc<Directory> {
        &self.udf
    }
    pub(crate) fn verify_before_close(&self) -> Result<(), SafeError> {
        if self.binding.admit_window(&self.window, &self.headers)? != self.caller {
            return Err(error("FORBIDDEN"));
        }
        self.pin
            .lock()
            .as_ref()
            .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?
            .verify()?;
        self.verify_frozen()?;
        if self.window.app_handle().webviews().len() != 1 {
            return Err(error("HISTORY_SOURCE_CONTROLLER_SET_CHANGED"));
        }
        Ok(())
    }
    /// UI remains open until this exact manager supplies a live readiness
    /// receipt. This method closes each retained controller at most once.
    pub(crate) async fn close_after_manager_ready(
        self: &Arc<Self>,
        ready: ManagerReadyReceipt,
        data: &TransactionDataRoot,
    ) -> Result<(), SafeError> {
        self.verify_before_close()?;
        data.verify()?;
        if data.transaction_id() != self.transaction {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let data_root = data.root().clone();
        ready
            .verify_for_source(&self.transaction, &self.source, data.root())
            .map_err(blocked)?;
        if !self.committed.load(Ordering::SeqCst) {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        if self.close_started.swap(true, Ordering::SeqCst) {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let owner = self.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        self.window
            .run_on_main_thread(move || {
                let result = SOURCE_VIEWS.with(|cell| {
                    let mut held = cell.borrow_mut();
                    let held = held
                        .as_mut()
                        .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?;
                    if !Arc::ptr_eq(&held.owner, &owner) || held.failed || held.ready.is_some() {
                        return Err(error("HISTORY_HANDOFF_CHANGED"));
                    }
                    owner.verify_before_close()?;
                    ready
                        .verify_for_source(&owner.transaction, &owner.source, &data_root)
                        .map_err(blocked)?;
                    held.ready = Some(ready);
                    held.views.close_controllers().map_err(|failure| {
                        held.failed = true;
                        blocked(failure)
                    })
                });
                let _ = send.send(result);
            })
            .map_err(blocked)?;
        receive.await.map_err(blocked)?
    }
    /// Invoke on the UI loop until it returns true. None of the waits/ticks
    /// manufacture completion: only actual matched browser events permit it.
    pub(crate) fn persist_exit_on_ui(
        self: &Arc<Self>,
        installation: &InstallationControl,
        data: &TransactionDataRoot,
        journal_binding: &JournalBinding,
    ) -> Result<bool, SafeError> {
        SOURCE_VIEWS.with(|cell| {
            let mut slot = cell.borrow_mut();
            let held = slot
                .as_mut()
                .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?;
            if !Arc::ptr_eq(&held.owner, self)
                || held.failed
                || !self.close_started.load(Ordering::SeqCst)
                || journal_binding.transaction_id != self.transaction
                || data.transaction_id() != self.transaction
            {
                return Err(error("HISTORY_HANDOFF_CHANGED"));
            }
            held.ready
                .as_ref()
                .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?
                .verify_for_source(&self.transaction, &self.source, data.root())
                .map_err(blocked)?;
            self.verify_frozen()?;
            if held.receipt.is_none() {
                held.receipt = held
                    .views
                    .persist_when_exited(
                        data.root().clone(),
                        &CurrentUser::capture().map_err(blocked)?,
                    )
                    .map_err(blocked)?;
            }
            let Some(receipt) = held.receipt.as_ref() else {
                return Ok(false);
            };
            receipt
                .verify_live_source(&self.source, &self.udf)
                .map_err(blocked)?;
            let control = installation.acquire_control()?;
            let mut store = JournalStore::open_windows_transaction(
                installation.root().clone(),
                &self.transaction,
            )?;
            store.bind_existing(journal_binding)?;
            let inspection = store.inspect(journal_binding)?;
            if inspection.blocked {
                return Err(error("HISTORY_RECOVERY_REQUIRED"));
            }
            let journal = inspection
                .last_valid
                .as_ref()
                .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
            if journal.manifest(ManifestRole::SourceHandoffExit).is_some() {
                return Err(error("HISTORY_RECONCILIATION_REQUIRED"));
            }
            let bytes = serde_json::to_vec(&SourceHandoffExitManifest {
                schema: 1,
                binding: journal_binding.clone(),
                browser: receipt.reference().map_err(blocked)?,
                udf_selector: launch_path(self.udf.raw())
                    .map_err(blocked)?
                    .into_string()
                    .map_err(blocked)?,
            })
            .map_err(blocked)?;
            held.failed = true;
            let digest = store.retain_manifest(&bytes)?;
            self.verify_frozen()?;
            control.verify_root(installation.root()).map_err(blocked)?;
            // Any write-boundary failure forbids replay. The durable browser
            // record remains present for typed recovery; the source stays alive.
            store.append(
                journal.generation(),
                JournalEvent::Manifest {
                    role: ManifestRole::SourceHandoffExit,
                    digest,
                },
            )?;
            let permission = SourceExitPermission {
                caller: self.caller.clone(),
                transaction: self.transaction.clone(),
                _receipt: held.receipt.take().expect("observed receipt"),
            };
            self.pin
                .lock()
                .take()
                .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?
                .finish_source_handoff(&permission)?;
            self.window.app_handle().exit(0);
            Ok(true)
        })
    }
}
