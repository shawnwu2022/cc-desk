//! Source-side composition from the authenticated preparation winner. The
//! measured payload permit is mandatory before persistent switch work begins.
use super::{
    context::ContextJournal,
    files::{ComponentName, PrivateDirectory},
    manager_bundle::ManagerBundle,
    manager_handoff::publish_initial_handoff,
    manager_process::PreparedManager,
    package::RetainedPackage,
    pre_context_abort::{publish_private_abort, publish_source_transition, PrivateAbortEvidence},
    registration_state::HeldRegistrationState,
    scope::{ConfiguredInventory, ObservedPath},
    security::CurrentUser,
    shortcuts::HeldProductShortcuts,
    source_lifecycle::SourceHandoff,
    space::SpaceAdmission,
    startup::{OrdinaryStartup, TransactionDataRoot},
};
use crate::{
    cli::{
        document::DocumentBinding, launch_service::NativeRun, profiles::error, types::SafeError,
    },
    version_history::{
        download::PreparedHandoff,
        journal::{CapacityPlan, JournalBinding, JournalStore},
        maintenance::{process_admissions, FrozenAdmissions},
        payload_policy::PayloadAdmission,
        policy::PRODUCT_IDENTIFIER,
        verified_package::{sha256, VerifiedPackage},
    },
};
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, sync::Arc};
use tauri::WebviewWindow;
fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_HANDOFF_CHANGED")
}
fn root_observation(root: &ObservedPath) -> Result<serde_json::Value, SafeError> {
    root.recheck().map_err(blocked)?;
    if let Some(directory) = root.directory() {
        return Ok(serde_json::json!({"present":directory.identity()}));
    }
    let (parent, suffix) = root
        .absent_location()
        .ok_or_else(|| error("HISTORY_ROOT_CHANGED"))?;
    Ok(
        serde_json::json!({"absentParent":parent.identity(),"suffix":suffix.iter().map(|name|name.os_string().encode_wide().collect::<Vec<_>>()).collect::<Vec<_>>()}),
    )
}
/// Candidate admission is held before its UUID is issued to the source. It
/// releases only an uncommitted freeze when the reservation cannot complete.
pub(crate) struct SourcePreflight {
    payload: Option<PayloadAdmission>,
    frozen: Option<FrozenAdmissions>,
}
impl SourcePreflight {
    pub(crate) fn admit(package: &VerifiedPackage, transaction: &str) -> Result<Self, SafeError> {
        super::manager_process::require_job_free_source()
            .map_err(|_| error("HISTORY_SOURCE_JOB_UNSUPPORTED"))?;
        let payload = PayloadAdmission::admit_begin(package)?;
        let frozen = process_admissions().freeze(transaction)?;
        let admission = Self {
            payload: Some(payload),
            frozen: Some(frozen),
        };
        admission
            .frozen
            .as_ref()
            .expect("source preflight owns freeze")
            .verify_quiescent(transaction)?;
        Ok(admission)
    }
    fn take(mut self) -> Result<(PayloadAdmission, FrozenAdmissions), SafeError> {
        Ok((
            self.payload
                .take()
                .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?,
            self.frozen
                .take()
                .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?,
        ))
    }
}
impl Drop for SourcePreflight {
    fn drop(&mut self) {
        if let Some(frozen) = self.frozen.take() {
            let _ = frozen.release_review();
        }
    }
}
/// Runs inside a blocking worker; registry observations never enter Tauri's
/// cross-thread managed state. The source event loop remains available.
pub(crate) fn begin(
    transfer: PreparedHandoff,
    preflight: SourcePreflight,
    startup: Arc<OrdinaryStartup>,
    window: WebviewWindow,
    binding: Arc<DocumentBinding<NativeRun>>,
    headers: tauri::http::HeaderMap,
) -> Result<(), SafeError> {
    transfer.check()?;
    preflight
        .payload
        .as_ref()
        .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?
        .verify_selection(transfer.selection())?;
    let (payload, frozen) = preflight.take()?;
    let transaction = transfer.transaction_id().to_owned();
    let source_ui = tauri::async_runtime::block_on(SourceHandoff::capture(
        window,
        binding,
        &headers,
        transaction.clone(),
        frozen,
    ))?;
    let result = prepare_and_handoff(&transfer, &payload, &startup, &source_ui);
    if result.is_err() {
        // This succeeds only while review remains uncommitted. Published
        // handoffs require the separately verified pre-context abort outcome.
        if let Ok(released) = tauri::async_runtime::block_on(source_ui.cancel_review()) {
            transfer.record_unstarted(&released)?;
        }
    }
    result
}
fn prepare_and_handoff(
    transfer: &PreparedHandoff,
    payload: &PayloadAdmission,
    startup: &OrdinaryStartup,
    source_ui: &Arc<SourceHandoff>,
) -> Result<(), SafeError> {
    transfer.check()?;
    source_ui.verify_before_close()?;
    let source = startup.capture_switch_source()?;
    let inventory = ConfiguredInventory::capture().map_err(blocked)?;
    let udf = ObservedPath::from_directory(source_ui.udf().clone()).map_err(blocked)?;
    let installed =
        ObservedPath::from_directory(source.installation().directory().clone()).map_err(blocked)?;
    let control_root = ObservedPath::from_directory(startup.control().root().directory().clone())
        .map_err(blocked)?;
    inventory
        .require_disjoint(&[&udf, &installed, &control_root])
        .map_err(blocked)?;
    let registration = HeldRegistrationState::capture(source.installation()).map_err(blocked)?;
    let shortcuts = HeldProductShortcuts::capture_current_user().map_err(blocked)?;
    let space = SpaceAdmission::source_handoff(
        startup.control().root().clone(),
        source.installation().directory(),
        source.bundle(),
        transfer,
        payload,
    )?;
    registration.recheck().map_err(blocked)?;
    shortcuts.verify().map_err(blocked)?;
    inventory.recheck().map_err(blocked)?;
    transfer.check()?;
    source_ui.verify_before_close()?;
    let control = startup.control().acquire_control()?;
    startup
        .shared()
        .verify_root(startup.control().root())
        .map_err(blocked)?;
    space.verify()?;
    super::manager_process::require_job_free_source()
        .map_err(|_| error("HISTORY_SOURCE_JOB_UNSUPPORTED"))?;
    source_ui.retain_for_recovery()?;
    let data = Arc::new(TransactionDataRoot::create(
        startup.control().clone(),
        &control,
        transfer.transaction_id(),
    )?);
    let recovery =
        ObservedPath::from_directory(data.root().directory().clone()).map_err(blocked)?;
    inventory
        .require_disjoint(&[&udf, &installed, &recovery])
        .map_err(blocked)?;
    // Scope locators are derived from held backend observations. Full M0 and
    // final configured exclusions are freshly captured after actual source exit.
    let roots = sha256(
        &serde_json::to_vec(&(
            root_observation(inventory.desk_root())?,
            source_ui.udf().identity(),
            source.installation().directory().identity(),
            data.root().directory().identity(),
        ))
        .map_err(blocked)?,
    );
    let user = CurrentUser::capture().map_err(blocked)?;
    let journal_binding = JournalBinding {
        transaction_id: transfer.transaction_id().into(),
        source_context: uuid::Uuid::new_v4().to_string(),
        target_context: uuid::Uuid::new_v4().to_string(),
        user_installation: sha256(
            &serde_json::to_vec(&(
                PRODUCT_IDENTIFIER,
                user.sid_text(),
                source
                    .installation()
                    .original_path()
                    .as_os_str()
                    .encode_wide()
                    .collect::<Vec<_>>(),
            ))
            .map_err(blocked)?,
        ),
        source_bundle: source
            .bundle()
            .manifest()
            .logical_digest()
            .map_err(blocked)?,
        target_package: transfer.selection().installer().sha256().into(),
        target_payload: payload.inventory_digest().into(),
        roots,
    };
    let mut store = JournalStore::create_windows_transaction(
        startup.control().root().clone(),
        transfer.transaction_id(),
    )?;
    // Bound record/dependency plans are tightened by each actual copy/return
    // executor before effects; this genesis reserves separate recovery space.
    store.initialize(
        journal_binding.clone(),
        CapacityPlan::for_effects(4_000, 6_000, 512, 2048)?,
    )?;
    publish_source_transition(startup.control(), &control, &mut store, &journal_binding)?;
    let private_copy = (|| {
        let package_root = Arc::new(
            PrivateDirectory::create_new(
                data.root().directory().clone(),
                ComponentName::new(OsStr::new("package")).map_err(blocked)?,
                &user,
            )
            .map_err(blocked)?,
        );
        let package = RetainedPackage::retain(transfer, package_root)?;
        let mut journal = ContextJournal::new_precommit(
            &mut store,
            startup.control().root().clone(),
            &control,
            startup.shared(),
            journal_binding.clone(),
            0,
        )
        .map_err(blocked)?;
        let bundle = Arc::new(
            ManagerBundle::prepare(
                source.installation(),
                source.bundle(),
                data.root().clone(),
                transfer.transaction_id(),
                &user,
                &mut journal,
            )
            .map_err(blocked)?,
        );
        let generation = journal.generation();
        drop(journal);
        source_ui.verify_before_close()?;
        registration.recheck().map_err(blocked)?;
        shortcuts.verify().map_err(blocked)?;
        inventory.recheck().map_err(blocked)?;
        transfer.check()?;
        Ok::<_, SafeError>((bundle, package, generation))
    })();
    let (bundle, package, generation) = match private_copy {
        Ok(copied) => copied,
        Err(failure) => {
            let evidence = PrivateAbortEvidence::capture(
                journal_binding.clone(),
                (&source, &inventory, &registration, &shortcuts),
                (source_ui, startup, &data),
            )?;
            let outcome = publish_private_abort(evidence, &mut store, &control)?;
            source_ui.finish_private_abort(&outcome)?;
            transfer.record_verified_abort(&outcome)?;
            return Err(failure);
        }
    };
    let package = transfer.complete(package)?;
    let mut manager = PreparedManager::create_suspended(
        bundle.clone(),
        source.installation(),
        &package,
        startup.control().root().clone(),
        &control,
        startup.shared(),
        &user,
    )
    .map_err(blocked)?;
    let resume = manager.prepare_resume(&control, &user).map_err(blocked)?;
    let published = publish_initial_handoff(
        store,
        startup.control(),
        control,
        &data,
        &bundle,
        resume,
        (&journal_binding, generation),
    )?;
    source_ui.commit_published(&published)?;
    manager.resume_once(published, &user).map_err(blocked)?;
    // Kernel/receipt checks govern completion. A wait interval never promotes
    // timeout or missing acknowledgement into success or launch replay.
    loop {
        if let Some(ready) = manager.observe_ready(&user).map_err(blocked)? {
            tauri::async_runtime::block_on(source_ui.close_after_manager_ready(ready, &data))?;
            let driver =
                source_ui.start_exit_driver(startup.control().clone(), data, journal_binding)?;
            return driver.join().map_err(blocked)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
