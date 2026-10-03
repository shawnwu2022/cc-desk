//! Bounded same-process Return after source sealing, before any installer
//! creation intent. Every fallible stage leaves its actual custody here.
use super::{
    context::{
        ContextJournal, ContextRestoration, FreshContextRoots, HeldBundle, LaterContextRoots,
        RestoredContextRoots, RetainedContextRoots,
    },
    files::{ComponentName, PrivateDirectory},
    registration_state::{RegistrationJournal, RestoredRegistrationReceipt},
    security::CurrentUser,
    shortcuts::{ShortcutJournal, ShortcutRestoreReceipt},
    source_failure::{reverse_source_fence_for_return, SourceReturnInputs},
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        journal::{
            EffectKind, EffectSpec, JournalEvent, JournalPhase, JournalStore, ManifestRole,
            Observation, ObservedResult, ShortcutSlot,
        },
        maintenance::SnapshotBoundary,
        snapshot::{SnapshotLimits, SnapshotManifest},
        verified_package::sha256,
    },
};
use std::{collections::BTreeMap, ffi::OsStr, io, sync::Arc};

fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_CONTEXT_RETURN_BLOCKED")
}

/// Adoption is infallible, so the coordinator can store this owner before the
/// first read or namespace operation. No lease, HKEY, or source owner moves
/// away from the coordinator's original dedicated thread.
pub(crate) struct PreinstallReturnAttempt {
    started: bool,
    originals: Option<RetainedContextRoots>,
    fresh: Option<FreshContextRoots>,
    quarantine: Option<Arc<PrivateDirectory>>,
    later: Option<LaterContextRoots>,
    restoration: Option<ContextRestoration>,
    restored_context: Option<RestoredContextRoots>,
    restored_bundle: Option<HeldBundle>,
    bundle_observation: Option<BundleObservation>,
    registration: Option<RestoredRegistrationReceipt>,
    shortcuts: BTreeMap<ShortcutSlot, ShortcutRestoreReceipt>,
}

struct BundleObservation {
    id: String,
    generation: u64,
    manifest: String,
    receipt: Option<String>,
    applied: bool,
}

/// This proof borrows the entire retained attempt. Reverification still needs
/// the live no-launch token and native owners; serialized state cannot mint it.
pub(crate) struct VerifiedPreinstallReturn<'a> {
    attempt: &'a PreinstallReturnAttempt,
}

impl PreinstallReturnAttempt {
    pub(crate) fn new(
        originals: Option<RetainedContextRoots>,
        fresh: Option<FreshContextRoots>,
    ) -> Self {
        Self {
            started: false,
            originals,
            fresh,
            quarantine: None,
            later: None,
            restoration: None,
            restored_context: None,
            restored_bundle: None,
            bundle_observation: None,
            registration: None,
            shortcuts: BTreeMap::new(),
        }
    }

    /// A rejected or uncertain operation is never replayed. All completed and
    /// partial stages remain held for inspection by the parked coordinator.
    pub(crate) fn restore(
        &mut self,
        inputs: &SourceReturnInputs<'_>,
        boundary: &SnapshotBoundary,
        store: &mut JournalStore,
        generation: &mut u64,
    ) -> Result<(), SafeError> {
        if self.started {
            return Err(error("HISTORY_CONTEXT_RETURN_BLOCKED"));
        }
        self.started = true;
        inputs.verify(store, *generation)?;
        boundary.verify_live()?;
        if boundary.binding() != inputs.binding {
            return Err(error("HISTORY_CONTEXT_RETURN_BLOCKED"));
        }
        let user = CurrentUser::capture().map_err(blocked)?;
        let originals = self
            .originals
            .as_ref()
            .ok_or_else(|| error("HISTORY_CONTEXT_RETURN_BLOCKED"))?;
        let snapshot_bytes = originals.snapshot().encode()?;
        SnapshotManifest::decode(&snapshot_bytes, &sha256(&snapshot_bytes), inputs.binding)?;
        originals.verify(&user).map_err(blocked)?;
        with_context_journal(inputs, store, generation, |journal| {
            originals.complete_preservation_for_return(
                boundary,
                &inputs.fence.lock(),
                inputs.no_launch,
                &user,
                journal,
            )
        })?;
        // SourceSealed may fail before the fresh owner itself is constructed.
        // Observing this empty attempt still preserves any actual occupants.
        if self.fresh.is_none() {
            self.fresh = Some(FreshContextRoots::new(originals, inputs.binding).map_err(blocked)?);
        }
        self.quarantine = Some(Arc::new(
            PrivateDirectory::create_new(
                inputs.data.root().directory().clone(),
                ComponentName::new(OsStr::new(&format!(
                    "preinstall-later-{}",
                    uuid::Uuid::new_v4()
                )))
                .map_err(blocked)?,
                &user,
            )
            .map_err(blocked)?,
        ));
        FreshContextRoots::observe_for_return_retaining(
            &mut self.fresh,
            &mut self.later,
            originals,
            self.quarantine
                .as_ref()
                .expect("retained quarantine")
                .clone(),
            boundary,
            &user,
        )
        .map_err(blocked)?;
        inputs.verify(store, *generation)?;
        with_context_journal(inputs, store, generation, |journal| {
            self.later
                .as_ref()
                .expect("retained later observation")
                .admit_preinstall_return(originals, boundary, &inputs.fence.lock(), &user, journal)
        })?;
        with_context_journal(inputs, store, generation, |journal| {
            self.later
                .as_mut()
                .expect("retained later observation")
                .preserve(originals, boundary, &inputs.fence.lock(), &user, journal)
        })?;
        let bytes = self
            .later
            .as_ref()
            .expect("retained later observation")
            .manifest_bytes(originals, &user)
            .map_err(blocked)?;
        let digest = store.retain_manifest(&bytes)?;
        inputs.verify(store, *generation)?;
        *generation = store.append(
            *generation,
            JournalEvent::Manifest {
                role: ManifestRole::RetainedTargetContext,
                digest,
            },
        )?;
        self.restoration = Some(
            ContextRestoration::new_retaining(&mut self.originals, &mut self.later)
                .map_err(blocked)?,
        );
        *generation = store.append(
            *generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restoring,
            },
        )?;
        inputs.verify(store, *generation)?;
        with_context_journal(inputs, store, generation, |journal| {
            self.restoration
                .as_mut()
                .expect("retained context restoration")
                .restore(boundary, &inputs.fence.lock(), &user, journal)
        })?;
        self.restored_context = Some(
            ContextRestoration::finish_retaining(&mut self.restoration, &user).map_err(blocked)?,
        );

        // The original executable is returned only after its exact matching
        // Desk/WebView roots are restored. The inverse uses the same exclusive
        // fence and its authenticated, durable forward-rename receipt.
        reverse_source_fence_for_return(
            inputs,
            self.restored_context.as_ref().expect("verified context"),
            store,
            generation,
        )?;
        self.restored_bundle = Some(
            HeldBundle::capture(
                inputs.scope.directory().clone(),
                inputs.scope.image_name().clone(),
                inputs.fence.clone(),
                SnapshotLimits::default(),
            )
            .map_err(blocked)?,
        );
        self.record_bundle_restore(inputs, store, generation)?;

        // These APIs record genuine same-state observations for unchanged
        // registration and shortcuts, including independently retained reads.
        let mut journal = RegistrationJournal::new(
            store,
            inputs.installation.root().clone(),
            inputs.exclusive,
            inputs.binding.clone(),
            *generation,
        )
        .map_err(blocked)?;
        let result = inputs.registration.restore(&mut journal);
        *generation = journal.generation();
        drop(journal);
        self.registration = Some(result.map_err(blocked)?);
        for slot in [ShortcutSlot::Desktop, ShortcutSlot::StartMenu] {
            let mut journal = ShortcutJournal::new(
                store,
                inputs.installation.root().clone(),
                inputs.exclusive,
                inputs.binding.clone(),
                *generation,
            )
            .map_err(blocked)?;
            let result = inputs.shortcuts.restore(slot, &mut journal);
            *generation = journal.generation();
            drop(journal);
            self.shortcuts.insert(slot, result.map_err(blocked)?);
        }
        self.verify_restored(inputs, boundary, store, *generation)?;
        Ok(())
    }

    fn verify_bundle(&self, inputs: &SourceReturnInputs<'_>) -> Result<(), SafeError> {
        let bundle = self
            .restored_bundle
            .as_ref()
            .ok_or_else(|| error("HISTORY_CONTEXT_RETURN_BLOCKED"))?;
        bundle.tree().verify().map_err(blocked)?;
        if bundle.manifest().tree != inputs.original_bundle.source_manifest().tree
            || bundle.manifest().logical_digest().map_err(blocked)? != inputs.binding.source_bundle
        {
            return Err(error("HISTORY_SOURCE_CHANGED"));
        }
        let fence = inputs.fence.lock();
        inputs.scope.verify_fence(&fence).map_err(blocked)?;
        let (parent, name) = fence.held_location().map_err(blocked)?;
        if &parent != inputs.scope.directory().identity() || &name != inputs.scope.image_name() {
            return Err(error("HISTORY_SOURCE_CHANGED"));
        }
        Ok(())
    }

    fn record_bundle_restore(
        &mut self,
        inputs: &SourceReturnInputs<'_>,
        store: &mut JournalStore,
        generation: &mut u64,
    ) -> Result<(), SafeError> {
        inputs.verify(store, *generation)?;
        self.verify_bundle(inputs)?;
        let bytes = self
            .restored_bundle
            .as_ref()
            .expect("retained restored bundle")
            .manifest()
            .encode()
            .map_err(blocked)?;
        let observed = store.retain_manifest(&bytes)?;
        let source = store.retain_manifest(
            &inputs
                .original_bundle
                .source_manifest()
                .encode()
                .map_err(blocked)?,
        )?;
        let id = uuid::Uuid::new_v4().to_string();
        *generation = store.append(
            *generation,
            JournalEvent::Intent {
                effect: EffectSpec {
                    effect_id: id.clone(),
                    kind: EffectKind::VerifySourceBundleRestore,
                    before: source,
                    expected_postconditions: observed.clone(),
                },
            },
        )?;
        self.bundle_observation = Some(BundleObservation {
            id,
            generation: *generation,
            manifest: observed,
            receipt: None,
            applied: false,
        });
        inputs.verify(store, *generation)?;
        self.verify_bundle(inputs)?;
        let observation = self
            .bundle_observation
            .as_mut()
            .expect("retained bundle intent");
        let receipt = store.retain_effect_receipt(
            &observation.id,
            Observation::Applied,
            &observation.manifest,
        )?;
        observation.receipt = Some(receipt.clone());
        *generation = store.append(
            *generation,
            JournalEvent::Observed {
                effect_id: observation.id.clone(),
                intent_generation: observation.generation,
                result: ObservedResult {
                    observation: Observation::Applied,
                    receipt: Some(receipt),
                },
            },
        )?;
        observation.applied = true;
        self.verify_bundle(inputs)
    }

    pub(crate) fn verify_restored<'a>(
        &'a self,
        inputs: &SourceReturnInputs<'_>,
        boundary: &SnapshotBoundary,
        store: &mut JournalStore,
        generation: u64,
    ) -> Result<VerifiedPreinstallReturn<'a>, SafeError> {
        inputs.verify(store, generation)?;
        boundary.verify_live()?;
        let user = CurrentUser::capture().map_err(blocked)?;
        let restored = self
            .restored_context
            .as_ref()
            .ok_or_else(|| error("HISTORY_CONTEXT_RETURN_BLOCKED"))?;
        restored.verify(&user).map_err(blocked)?;
        inputs.no_launch.verify_restored(restored)?;
        if boundary.binding() != inputs.binding
            || restored.original_snapshot().context_id != inputs.binding.source_context
        {
            return Err(error("HISTORY_SOURCE_CHANGED"));
        }
        self.verify_bundle(inputs)?;
        let observation = self
            .bundle_observation
            .as_ref()
            .ok_or_else(|| error("HISTORY_CONTEXT_RETURN_BLOCKED"))?;
        let bundle_bytes = self
            .restored_bundle
            .as_ref()
            .expect("verified restored bundle")
            .manifest()
            .encode()
            .map_err(blocked)?;
        if !observation.applied
            || observation.receipt.is_none()
            || sha256(&bundle_bytes) != observation.manifest
            || store.read_manifest(&observation.manifest)? != bundle_bytes
        {
            return Err(error("HISTORY_CONTEXT_RETURN_BLOCKED"));
        }
        let inspection = store.inspect(inputs.binding)?;
        let current = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_CONTEXT_RETURN_BLOCKED"))?;
        if inspection.blocked
            || current.generation() != generation
            || current.requires_reconciliation()
            || !matches!(
                current.phase(),
                JournalPhase::Restoring | JournalPhase::Restored
            )
            || current.effect_observation(&observation.id) != Some(Observation::Applied)
        {
            return Err(error("HISTORY_CONTEXT_RETURN_BLOCKED"));
        }
        self.registration
            .as_ref()
            .ok_or_else(|| error("HISTORY_CONTEXT_RETURN_BLOCKED"))?
            .verify()
            .map_err(blocked)?;
        if self.shortcuts.len() != 2 {
            return Err(error("HISTORY_CONTEXT_RETURN_BLOCKED"));
        }
        for slot in [ShortcutSlot::Desktop, ShortcutSlot::StartMenu] {
            self.shortcuts
                .get(&slot)
                .ok_or_else(|| error("HISTORY_CONTEXT_RETURN_BLOCKED"))?
                .verify()
                .map_err(blocked)?;
        }
        Ok(VerifiedPreinstallReturn { attempt: self })
    }
}

impl VerifiedPreinstallReturn<'_> {
    pub(crate) fn verify(
        &self,
        inputs: &SourceReturnInputs<'_>,
        boundary: &SnapshotBoundary,
        store: &mut JournalStore,
        generation: u64,
    ) -> Result<(), SafeError> {
        self.attempt
            .verify_restored(inputs, boundary, store, generation)?;
        Ok(())
    }
}

fn with_context_journal(
    inputs: &SourceReturnInputs<'_>,
    store: &mut JournalStore,
    generation: &mut u64,
    operation: impl FnOnce(&mut ContextJournal<'_>) -> io::Result<()>,
) -> Result<(), SafeError> {
    let mut journal = ContextJournal::new(
        store,
        inputs.installation.root().clone(),
        inputs.exclusive,
        inputs.binding.clone(),
        *generation,
    )
    .map_err(blocked)?;
    let result = operation(&mut journal);
    *generation = journal.generation();
    result.map_err(blocked)
}
