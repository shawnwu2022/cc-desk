//! 已封存返回点的窄重入。旧receipt不构造进程权限；本次重新取得独占权及所有实际对象。
use super::*;
use crate::version_history::{
    journal::{EffectKind, JournalEvent, JournalPhase, RetainedRoleGuard, RootKind, ShortcutSlot},
    manager_types::{ManagerAction, ManagerPhase},
    manager_worker::ManagerDocumentProof,
    snapshot::SnapshotManifest,
    windows::{
        context::bundle_restore::{
            BundleRestoration, RestoredInstallationBundle, RetainedInstallationBundle,
        },
        context::{ContextJournal, ContextRestoration, LaterContextRoots, RetainedContextRoots},
        coordinator_evidence::{CurrentImageEvidence, ReturnBoundary, VerifiedImageAbsence},
        fence::ImageFence,
        files::{ComponentName, Directory, FileAccess, FileIdentity, PrivateDirectory},
        lease::{ControlLease, ExclusiveLease, ExclusiveLeaseWitness},
        manager_bundle::{ManagerRecord, ManagerRecordReference},
        registration_state::{RegistrationJournal, RetainedRegistrationState},
        return_checkpoint::ReturnCheckpointMaterials,
        scope::ConfiguredExclusions,
        shortcuts::{RetainedProductShortcuts, ShortcutJournal},
        source_lifecycle::SourceHandoffExitManifest,
        startup::TransactionDataReference,
    },
};
use parking_lot::Mutex;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    io,
    os::windows::ffi::OsStringExt,
};

fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_RETURN_CHECKPOINT_BLOCKED")
}
fn io_blocked(_: impl std::fmt::Debug) -> io::Error {
    io::Error::other("sealed return objects changed")
}
fn component(value: &str) -> Result<ComponentName, SafeError> {
    ComponentName::new(OsStr::new(value)).map_err(blocked)
}
fn child(
    data: &TransactionDataRoot,
    name: &str,
    user: &CurrentUser,
) -> Result<Arc<PrivateDirectory>, SafeError> {
    Ok(Arc::new(
        PrivateDirectory::open_existing(data.root().directory().clone(), component(name)?, user)
            .map_err(blocked)?,
    ))
}
fn image_quarantine(record: &str) -> Result<String, SafeError> {
    let id = record
        .strip_prefix("return-boundary-")
        .and_then(|s| s.strip_suffix(".json"))
        .ok_or_else(|| blocked("invalid boundary selector"))?;
    if id.len() != 32
        || !id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(blocked("invalid boundary selector"));
    }
    Ok(format!("return-image-{id}"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct BoundaryRecord {
    schema: u32,
    binding: JournalBinding,
    installation: FileIdentity,
    data: TransactionDataReference,
    roots: BTreeMap<RootKind, String>,
    original_bundle: ManagerRecordReference,
    original_context: SnapshotManifest,
    installer: (String, String),
    historical: (String, String),
    image: ImageObservation,
    quarantine: FileIdentity,
    configured_exclusions: String,
}
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged, deny_unknown_fields)]
enum ImageObservation {
    Fenced {
        fenced: FileIdentity,
        digest: String,
        parent: FileIdentity,
        name: Vec<u16>,
    },
    Absent {
        #[serde(rename = "absentParent")]
        absent_parent: FileIdentity,
        name: Vec<u16>,
    },
}
impl ImageObservation {
    fn reopen_completed_fence(
        &self,
        quarantine: &Arc<Directory>,
    ) -> io::Result<Option<ImageFence>> {
        match self {
            Self::Fenced {
                fenced,
                digest,
                parent,
                name,
            } => {
                if parent != quarantine.identity()
                    || OsString::from_wide(name) != OsStr::new("current-image.exe")
                {
                    return Err(io_blocked("foreign completed quarantine"));
                }
                ImageFence::acquire(
                    quarantine.clone(),
                    ComponentName::new(&OsString::from_wide(name))?,
                    fenced,
                    digest,
                )
                .map(Some)
            }
            Self::Absent { .. } => Ok(None),
        }
    }
}

/// 字段仅由本模块在完整seal、Applied移动记录及实际父对象核验后构造。
pub(crate) struct VerifiedRecoveryImageOrigin {
    record: Arc<ManagerRecord>,
    installation: Arc<Directory>,
    image_name: ComponentName,
    quarantine: Arc<PrivateDirectory>,
    expected: ImageObservation,
}
impl VerifiedRecoveryImageOrigin {
    pub(crate) fn verify(&self) -> io::Result<()> {
        let user = CurrentUser::capture()?;
        self.record.verify(&user)?;
        self.installation.recheck()?;
        self.quarantine.verify(&user)?;
        match &self.expected {
            ImageObservation::Fenced { parent, name, .. } => {
                if parent != self.quarantine.directory().identity()
                    || OsString::from_wide(name) != OsStr::new("current-image.exe")
                {
                    return Err(io_blocked("foreign quarantine"));
                }
            }
            ImageObservation::Absent {
                absent_parent,
                name,
            } => {
                if absent_parent != self.installation.identity()
                    || OsString::from_wide(name) != self.image_name.os_string()
                {
                    return Err(io_blocked("foreign absent image"));
                }
            }
        }
        Ok(())
    }
    pub(crate) fn retained_image(
        &self,
    ) -> io::Result<(Arc<Directory>, ComponentName, &FileIdentity, &str)> {
        self.verify()?;
        let ImageObservation::Fenced {
            fenced,
            digest,
            name,
            ..
        } = &self.expected
        else {
            return Err(io_blocked("image is absent"));
        };
        // 准备完成但尚未恢复的检查点要求原入口仍为空；绝不覆盖新出现的对象。
        match self
            .installation
            .open_file(self.image_name.clone(), FileAccess::Read)
        {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(io_blocked("original slot no longer absent")),
        }
        Ok((
            self.quarantine.directory().clone(),
            ComponentName::new(&OsString::from_wide(name))?,
            fenced,
            digest,
        ))
    }
    pub(crate) fn installation(&self) -> &Arc<Directory> {
        &self.installation
    }
    pub(crate) fn image_name(&self) -> &ComponentName {
        &self.image_name
    }
}

/// 新协议的独占对象守卫，独立于旧TerminalProcessJob类型。
pub(crate) struct RecoveredSnapshotGuards {
    binding: JournalBinding,
    installation: Arc<InstallationControl>,
    data: Arc<TransactionDataRoot>,
    exclusive: ExclusiveLeaseWitness,
    origin: Arc<VerifiedRecoveryImageOrigin>,
    roots: BTreeMap<RootKind, String>,
    roles: Vec<(ManifestRole, RetainedRoleGuard)>,
    exclusions: ConfiguredExclusions,
    completed_historical_image: Option<ImageFence>,
}
impl RecoveredSnapshotGuards {
    pub(crate) fn verify_live(&self) -> Result<(), SafeError> {
        self.data.verify_installation(&self.installation)?;
        self.exclusive
            .verify_root(self.installation.root())
            .map_err(blocked)?;
        self.origin.verify().map_err(blocked)?;
        if let Some(image) = &self.completed_historical_image {
            image.verify().map_err(blocked)?;
        }
        self.exclusions.verify_external().map_err(blocked)?;
        for (role, guard) in &self.roles {
            guard.verify_role(&self.binding, *role, self.installation.root())?;
        }
        Ok(())
    }
    pub(crate) fn binding(&self) -> &JournalBinding {
        &self.binding
    }
    pub(crate) fn roots(&self) -> &BTreeMap<RootKind, String> {
        &self.roots
    }
    pub(crate) fn installation(&self) -> &Arc<Directory> {
        self.origin.installation()
    }
    pub(crate) fn image_name(&self) -> &ComponentName {
        self.origin.image_name()
    }
}

struct ReturnObjects {
    control: ControlLease,
    exclusive: ExclusiveLease,
    installation: Arc<InstallationControl>,
    binding: JournalBinding,
    generation: u64,
    materials: Vec<u8>,
    marker: ActiveContextMarker,
    boundary: Arc<ReturnBoundary>,
    context: Option<ContextRestoration>,
    bundle: Option<BundleRestoration>,
    original: Arc<RetainedInstallationBundle>,
    completion: Option<ManagerRecordReference>,
    registration: RetainedRegistrationState,
    shortcuts: RetainedProductShortcuts,
}

/// 只有已重开完整实际对象的调用栈能构造该借用证据，且claim前再次校验原文档。
pub(crate) struct ReenteredReturnCheckpoint<'a> {
    objects: &'a ReturnObjects,
    document: &'a ManagerDocumentProof,
}
impl ReenteredReturnCheckpoint<'_> {
    pub(crate) fn binding(&self) -> &JournalBinding {
        &self.objects.binding
    }
    pub(crate) fn verify(
        &self,
        store: &mut JournalStore,
        generation: u64,
    ) -> Result<Vec<u8>, SafeError> {
        let o = self.objects;
        self.document.check_transaction(&o.binding)?;
        o.exclusive
            .verify_root(o.installation.root())
            .map_err(blocked)?;
        o.control
            .verify_root(o.installation.root())
            .map_err(blocked)?;
        store.verify_windows_binding(o.installation.root(), &o.binding, generation)?;
        o.boundary.verify_current_image()?;
        let user = CurrentUser::capture().map_err(blocked)?;
        let (source, later) = o
            .context
            .as_ref()
            .ok_or_else(|| blocked("context missing"))?
            .verify_return_checkpoint(&user)
            .map_err(blocked)?;
        let inspection = store.inspect(&o.binding)?;
        let state = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| blocked("state missing"))?;
        for (role, bytes) in [
            (ManifestRole::SourceContext, source),
            (ManifestRole::RetainedTargetContext, later),
        ] {
            if store.read_manifest(
                state
                    .manifest(role)
                    .ok_or_else(|| blocked("role missing"))?,
            )? != bytes
            {
                return Err(blocked("context changed"));
            }
        }
        let mut journal = ContextJournal::new(
            store,
            o.installation.root().clone(),
            &o.exclusive,
            o.binding.clone(),
            generation,
        )
        .map_err(blocked)?;
        o.bundle
            .as_ref()
            .ok_or_else(|| blocked("bundle missing"))?
            .verify_return_checkpoint(&user, &mut journal)
            .map_err(blocked)?;
        drop(journal);
        let mut journal = RegistrationJournal::new(
            store,
            o.installation.root().clone(),
            &o.exclusive,
            o.binding.clone(),
            generation,
        )
        .map_err(blocked)?;
        o.registration
            .verify_retained(&mut journal)
            .map_err(blocked)?;
        o.shortcuts.verify_retained().map_err(blocked)?;
        self.document.check_transaction(&o.binding)?;
        Ok(o.materials.clone())
    }
}

impl ReenteredManager {
    fn open_return_objects(
        &self,
        completed: bool,
    ) -> Result<(JournalStore, ReturnObjects), SafeError> {
        self.verify_reentry_owner()?;
        let user = CurrentUser::capture().map_err(blocked)?;
        let control = self.installation.acquire_control()?;
        let exclusive = self
            .installation
            .leases()
            .acquire_exclusive(&control)
            .map_err(blocked)?;
        let marker = {
            let saved = MarkerStore::open_existing(self.installation.root().clone(), &control)
                .map_err(blocked)?
                .ok_or_else(|| blocked("marker absent"))?;
            ActiveContextMarker::decode(saved.current().map_err(blocked)?)?
        };
        if marker.binding() != &self.binding {
            return Err(blocked("foreign marker"));
        }
        let mut store = JournalStore::open_windows_transaction(
            self.installation.root().clone(),
            self.transaction_id(),
        )?;
        store.bind_existing(&self.binding)?;
        let checkpoint = if completed {
            store.inspect_completed_return_checkpoint(&self.binding, &marker)?
        } else {
            store.inspect_return_checkpoint(&self.binding, &marker)?
        };
        let generation = checkpoint.generation;
        let materials: ReturnCheckpointMaterials =
            serde_json::from_slice(&checkpoint.materials).map_err(blocked)?;
        if materials.schema != 1
            || materials.binding != self.binding
            || serde_json::to_vec(&materials.data).map_err(blocked)?
                != serde_json::to_vec(self.data.reference()).map_err(blocked)?
        {
            return Err(blocked("foreign checkpoint materials"));
        }
        let record = Arc::new(
            ManagerRecord::open(
                self.data.root().clone(),
                &materials.boundary.0,
                &materials.boundary.1,
                &user,
            )
            .map_err(blocked)?,
        );
        let saved: BoundaryRecord = record.decode(&user).map_err(blocked)?;
        if saved.schema != 1
            || saved.binding != self.binding
            || saved.roots.len() != 2
            || !saved.roots.contains_key(&RootKind::Desk)
            || !saved.roots.contains_key(&RootKind::WebView)
            || saved.installer != materials.installer_terminal
            || saved.historical != materials.historical_terminal
            || serde_json::to_vec(&saved.data).map_err(blocked)?
                != serde_json::to_vec(&materials.data).map_err(blocked)?
        {
            return Err(blocked("boundary binding differs"));
        }
        crate::version_history::journal::validate_digest(&saved.configured_exclusions)?;
        let (_, observed) = store.applied_effect_observation(&EffectKind::FenceHistoricalImage)?;
        let (record_name, record_ref, image): (String, ManagerRecordReference, ImageObservation) =
            serde_json::from_slice(&observed).map_err(blocked)?;
        if record_name != materials.boundary.0
            || record_ref != materials.boundary.1
            || image != saved.image
        {
            return Err(blocked("image move not applied"));
        }
        let quarantine = child(&self.data, &image_quarantine(&record_name)?, &user)?;
        if quarantine.directory().identity() != &saved.quarantine {
            return Err(blocked("quarantine replaced"));
        }
        let mut journal = ContextJournal::new(
            &mut store,
            self.installation.root().clone(),
            &exclusive,
            self.binding.clone(),
            generation,
        )
        .map_err(blocked)?;
        let original = Arc::new(
            RetainedInstallationBundle::reopen(
                self.data.root().clone(),
                &saved.original_bundle,
                &user,
                &mut journal,
            )
            .map_err(blocked)?,
        );
        drop(journal);
        if original.directory().identity() != &saved.installation {
            return Err(blocked("installation replaced"));
        }
        let state = store
            .inspect(&self.binding)?
            .last_valid
            .ok_or_else(|| blocked("journal absent"))?;
        if saved.original_context.encode()?
            != store.read_manifest(
                state
                    .manifest(ManifestRole::SourceContext)
                    .ok_or_else(|| blocked("source context absent"))?,
            )?
        {
            return Err(blocked("source context differs"));
        }
        let exit: SourceHandoffExitManifest = serde_json::from_slice(
            &store.read_manifest(
                state
                    .manifest(ManifestRole::SourceHandoffExit)
                    .ok_or_else(|| blocked("source exit absent"))?,
            )?,
        )
        .map_err(blocked)?;
        let parents = exit.recovery_context_parents(&self.binding)?;
        let source_quarantine = child(&self.data, "source-context", &user)?;
        let later_quarantine = child(&self.data, "later-context", &user)?;
        let mut journal = ContextJournal::new(
            &mut store,
            self.installation.root().clone(),
            &exclusive,
            self.binding.clone(),
            generation,
        )
        .map_err(blocked)?;
        let originals = RetainedContextRoots::reopen_observation(
            parents,
            self.data.root().clone(),
            source_quarantine,
            &user,
            &mut journal,
        )
        .map_err(blocked)?;
        let later = LaterContextRoots::reopen_observation(
            &originals,
            later_quarantine,
            &user,
            &mut journal,
        )
        .map_err(blocked)?;
        let mut context =
            ContextRestoration::reopen_observation(originals, later, &user, &mut journal)
                .map_err(blocked)?;
        drop(journal);
        let exclusions = context
            .reopen_return_exclusions(
                original.directory().clone(),
                self.data.root().clone(),
                &saved.configured_exclusions,
                &user,
            )
            .map_err(blocked)?;
        let origin = Arc::new(VerifiedRecoveryImageOrigin {
            record,
            installation: original.directory().clone(),
            image_name: component(original.source_manifest().original_image_name())?,
            quarantine,
            expected: saved.image,
        });
        origin.verify().map_err(blocked)?;
        let completed_historical_image = if completed {
            origin
                .expected
                .reopen_completed_fence(origin.quarantine.directory())
                .map_err(blocked)?
        } else {
            None
        };
        let mut completion = None;
        let image = if completed {
            let mut journal = ContextJournal::new(
                &mut store,
                self.installation.root().clone(),
                &exclusive,
                self.binding.clone(),
                generation,
            )
            .map_err(blocked)?;
            let observation = RestoredInstallationBundle::observe_completed(
                &original,
                &materials.bundle_plan,
                &user,
                &mut journal,
            )
            .map_err(blocked)?;
            drop(journal);
            completion = Some(observation.record);
            CurrentImageEvidence::Fenced(Arc::new(Mutex::new(
                ImageFence::acquire(
                    original.directory().clone(),
                    origin.image_name.clone(),
                    &observation.image_identity,
                    &observation.image_digest,
                )
                .map_err(blocked)?,
            )))
        } else {
            match &origin.expected {
                ImageObservation::Fenced { .. } => CurrentImageEvidence::Fenced(Arc::new(
                    Mutex::new(ImageFence::reopen_return_checkpoint(&origin).map_err(blocked)?),
                )),
                ImageObservation::Absent { .. } => CurrentImageEvidence::Absent(
                    VerifiedImageAbsence::capture(
                        original.directory().clone(),
                        origin.image_name.clone(),
                    )
                    .map_err(blocked)?,
                ),
            }
        };
        let mut roles = Vec::new();
        for role in [
            ManifestRole::ManagerHandoff,
            ManifestRole::SourceHandoffExit,
            ManifestRole::SourceContext,
            ManifestRole::SourceBundle,
            ManifestRole::FreshTargetContext,
            ManifestRole::RetainedTargetContext,
            ManifestRole::Registration,
            ManifestRole::Shortcuts,
        ] {
            roles.push((
                role,
                store.retain_role_guard(
                    self.installation.root().clone(),
                    &self.binding,
                    generation,
                    role,
                )?,
            ));
        }
        let boundary = Arc::new(ReturnBoundary::from_recovered(
            RecoveredSnapshotGuards {
                binding: self.binding.clone(),
                installation: self.installation.clone(),
                data: self.data.clone(),
                exclusive: exclusive.witness().map_err(blocked)?,
                origin,
                roots: saved.roots,
                roles,
                exclusions,
                completed_historical_image,
            },
            image,
        )?);
        let mut journal = ContextJournal::new(
            &mut store,
            self.installation.root().clone(),
            &exclusive,
            self.binding.clone(),
            generation,
        )
        .map_err(blocked)?;
        let bundle = if completed {
            None
        } else {
            Some(
                BundleRestoration::reopen_prepared(
                    original.clone(),
                    boundary.clone(),
                    &materials.bundle_plan,
                    &user,
                    &mut journal,
                )
                .map_err(blocked)?,
            )
        };
        drop(journal);
        let mut journal = RegistrationJournal::new(
            &mut store,
            self.installation.root().clone(),
            &exclusive,
            self.binding.clone(),
            generation,
        )
        .map_err(blocked)?;
        let registration =
            RetainedRegistrationState::reopen(&mut journal, original.directory().clone())
                .map_err(blocked)?;
        registration
            .verify_retained(&mut journal)
            .map_err(blocked)?;
        drop(journal);
        let shortcuts = RetainedProductShortcuts::reopen_current_user(
            &store,
            &self.binding,
            state
                .manifest(ManifestRole::Shortcuts)
                .ok_or_else(|| blocked("shortcuts absent"))?,
        )
        .map_err(blocked)?;
        self.verify_reentry_owner()?;
        Ok((
            store,
            ReturnObjects {
                control,
                exclusive,
                installation: self.installation.clone(),
                binding: self.binding.clone(),
                generation,
                materials: checkpoint.materials,
                marker,
                boundary,
                context: Some(context),
                bundle,
                original,
                completion,
                registration,
                shortcuts,
            },
        ))
    }

    pub(super) fn inspect_recovered_return(&self) -> Result<ManagerStatus, SafeError> {
        if let Ok((store, objects)) = self.open_return_objects(false) {
            // 完整对象只读重验后才提供按钮；点击时仍需从头重验和新文档确认。
            let user = CurrentUser::capture().map_err(blocked)?;
            objects
                .context
                .as_ref()
                .ok_or_else(|| blocked("context absent"))?
                .verify_return_checkpoint(&user)
                .map_err(blocked)?;
            return self.return_status(
                &store,
                ManagerPhase::RecoveryRequired,
                &[ManagerAction::Refresh, ManagerAction::ReturnToPrevious],
            );
        }
        let (mut store, mut objects) = self.open_return_objects(true)?;
        let user = CurrentUser::capture().map_err(blocked)?;
        let context =
            ContextRestoration::finish_retaining(&mut objects.context, &user).map_err(blocked)?;
        context.verify(&user).map_err(blocked)?;
        let mut journal = ContextJournal::new(
            &mut store,
            self.installation.root().clone(),
            &objects.exclusive,
            self.binding.clone(),
            objects.generation,
        )
        .map_err(blocked)?;
        let bundle = RestoredInstallationBundle::reopen(
            objects.original.clone(),
            objects.boundary.clone(),
            objects
                .completion
                .as_ref()
                .ok_or_else(|| blocked("completion reference absent"))?,
            &user,
            &mut journal,
        )
        .map_err(blocked)?;
        bundle.verify(&user).map_err(blocked)?;
        drop(journal);
        let mut journal = RegistrationJournal::new(
            &mut store,
            self.installation.root().clone(),
            &objects.exclusive,
            self.binding.clone(),
            objects.generation,
        )
        .map_err(blocked)?;
        let registration = objects
            .registration
            .reopen_completed(&mut journal)
            .map_err(blocked)?;
        drop(journal);
        let shortcuts = objects
            .shortcuts
            .reopen_completed(&store)
            .map_err(blocked)?;
        self.verify_reentry_owner()?;
        context.verify(&user).map_err(blocked)?;
        bundle.verify(&user).map_err(blocked)?;
        registration.recheck().map_err(blocked)?;
        shortcuts.verify().map_err(blocked)?;
        self.return_status(&store, ManagerPhase::Restored, &[ManagerAction::Refresh])
    }

    fn return_status(
        &self,
        store: &JournalStore,
        phase: ManagerPhase,
        actions: &[ManagerAction],
    ) -> Result<ManagerStatus, SafeError> {
        let inspection = store.inspect(&self.binding)?;
        if inspection.blocked {
            return Err(blocked("journal changed"));
        }
        ManagerStatus::project_recovered_return(
            inspection
                .last_valid
                .as_ref()
                .ok_or_else(|| blocked("journal absent"))?,
            self.material.diagnostic(),
            phase,
            actions,
        )
    }

    pub(crate) fn return_previous(
        &mut self,
        document: Arc<ManagerDocumentProof>,
        expected_generation: u64,
        mut publish: impl FnMut(ManagerStatus),
    ) -> Result<ManagerStatus, SafeError> {
        document.check_transaction(&self.binding)?;
        let (mut store, mut objects) = self.open_return_objects(false)?;
        if objects.generation != expected_generation {
            return Err(error("HISTORY_GENERATION_CHANGED"));
        }
        let user = CurrentUser::capture().map_err(blocked)?;
        let result = (|| {
            let evidence = ReenteredReturnCheckpoint {
                objects: &objects,
                document: &document,
            };
            objects.generation = store.claim_reentered_return_checkpoint(
                &evidence,
                &objects.marker,
                objects.generation,
            )?;
            publish(self.return_status(
                &store,
                ManagerPhase::Returning,
                &[ManagerAction::Refresh],
            )?);
            self.verify_return_command(&objects, &mut store, &document)?;
            let mut journal = ContextJournal::new(
                &mut store,
                self.installation.root().clone(),
                &objects.exclusive,
                self.binding.clone(),
                objects.generation,
            )
            .map_err(blocked)?;
            let result = objects
                .context
                .as_mut()
                .ok_or_else(|| blocked("context missing"))?
                .restore_after_exit(&objects.boundary, &user, &mut journal);
            objects.generation = journal.generation();
            drop(journal);
            result.map_err(blocked)?;
            let context = ContextRestoration::finish_retaining(&mut objects.context, &user)
                .map_err(blocked)?;
            self.verify_return_command(&objects, &mut store, &document)?;
            publish(self.return_status(
                &store,
                ManagerPhase::Returning,
                &[ManagerAction::Refresh],
            )?);
            let mut journal = ContextJournal::new(
                &mut store,
                self.installation.root().clone(),
                &objects.exclusive,
                self.binding.clone(),
                objects.generation,
            )
            .map_err(blocked)?;
            let result = objects
                .bundle
                .as_mut()
                .ok_or_else(|| blocked("bundle missing"))?
                .restore(&user, &mut journal);
            objects.generation = journal.generation();
            drop(journal);
            let bundle = result.map_err(blocked)?;
            self.verify_return_command(&objects, &mut store, &document)?;
            let mut journal = RegistrationJournal::new(
                &mut store,
                self.installation.root().clone(),
                &objects.exclusive,
                self.binding.clone(),
                objects.generation,
            )
            .map_err(blocked)?;
            let result = objects.registration.restore(&mut journal);
            objects.generation = journal.generation();
            drop(journal);
            let registration = result.map_err(blocked)?;
            let mut shortcuts = Vec::new();
            for slot in [ShortcutSlot::Desktop, ShortcutSlot::StartMenu] {
                self.verify_return_command(&objects, &mut store, &document)?;
                let mut journal = ShortcutJournal::new(
                    &mut store,
                    self.installation.root().clone(),
                    &objects.exclusive,
                    self.binding.clone(),
                    objects.generation,
                )
                .map_err(blocked)?;
                let result = objects.shortcuts.restore(slot, &mut journal);
                objects.generation = journal.generation();
                drop(journal);
                shortcuts.push(result.map_err(blocked)?);
            }
            self.verify_return_command(&objects, &mut store, &document)?;
            context.verify(&user).map_err(blocked)?;
            bundle.verify(&user).map_err(blocked)?;
            registration.verify().map_err(blocked)?;
            for receipt in &shortcuts {
                receipt.verify().map_err(blocked)?;
            }
            if context.original_snapshot().context_id != self.binding.source_context
                || bundle.source_manifest().logical_digest().map_err(blocked)?
                    != self.binding.source_bundle
            {
                return Err(blocked("restored source differs"));
            }
            objects.generation = store.append(
                objects.generation,
                JournalEvent::Phase {
                    phase: JournalPhase::Restored,
                },
            )?;
            let terminal = ActiveContextMarker::restored(&store.inspect(&self.binding)?)?;
            self.verify_return_command(&objects, &mut store, &document)?;
            context.verify(&user).map_err(blocked)?;
            bundle.verify(&user).map_err(blocked)?;
            registration.verify().map_err(blocked)?;
            for receipt in &shortcuts {
                receipt.verify().map_err(blocked)?;
            }
            {
                let mut marker =
                    MarkerStore::open_existing(self.installation.root().clone(), &objects.control)
                        .map_err(blocked)?
                        .ok_or_else(|| blocked("marker missing"))?;
                if marker.current().map_err(blocked)? != objects.marker.encode()? {
                    return Err(blocked("marker changed"));
                }
                marker.append(&terminal, &mut store).map_err(blocked)?;
                if marker.current().map_err(blocked)? != terminal.encode()? {
                    return Err(blocked("terminal marker changed"));
                }
            }
            self.verify_return_command(&objects, &mut store, &document)?;
            context.verify(&user).map_err(blocked)?;
            bundle.verify(&user).map_err(blocked)?;
            registration.verify().map_err(blocked)?;
            for receipt in &shortcuts {
                receipt.verify().map_err(blocked)?;
            }
            self.return_status(&store, ManagerPhase::Restored, &[ManagerAction::Refresh])
        })();
        if result.is_err() {
            // claim或任何效果后不补写、不回滚、不重新派发；旧marker/日志保留真实不确定状态。
            if let Ok(status) = self.return_status(
                &store,
                ManagerPhase::RecoveryRequired,
                &[ManagerAction::Refresh],
            ) {
                publish(status);
            }
        }
        result
    }

    fn verify_return_command(
        &self,
        objects: &ReturnObjects,
        store: &mut JournalStore,
        document: &ManagerDocumentProof,
    ) -> Result<(), SafeError> {
        document.check_transaction(&self.binding)?;
        self.verify_reentry_owner()?;
        objects
            .control
            .verify_root(self.installation.root())
            .map_err(blocked)?;
        objects
            .exclusive
            .verify_root(self.installation.root())
            .map_err(blocked)?;
        objects.boundary.verify_live()?;
        store.verify_windows_binding(
            self.installation.root(),
            &self.binding,
            objects.generation,
        )?;
        document.check_transaction(&self.binding)
    }
    fn verify_reentry_owner(&self) -> Result<(), SafeError> {
        self.data.verify_installation(&self.installation)?;
        self.material.verify().map_err(blocked)?;
        let user = CurrentUser::capture().map_err(blocked)?;
        user.require_unelevated().map_err(blocked)?;
        self.current.verify_current_user(&user).map_err(blocked)?;
        self.current
            .verify_held_image(self.material.bundle().image())
            .map_err(blocked)?;
        if self.current.terminal(0).map_err(blocked)?.is_some() {
            return Err(blocked("manager terminated"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    #[test]
    fn completed_historical_image_retains_exact_actual_object() {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let quarantine = Arc::new(
            PrivateDirectory::create_new(parent, component("quarantine").unwrap(), &user).unwrap(),
        );
        drop(
            ManagerRecord::create(
                quarantine.clone(),
                "current-image.exe",
                &"historical image",
                &user,
            )
            .unwrap(),
        );
        let image = quarantine
            .directory()
            .open_file(component("current-image.exe").unwrap(), FileAccess::Read)
            .unwrap();
        let observation = ImageObservation::Fenced {
            fenced: image.identity().clone(),
            digest: image.digest().unwrap(),
            parent: quarantine.directory().identity().clone(),
            name: OsStr::new("current-image.exe").encode_wide().collect(),
        };
        drop(image);
        let path = temp.path().join("quarantine").join("current-image.exe");
        let fence = observation
            .reopen_completed_fence(quarantine.directory())
            .unwrap()
            .unwrap();
        assert!(std::fs::write(&path, b"changed").is_err());
        assert!(std::fs::remove_file(&path).is_err());
        fence.verify().unwrap();
        drop(fence);
        std::fs::write(&path, b"changed").unwrap();
        assert!(observation
            .reopen_completed_fence(quarantine.directory())
            .is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(observation
            .reopen_completed_fence(quarantine.directory())
            .is_err());
        drop(
            ManagerRecord::create(
                quarantine.clone(),
                "current-image.exe",
                &"historical image",
                &user,
            )
            .unwrap(),
        );
        assert!(observation
            .reopen_completed_fence(quarantine.directory())
            .is_err());
    }
    #[test]
    fn recovered_image_selector_is_exact_and_bounded() {
        assert_eq!(
            image_quarantine("return-boundary-0123456789abcdef0123456789abcdef.json").unwrap(),
            "return-image-0123456789abcdef0123456789abcdef"
        );
        for selector in [
            "return-boundary-ABCDEF01234567890123456789abcdef.json",
            "return-boundary-../image.json",
            "return-boundary-.json",
            "other.json",
            "return-boundary-0123456789abcdef0123456789abcdef.json.extra",
        ] {
            assert!(image_quarantine(selector).is_err());
        }
    }
}
