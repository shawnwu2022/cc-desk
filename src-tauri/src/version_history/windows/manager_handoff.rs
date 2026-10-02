//! Protected initial-child selector. CLI contributes only the canonical UUID;
//! paths, process identities and record references come from the held journal.
use super::{
    durability::MarkerStore,
    lease::ControlLease,
    manager_bundle::{ManagerBundle, ManagerRecordReference},
    manager_process::{ManagerChildAdmission, ManagerResumeAdmission},
    security::CurrentUser,
    startup::{InstallationControl, TransactionDataReference, TransactionDataRoot},
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        catalog::CatalogService,
        journal::{JournalBinding, JournalEvent, JournalPhase, JournalStore, ManifestRole},
        maintenance::ActiveContextMarker,
        manager_entry::ManagerRequest,
    },
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoffManifest {
    schema: u32,
    binding: JournalBinding,
    data: TransactionDataReference,
    bundle: ManagerRecordReference,
    resume: ManagerRecordReference,
}
fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_HANDOFF_CHANGED")
}
/// One-use handoff publication. This constructor owns and drops the actual
/// journal writer and control lease after validating the published checkpoint.
/// No serialized record or caller-supplied digest can construct this token.
pub(crate) struct PublishedManagerHandoff {
    resume: ManagerResumeAdmission,
}
impl PublishedManagerHandoff {
    pub(crate) fn transaction(&self) -> &str {
        self.resume.transaction()
    }
    pub(super) fn admission(&self) -> &ManagerResumeAdmission {
        &self.resume
    }
}
/// The source keeps its separate shared lifetime lease. This function consumes
/// only its control/writer ownership, allowing the child to re-admit the log.
pub(crate) fn publish_initial_handoff(
    mut store: JournalStore,
    installation: &InstallationControl,
    control: ControlLease,
    data: &TransactionDataRoot,
    bundle: &ManagerBundle,
    resume: ManagerResumeAdmission,
    binding: &JournalBinding,
    generation: u64,
) -> Result<PublishedManagerHandoff, SafeError> {
    control.verify_root(installation.root()).map_err(blocked)?;
    store.verify_windows_binding(installation.root(), binding, generation)?;
    data.verify()?;
    bundle
        .verify(&CurrentUser::capture().map_err(blocked)?)
        .map_err(blocked)?;
    if binding.transaction_id != data.transaction_id()
        || binding.transaction_id != resume.transaction()
        || bundle.source_bundle() != binding.source_bundle
        || bundle.data_root().directory().identity() != data.root().directory().identity()
    {
        return Err(error("HISTORY_HANDOFF_CHANGED"));
    }
    let bytes = serde_json::to_vec(&HandoffManifest {
        schema: 1,
        binding: binding.clone(),
        data: data.reference().clone(),
        bundle: bundle.reference().clone(),
        resume: resume.reference().clone(),
    })
    .map_err(blocked)?;
    let digest = store.retain_manifest(&bytes)?;
    store.append(
        generation,
        JournalEvent::Manifest {
            role: ManifestRole::ManagerHandoff,
            digest,
        },
    )?;
    let inspection = store.inspect(binding)?;
    let checkpoint = ActiveContextMarker::transition_from(&inspection)?;
    {
        match MarkerStore::open_existing(installation.root().clone(), &control).map_err(blocked)? {
            None => {
                MarkerStore::create(
                    installation.root().clone(),
                    &control,
                    &checkpoint,
                    &mut store,
                )
                .map_err(blocked)?;
            }
            Some(mut marker) => {
                let prior = ActiveContextMarker::decode(marker.current().map_err(blocked)?)?;
                if prior.binding() == binding {
                    marker.append(&checkpoint, &mut store).map_err(blocked)?;
                } else {
                    if !prior.is_terminal() {
                        return Err(error("HISTORY_RECOVERY_REQUIRED"));
                    }
                    let mut previous = JournalStore::open_windows_transaction(
                        installation.root().clone(),
                        &prior.binding().transaction_id,
                    )?;
                    previous.bind_existing(prior.binding())?;
                    marker
                        .append_successor(&checkpoint, &mut previous, &mut store)
                        .map_err(blocked)?;
                }
            }
        }
    }
    control.verify_root(installation.root()).map_err(blocked)?;
    drop(store);
    drop(control);
    Ok(PublishedManagerHandoff { resume })
}

pub(crate) struct InitialManager {
    pub(crate) installation: Arc<InstallationControl>,
    pub(crate) data: TransactionDataRoot,
    pub(crate) binding: JournalBinding,
    pub(crate) child: ManagerChildAdmission,
}
impl InitialManager {
    /// Read-only re-admission of the already resumed exact initial child. It
    /// cannot launch/resume an installer or reconstruct an expired process.
    pub(crate) fn open(request: &ManagerRequest) -> Result<Self, SafeError> {
        let installation = InstallationControl::open(false)?;
        let control = installation.acquire_control()?;
        let marker = {
            let stored = MarkerStore::open_existing(installation.root().clone(), &control)
                .map_err(blocked)?
                .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
            ActiveContextMarker::decode(stored.current().map_err(blocked)?)?
        };
        if marker.binding().transaction_id != request.transaction_id() || marker.is_terminal() {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let binding = marker.binding().clone();
        let store = JournalStore::open_windows_transaction(
            installation.root().clone(),
            request.transaction_id(),
        )?;
        let inspection = store.inspect(&binding)?;
        if inspection.blocked {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        let journal = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        marker.validate_checkpoint(
            journal,
            inspection
                .head()
                .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?,
        )?;
        if journal.phase() != JournalPhase::Reviewed || journal.requires_reconciliation() {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        let digest = journal
            .manifest(ManifestRole::ManagerHandoff)
            .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?;
        let handoff: HandoffManifest =
            serde_json::from_slice(&store.read_manifest(digest)?).map_err(blocked)?;
        if handoff.schema != 1 || handoff.binding != binding {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let data = TransactionDataRoot::reopen(
            installation.clone(),
            &control,
            request.transaction_id(),
            handoff.data,
        )?;
        let bundle = ManagerBundle::reopen(
            data.root().clone(),
            request.transaction_id(),
            &handoff.bundle,
            &CurrentUser::capture().map_err(blocked)?,
        )
        .map_err(blocked)?;
        if bundle.source_bundle() != binding.source_bundle {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let child = ManagerChildAdmission::admit(
            data.root().clone(),
            request.transaction_id(),
            &handoff.resume,
            &CatalogService::production()?,
        )
        .map_err(blocked)?;
        if child.selection().installer().sha256() != binding.target_package {
            return Err(error("HISTORY_TARGET_CHANGED"));
        }
        control.verify_root(installation.root()).map_err(blocked)?;
        // The source must reopen the journal to record its actual browser exit.
        // No writer or installation-control guard survives into readiness.
        drop(store);
        drop(control);
        Ok(Self {
            installation,
            data,
            binding,
            child,
        })
    }
}
