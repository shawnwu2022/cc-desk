//! Same-process source-only return. Absence of a process handle is not proof
//! that CreateProcess never ran: both the one-use live lane and the complete
//! intent history must still exclude every installer/historical creation.
use super::{
    context::{
        bundle_restore::RetainedInstallationBundle, HeldBundle, HeldContext, HeldRoot,
        PrivateTreeCopy, RestoredContextRoots, TreeManifest,
    },
    durability::MarkerStore,
    fence::ImageFence,
    lease::{ControlLease, ExclusiveLease},
    registration_state::{RegistrationJournal, RetainedRegistrationState},
    scope::{ConfiguredExclusions, FencedInstallation},
    security::CurrentUser,
    shortcuts::RetainedProductShortcuts,
    source_lifecycle::SourceHandoffTerminal,
    source_session::AcquiredSourceParts,
    startup::{InstallationControl, TransactionDataRoot},
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        journal::{
            EffectKind, EffectSpec, JournalBinding, JournalEvent, JournalStore, Observation,
            ObservedResult, RootKind,
        },
        maintenance::ActiveContextMarker,
        verified_package::sha256,
    },
};
use parking_lot::Mutex;
use std::{collections::BTreeMap, os::windows::ffi::OsStrExt, sync::Arc};

fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_EARLY_ABORT_BLOCKED")
}

/// Minted once, before any source mutation, from the actual acquired owners.
/// Not Clone/Deserialize. The coordinator invalidates it BEFORE attempting any
/// process-creation intent, even if that journal write itself fails.
pub(crate) struct SourceNoLaunch {
    binding: JournalBinding,
    initial_generation: u64,
    open: bool,
    fence: Arc<Mutex<ImageFence>>,
    terminal: Arc<SourceHandoffTerminal>,
    roots: BTreeMap<RootKind, TreeManifest>,
}
impl SourceNoLaunch {
    pub(crate) fn capture(parts: &AcquiredSourceParts) -> Result<Self, SafeError> {
        parts
            .store
            .verify_no_source_launch(&parts.binding, parts.generation)?;
        parts.data.verify_installation(&parts.installation)?;
        parts
            .control
            .verify_root(parts.installation.root())
            .map_err(blocked)?;
        parts
            .exclusive
            .verify_root(parts.installation.root())
            .map_err(blocked)?;
        parts.terminal.verify(&parts.binding)?;
        parts.scope.verify().map_err(blocked)?;
        parts.context.verify_durable().map_err(blocked)?;
        parts.current_bundle.tree().verify().map_err(blocked)?;
        parts
            .exclusions
            .verify_context(&parts.context)
            .map_err(blocked)?;
        verify_bound_roots(
            &parts.binding,
            &parts.context,
            &parts.terminal,
            &parts.scope,
            &parts.data,
        )?;
        let fence = parts.fence.lock();
        parts.scope.verify_fence(&fence).map_err(blocked)?;
        require_original_fence(&parts.scope, &fence)?;
        if parts.scope.source_process_identity() != parts.terminal.exit().host_identity()
            || parts
                .current_bundle
                .manifest()
                .logical_digest()
                .map_err(blocked)?
                != parts.binding.source_bundle
        {
            return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
        }
        Ok(Self {
            binding: parts.binding.clone(),
            initial_generation: parts.generation,
            open: true,
            fence: parts.fence.clone(),
            terminal: parts.terminal.clone(),
            roots: [RootKind::Desk, RootKind::WebView]
                .into_iter()
                .map(|kind| (kind, parts.context.tree(kind).manifest().clone()))
                .collect(),
        })
    }
    pub(crate) fn invalidate_before_process_intent(&mut self) {
        self.open = false;
    }
    pub(crate) fn verify(
        &self,
        binding: &JournalBinding,
        store: &JournalStore,
        generation: u64,
    ) -> Result<(), SafeError> {
        if !self.open || &self.binding != binding || generation < self.initial_generation {
            return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
        }
        store.verify_no_source_launch(binding, generation)
    }
    fn verify_context(&self, context: &HeldContext) -> Result<(), SafeError> {
        context.verify_durable().map_err(blocked)?;
        if [RootKind::Desk, RootKind::WebView]
            .into_iter()
            .any(|kind| self.roots.get(&kind) != Some(context.tree(kind).manifest()))
        {
            return Err(error("HISTORY_SOURCE_CHANGED"));
        }
        Ok(())
    }
    pub(crate) fn verify_restored(&self, context: &RestoredContextRoots) -> Result<(), SafeError> {
        context
            .verify(&CurrentUser::capture().map_err(blocked)?)
            .map_err(blocked)?;
        let snapshot = context.original_snapshot();
        let bytes = snapshot.encode()?;
        crate::version_history::snapshot::SnapshotManifest::decode(
            &bytes,
            &sha256(&bytes),
            &self.binding,
        )?;
        if snapshot.context_id != self.binding.source_context
            || snapshot.roots.len() != 2
            || snapshot.roots.iter().any(|root| {
                self.roots.get(&root.root).is_none_or(|original| {
                    original.location_identity != root.location_identity
                        || original.entries != root.entries
                })
            })
        {
            return Err(error("HISTORY_SOURCE_CHANGED"));
        }
        Ok(())
    }
}

/// The actual source owners shared by the pre-seal and fully restored-context
/// lanes. These borrowed capabilities remain alive through marker readback.
#[derive(Clone, Copy)]
pub(crate) struct SourceReturnInputs<'a> {
    pub(crate) no_launch: &'a SourceNoLaunch,
    pub(crate) binding: &'a JournalBinding,
    pub(crate) installation: &'a InstallationControl,
    pub(crate) data: &'a TransactionDataRoot,
    pub(crate) terminal: &'a Arc<SourceHandoffTerminal>,
    pub(crate) scope: &'a FencedInstallation,
    pub(crate) fence: &'a Arc<Mutex<ImageFence>>,
    pub(crate) original_bundle: &'a RetainedInstallationBundle,
    pub(crate) registration: &'a RetainedRegistrationState,
    pub(crate) shortcuts: &'a RetainedProductShortcuts,
    pub(crate) exclusive: &'a ExclusiveLease,
    pub(crate) control: &'a ControlLease,
}
impl SourceReturnInputs<'_> {
    pub(crate) fn verify(
        &self,
        store: &mut JournalStore,
        generation: u64,
    ) -> Result<(), SafeError> {
        self.no_launch.verify(self.binding, store, generation)?;
        store.verify_windows_binding(self.installation.root(), self.binding, generation)?;
        self.data.verify_installation(self.installation)?;
        self.control
            .verify_root(self.installation.root())
            .map_err(blocked)?;
        self.exclusive
            .verify_root(self.installation.root())
            .map_err(blocked)?;
        self.terminal.verify(self.binding)?;
        self.scope.verify().map_err(blocked)?;
        if !Arc::ptr_eq(self.fence, &self.no_launch.fence)
            || !Arc::ptr_eq(self.terminal, &self.no_launch.terminal)
            || self.data.transaction_id() != self.binding.transaction_id
            || self.scope.source_process_identity() != self.terminal.exit().host_identity()
            || self.original_bundle.directory().identity() != self.scope.directory().identity()
            || self
                .original_bundle
                .source_manifest()
                .logical_digest()
                .map_err(blocked)?
                != self.binding.source_bundle
        {
            return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
        }
        let user = CurrentUser::capture().map_err(blocked)?;
        user.require_unelevated().map_err(blocked)?;
        self.original_bundle.verify(&user).map_err(blocked)?;
        self.scope
            .verify_fence(&self.fence.lock())
            .map_err(blocked)?;
        let mut journal = RegistrationJournal::new(
            store,
            self.installation.root().clone(),
            self.exclusive,
            self.binding.clone(),
            generation,
        )
        .map_err(blocked)?;
        self.registration
            .verify_original(&mut journal)
            .map_err(blocked)?;
        self.shortcuts.verify_original().map_err(blocked)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SourceAbortInputs<'a> {
    pub(crate) owners: SourceReturnInputs<'a>,
    pub(crate) current_bundle: &'a HeldBundle,
    pub(crate) context: &'a HeldContext,
    pub(crate) exclusions: &'a ConfiguredExclusions,
}
#[derive(Clone, Copy)]
pub(crate) struct SourceAbortEvidence<'a> {
    inputs: SourceAbortInputs<'a>,
}
impl SourceAbortEvidence<'_> {
    pub(crate) fn binding(&self) -> &JournalBinding {
        self.inputs.owners.binding
    }
    pub(crate) fn verify_writer(
        &self,
        store: &mut JournalStore,
        generation: u64,
    ) -> Result<(), SafeError> {
        self.verify_source_contents(store, generation)?;
        require_original_fence(self.inputs.owners.scope, &self.inputs.owners.fence.lock())
    }
    fn verify_source_contents(
        &self,
        store: &mut JournalStore,
        generation: u64,
    ) -> Result<(), SafeError> {
        let inputs = &self.inputs;
        inputs.owners.verify(store, generation)?;
        inputs.owners.no_launch.verify_context(inputs.context)?;
        inputs
            .exclusions
            .verify_context(inputs.context)
            .map_err(blocked)?;
        inputs.current_bundle.tree().verify().map_err(blocked)?;
        if inputs.current_bundle.manifest().tree
            != inputs.owners.original_bundle.source_manifest().tree
            || inputs
                .current_bundle
                .manifest()
                .logical_digest()
                .map_err(blocked)?
                != self.binding().source_bundle
        {
            return Err(error("HISTORY_SOURCE_CHANGED"));
        }
        verify_bound_roots(
            self.binding(),
            inputs.context,
            inputs.owners.terminal,
            inputs.owners.scope,
            inputs.owners.data,
        )
    }
    pub(crate) fn verify_current(&self, store: &mut JournalStore) -> Result<(), SafeError> {
        let inspection = store.inspect(self.binding())?;
        if inspection.blocked {
            return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
        }
        let generation = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_EARLY_ABORT_BLOCKED"))?
            .generation();
        self.verify_writer(store, generation)
    }
    pub(crate) fn observations(&self, store: &mut JournalStore) -> Result<[Vec<u8>; 4], SafeError> {
        self.verify_current(store)?;
        let owner = &self.inputs.owners;
        Ok([
            self.inputs
                .current_bundle
                .manifest()
                .encode()
                .map_err(blocked)?,
            serde_json::to_vec(&[
                self.inputs.context.tree(RootKind::Desk).manifest(),
                self.inputs.context.tree(RootKind::WebView).manifest(),
            ])
            .map_err(blocked)?,
            serde_json::to_vec(&(
                store.read_manifest(owner.registration.digest())?,
                store.read_manifest(owner.shortcuts.digest())?,
            ))
            .map_err(blocked)?,
            serde_json::to_vec(&(
                "actual-source-host-and-webview-terminal-no-process-intent",
                owner.terminal.exit().host_identity(),
                owner.terminal.exit().udf_identity(),
                owner.no_launch.initial_generation,
            ))
            .map_err(blocked)?,
        ])
    }
}

pub(crate) struct SourcePartialEvidence<'a> {
    inputs: SourceAbortInputs<'a>,
    copy: &'a PrivateTreeCopy,
    effect_id: String,
    intent_generation: u64,
    plan_generation: u64,
    generation: u64,
}
pub(crate) struct SourcePartialRequest {
    pub(crate) effect_id: String,
    pub(crate) intent_generation: u64,
    pub(crate) generation: u64,
    pub(crate) partial: Vec<u8>,
    pub(crate) source: Vec<u8>,
}
impl SourcePartialEvidence<'_> {
    pub(crate) fn verify(
        &self,
        store: &mut JournalStore,
    ) -> Result<SourcePartialRequest, SafeError> {
        let evidence = SourceAbortEvidence {
            inputs: self.inputs,
        };
        evidence.verify_source_contents(store, self.generation)?;
        let (pending, generation) = store
            .context_pending()?
            .ok_or_else(|| error("HISTORY_EARLY_ABORT_BLOCKED"))?;
        let EffectKind::PrivateBackupEntry {
            plan_generation,
            manifest,
            ..
        } = &pending.kind
        else {
            return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
        };
        if pending.effect_id != self.effect_id
            || generation != self.intent_generation
            || *plan_generation != self.plan_generation
            || ![RootKind::Desk, RootKind::WebView].into_iter().any(|kind| {
                self.inputs
                    .context
                    .tree(kind)
                    .manifest()
                    .digest()
                    .ok()
                    .as_ref()
                    == Some(manifest)
            })
        {
            return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
        }
        let partial = self
            .copy
            .partial_source_observation(
                self.inputs.owners.data.root(),
                self.plan_generation,
                &CurrentUser::capture().map_err(blocked)?,
            )
            .map_err(blocked)?;
        let source = serde_json::to_vec(&(
            self.inputs.current_bundle.manifest(),
            self.inputs.context.tree(RootKind::Desk).manifest(),
            self.inputs.context.tree(RootKind::WebView).manifest(),
            self.inputs.owners.terminal.exit().host_identity(),
            self.inputs.owners.terminal.exit().udf_identity(),
            self.inputs.owners.no_launch.initial_generation,
        ))
        .map_err(blocked)?;
        Ok(SourcePartialRequest {
            effect_id: self.effect_id.clone(),
            intent_generation: self.intent_generation,
            generation: self.generation,
            partial,
            source,
        })
    }
}
/// Retain and abandon ONLY an exact pending source-context private copy. The
/// same root and all actual partial bytes remain guarded; no source-image or
/// root-move uncertainty can be cleared by this admission.
pub(crate) fn retain_source_partial(
    inputs: &SourceAbortInputs<'_>,
    copy: &mut PrivateTreeCopy,
    store: &mut JournalStore,
    generation: &mut u64,
) -> Result<(), SafeError> {
    SourceAbortEvidence { inputs: *inputs }.verify_source_contents(store, *generation)?;
    let (pending, intent_generation) = store
        .context_pending()?
        .ok_or_else(|| error("HISTORY_EARLY_ABORT_BLOCKED"))?;
    let EffectKind::PrivateBackupEntry {
        plan_generation, ..
    } = pending.kind
    else {
        return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
    };
    let user = CurrentUser::capture().map_err(blocked)?;
    copy.observe_partial_for_source_abort(inputs.owners.data.root(), plan_generation, &user)
        .map_err(blocked)?;
    let evidence = SourcePartialEvidence {
        inputs: *inputs,
        copy,
        effect_id: pending.effect_id,
        intent_generation,
        plan_generation,
        generation: *generation,
    };
    *generation = store.retain_source_partial(&evidence)?;
    SourceAbortEvidence { inputs: *inputs }.verify_source_contents(store, *generation)?;
    copy.partial_source_observation(inputs.owners.data.root(), plan_generation, &user)
        .map_err(blocked)?;
    Ok(())
}

/// Partial private data and all recovery copies remain owned by the caller.
/// This publishes only after the original roots/image/registration/shortcuts
/// are actually back and unchanged. It never invents restoration effects.
pub(crate) fn publish_source_abort(
    inputs: SourceAbortInputs<'_>,
    store: &mut JournalStore,
    generation: &mut u64,
) -> Result<(), SafeError> {
    let evidence = SourceAbortEvidence { inputs };
    evidence.verify_writer(store, *generation)?;
    let proof = store.admit_source_abort(evidence)?;
    *generation = store.abort_pre_context(&proof)?;
    let checkpoint = ActiveContextMarker::pre_context_aborted(&store.inspect(evidence.binding())?)?;
    let mut marker = MarkerStore::open_existing(
        inputs.owners.installation.root().clone(),
        inputs.owners.control,
    )
    .map_err(blocked)?
    .ok_or_else(|| error("HISTORY_EARLY_ABORT_BLOCKED"))?;
    let prior = ActiveContextMarker::decode(marker.current().map_err(blocked)?)?;
    if prior.binding() != evidence.binding() || prior.is_terminal() {
        return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
    }
    evidence.verify_writer(store, *generation)?;
    marker.append(&checkpoint, store).map_err(blocked)?;
    evidence.verify_writer(store, *generation)?;
    if marker.current().map_err(blocked)? != checkpoint.encode()?
        || ActiveContextMarker::pre_context_aborted(&store.inspect(evidence.binding())?)?
            .encode()?
            != checkpoint.encode()?
    {
        return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
    }
    Ok(())
}

/// Reverse roots with their original PrivateTreeCopy rotation tickets first.
/// The caller must then readmit HeldBundle through this restored SAME fence
/// before publish_source_abort; the old moved-location owner stays retained on
/// any readmission failure. A pending/unknown image move is never replayed.
pub(crate) fn reverse_source_fence(
    inputs: &SourceAbortInputs<'_>,
    store: &mut JournalStore,
    generation: &mut u64,
) -> Result<(), SafeError> {
    inputs.owners.no_launch.verify_context(inputs.context)?;
    inputs
        .exclusions
        .verify_context(inputs.context)
        .map_err(blocked)?;
    reverse_fence(&inputs.owners, store, generation)
}
pub(crate) fn reverse_source_fence_for_return(
    inputs: &SourceReturnInputs<'_>,
    context: &RestoredContextRoots,
    store: &mut JournalStore,
    generation: &mut u64,
) -> Result<(), SafeError> {
    inputs.no_launch.verify_restored(context)?;
    store.verify_preinstall_return()?;
    reverse_fence(inputs, store, generation)
}
fn reverse_fence(
    inputs: &SourceReturnInputs<'_>,
    store: &mut JournalStore,
    generation: &mut u64,
) -> Result<(), SafeError> {
    inputs.verify(store, *generation)?;
    let Some((original, original_generation, observed)) =
        store.source_fence_for_reverse(inputs.binding, *generation)?
    else {
        return require_original_fence(inputs.scope, &inputs.fence.lock());
    };
    let before = fence_bytes(&inputs.fence.lock())?;
    if sha256(&before) != observed {
        return Err(error("HISTORY_SOURCE_CHANGED"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    *generation = store.append(
        *generation,
        JournalEvent::Intent {
            effect: EffectSpec {
                effect_id: id.clone(),
                kind: EffectKind::ReverseSourceFence {
                    original_effect_id: original.effect_id,
                    original_intent_generation: original_generation,
                },
                before: observed,
                expected_postconditions: original.before.clone(),
            },
        },
    )?;
    let intent_generation = *generation;
    inputs.verify(store, *generation)?;
    // The rename is no-replacement and uses only the retained registered slot.
    // Errors preserve the inverse intent and the actual exclusive file handle.
    inputs
        .fence
        .lock()
        .rename_to(
            inputs.scope.directory().clone(),
            inputs.scope.image_name().clone(),
        )
        .map_err(blocked)?;
    inputs.verify(store, *generation)?;
    let actual = fence_bytes(&inputs.fence.lock())?;
    if sha256(&actual) != original.before {
        return Err(error("HISTORY_SOURCE_CHANGED"));
    }
    let actual = store.retain_manifest(&actual)?;
    let receipt = store.retain_effect_receipt(&id, Observation::Applied, &actual)?;
    *generation = store.append(
        *generation,
        JournalEvent::Observed {
            effect_id: id,
            intent_generation,
            result: ObservedResult {
                observation: Observation::Applied,
                receipt: Some(receipt),
            },
        },
    )?;
    require_original_fence(inputs.scope, &inputs.fence.lock())?;
    inputs.verify(store, *generation)
}
fn fence_bytes(fence: &ImageFence) -> Result<Vec<u8>, SafeError> {
    let (parent, name) = fence.held_location().map_err(blocked)?;
    serde_json::to_vec(&(
        fence.identity(),
        parent,
        name.os_string().encode_wide().collect::<Vec<_>>(),
    ))
    .map_err(blocked)
}
fn require_original_fence(scope: &FencedInstallation, fence: &ImageFence) -> Result<(), SafeError> {
    scope.verify_fence(fence).map_err(blocked)?;
    let (parent, name) = fence.held_location().map_err(blocked)?;
    if &parent != scope.directory().identity() || &name != scope.image_name() {
        return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
    }
    Ok(())
}
fn verify_bound_roots(
    binding: &JournalBinding,
    context: &HeldContext,
    terminal: &SourceHandoffTerminal,
    scope: &FencedInstallation,
    data: &TransactionDataRoot,
) -> Result<(), SafeError> {
    let desk = match context.tree(RootKind::Desk).root() {
        HeldRoot::Present(root) => serde_json::json!({"present":root.identity()}),
        HeldRoot::Absent { parent, name } => serde_json::json!({"absentParent":parent.identity(),
            "suffix":[name.os_string().encode_wide().collect::<Vec<_>>()]}),
    };
    let HeldRoot::Present(udf) = context.tree(RootKind::WebView).root() else {
        return Err(error("HISTORY_ROOT_CHANGED"));
    };
    if udf.identity() != terminal.udf().identity()
        || sha256(
            &serde_json::to_vec(&(
                desk,
                udf.identity(),
                scope.directory().identity(),
                data.root().directory().identity(),
            ))
            .map_err(blocked)?,
        ) != binding.roots
    {
        return Err(error("HISTORY_ROOT_CHANGED"));
    }
    Ok(())
}
