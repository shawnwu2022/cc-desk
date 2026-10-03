//! Same-object context cutover. Manifests are observations; the coordinator's
//! live boundary and this module's retained guards authorize each operation.
use super::super::coordinator_evidence::ReturnBoundary;
use super::*;
use crate::version_history::{
    journal::{JournalPhase, ManifestRole},
    snapshot::SnapshotManifest,
};

#[cfg(test)]
#[path = "../../tests/version_history_context_custody_windows.rs"]
mod custody_tests;

#[cfg(test)]
#[path = "../../tests/version_history_preinstall_custody_windows.rs"]
mod preinstall_custody_tests;

/// Source quiescence is usable only after the journal positively admitted the
/// no-launch return path. Normal return requires the distinct current-image
/// and historical-process boundary; an original image guard cannot replace it.
#[derive(Clone, Copy)]
enum ContextAdmission<'a> {
    BeforeInstall {
        boundary: &'a SnapshotBoundary,
        fence: &'a ImageFence,
    },
    AfterExit(&'a ReturnBoundary),
}
impl ContextAdmission<'_> {
    fn binding(&self) -> &JournalBinding {
        match self {
            Self::BeforeInstall { boundary, .. } => boundary.binding(),
            Self::AfterExit(boundary) => boundary.binding(),
        }
    }
    fn verify_live(&self) -> io::Result<()> {
        match self {
            Self::BeforeInstall { boundary, fence } => {
                safe(boundary.verify_live())?;
                fence.verify()
            }
            Self::AfterExit(boundary) => safe(boundary.verify_current_image()),
        }
    }
    fn verify_store(&self, store: &JournalStore) -> io::Result<()> {
        self.verify_live()?;
        if matches!(self, Self::BeforeInstall { .. }) {
            safe(store.verify_preinstall_return())?;
        }
        Ok(())
    }
    fn admit(
        &self,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        self.verify_store(journal.store)?;
        if self.binding() != &journal.binding {
            return Err(blocked("context return binding differs"));
        }
        journal.exclusive()?.verify_root(&journal.root)?;
        journal.verify()?;
        let bytes = safe(originals.expected.encode())?;
        safe(SnapshotManifest::decode(
            &bytes,
            &sha256(&bytes),
            &journal.binding,
        ))?;
        originals.verify(user)
    }
}

#[derive(Clone)]
struct OriginSlot {
    parent: Arc<Directory>,
    name: ComponentName,
}
impl OriginSlot {
    fn absent(&self) -> HeldRoot {
        HeldRoot::Absent {
            parent: self.parent.clone(),
            name: self.name.clone(),
        }
    }
    fn observe(&self) -> io::Result<HeldRoot> {
        HeldRoot::observe(self.parent.clone(), self.name.clone())
    }
    fn record(&self) -> io::Result<RootSlotRecord> {
        self.parent.recheck()?;
        Ok(RootSlotRecord {
            parent: self.parent.identity().clone(),
            name: text(&self.name)?,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreservedSourcePlan {
    schema: u32,
    root: RootKind,
    context_id: String,
    snapshot: String,
    before: String,
    after: String,
    copy: CopyReference,
    origin: RootSlotRecord,
    retained: Option<RootSlotRecord>,
    object: Option<FileIdentity>,
    rotation_effect: Option<String>,
}

/// Protected selectors for a particular same-object namespace attempt. They
/// locate evidence on restart; only freshly held objects authorize operations.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextLocationPlan {
    schema: u32,
    root: RootKind,
    context_id: String,
    source: String,
    copy: CopyReference,
    origin: RootSlotRecord,
    retained: Option<RootSlotRecord>,
    object: Option<FileIdentity>,
}

/// Complete original context held after equal-state rotation. No public fields,
/// Deserialize, or source-role binding constructor. The manager binds its role
/// only after combining this evidence with real scope and process ownership.
pub(crate) struct RetainedContextRoots {
    source: HeldContext,
    copies: BTreeMap<RootKind, PrivateTreeCopy>,
    expected: SnapshotManifest,
    origins: BTreeMap<RootKind, OriginSlot>,
    retained: BTreeMap<RootKind, TreeManifest>,
}

/// Immutable backup custody for Return admission. Only private copy trees and
/// original slot parents are shared: original/current descendant readers must
/// remain movable by the context executor's guarded release/re-admission gap.
pub(crate) struct ReturnContextCustody {
    copies: BTreeMap<RootKind, PrivateTreeCopy>,
    expected: SnapshotManifest,
    origins: BTreeMap<RootKind, OriginSlot>,
}
impl ReturnContextCustody {
    pub(crate) fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        if self.copies.len() != 2 || self.origins.len() != 2 {
            return Err(blocked("incomplete original context custody"));
        }
        for kind in [RootKind::Desk, RootKind::WebView] {
            let copy = &self.copies[&kind];
            copy.verify(user)?;
            let source = &copy.manifest()?.source;
            let expected = self
                .expected
                .roots
                .iter()
                .find(|root| root.root == kind)
                .ok_or_else(|| blocked("missing original context root"))?;
            if source.entries != expected.entries
                || source.location_identity != expected.location_identity
            {
                return Err(blocked("original context copy differs"));
            }
            self.origins[&kind].parent.recheck()?;
        }
        Ok(())
    }
    pub(crate) fn snapshot(&self) -> &SnapshotManifest {
        &self.expected
    }
    pub(crate) fn verify_data_root(&self, data: &PrivateDirectory) -> io::Result<()> {
        for copy in self.copies.values() {
            if copy.parent.directory().identity() != data.directory().identity() {
                return Err(blocked("original context belongs to another recovery root"));
            }
        }
        Ok(())
    }
    /// The caller stores the complete capture before the final custody check,
    /// so a failure there cannot silently release current namespace readers.
    pub(crate) fn capture_current_retaining(
        &self,
        retained: &mut Option<HeldContext>,
        user: &CurrentUser,
    ) -> io::Result<()> {
        if retained.is_some() {
            return Err(blocked("current context custody is already retained"));
        }
        self.verify(user)?;
        *retained = Some(HeldContext::capture_durable(
            observe_renameable(&self.origins[&RootKind::Desk])?,
            observe_renameable(&self.origins[&RootKind::WebView])?,
            SnapshotLimits::default(),
        )?);
        #[cfg(test)]
        if CONTEXT_CAPTURE_FAILURE.replace(false) {
            return Err(blocked("injected post-capture custody verification"));
        }
        self.verify(user)?;
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    static CONTEXT_CAPTURE_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
impl RetainedContextRoots {
    pub(crate) fn retain_return_custody(
        &self,
        user: &CurrentUser,
    ) -> io::Result<ReturnContextCustody> {
        self.verify(user)?;
        let mut copies = BTreeMap::new();
        for kind in [RootKind::Desk, RootKind::WebView] {
            let original = &self.copies[&kind];
            let held = original
                .tree
                .as_ref()
                .ok_or_else(|| blocked("original copy is incomplete"))?;
            let entries = held
                .entries
                .iter()
                .map(|(path, entry)| {
                    let entry = match entry {
                        HeldEntry::Directory(root) => HeldEntry::Directory(root.clone()),
                        HeldEntry::File(FileGuard::Ordinary(file)) => {
                            HeldEntry::File(FileGuard::Ordinary(file.clone()))
                        }
                        HeldEntry::File(FileGuard::Fenced(_)) => {
                            return Err(blocked("source image cannot be context copy custody"))
                        }
                    };
                    Ok((path.clone(), entry))
                })
                .collect::<io::Result<BTreeMap<_, _>>>()?;
            if held.detached_image.is_some() || held.fenced_location.is_some() {
                return Err(blocked("context backup contains an image fence"));
            }
            copies.insert(
                kind,
                PrivateTreeCopy {
                    parent: original.parent.clone(),
                    name: original.name.clone(),
                    tree: Some(HeldTree {
                        root: held.root.clone(),
                        manifest: held.manifest.clone(),
                        entries,
                        limits: held.limits,
                        detached_image: None,
                        fenced_location: None,
                        flush_required: held.flush_required,
                        durably_flushed: held.durably_flushed,
                    }),
                    manifest: Some(original.manifest()?.clone()),
                    attempted: true,
                    plan_generation: None,
                    rotation_attempted: false,
                    rotation: None,
                    recovery_copy: None,
                    retained_recovery_attempts: BTreeMap::new(),
                },
            );
        }
        let custody = ReturnContextCustody {
            copies,
            expected: self.expected.clone(),
            origins: self.origins.clone(),
        };
        custody.verify(user)?;
        self.verify(user)?;
        Ok(custody)
    }
    pub(crate) fn admit(
        source: HeldContext,
        mut copies: BTreeMap<RootKind, PrivateTreeCopy>,
        mut readmitted: BTreeMap<RootKind, ReadmittedRoot>,
        expected: SnapshotManifest,
        boundary: &SnapshotBoundary,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        Self::admit_retaining(
            &mut Some(source),
            &mut copies,
            &mut readmitted,
            &expected,
            boundary,
            user,
        )
    }
    /// Validate all live evidence before taking custody. A rejected admission
    /// leaves every rotated source, copy, and readmission proof with its caller.
    pub(crate) fn admit_retaining(
        source: &mut Option<HeldContext>,
        copies: &mut BTreeMap<RootKind, PrivateTreeCopy>,
        readmitted: &mut BTreeMap<RootKind, ReadmittedRoot>,
        expected: &SnapshotManifest,
        boundary: &SnapshotBoundary,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        let held_source = source
            .as_ref()
            .ok_or_else(|| blocked("original context custody missing"))?;
        safe(boundary.verify_live())?;
        held_source.verify_durable()?;
        let bytes = safe(expected.encode())?;
        let expected = safe(SnapshotManifest::decode(
            &bytes,
            &sha256(&bytes),
            boundary.binding(),
        ))?;
        if expected.context_id != boundary.binding().source_context || copies.len() != 2 {
            return Err(blocked("original context binding differs"));
        }
        let mut origins = BTreeMap::new();
        let mut retained = BTreeMap::new();
        for kind in [RootKind::Desk, RootKind::WebView] {
            let copy = copies
                .get(&kind)
                .ok_or_else(|| blocked("missing complete original copy"))?;
            copy.verify(user)?;
            let root = expected
                .roots
                .iter()
                .find(|root| root.root == kind)
                .ok_or_else(|| blocked("missing original snapshot root"))?;
            if root.entries != copy.manifest()?.source.entries
                || root.location_identity != copy.manifest()?.source.location_identity
                || boundary.root_identity(kind) != Some(root.location_identity.as_str())
            {
                return Err(blocked("original snapshot does not describe C0"));
            }
            let origin = if root.entries.is_empty() {
                if readmitted.contains_key(&kind)
                    || !held_source.tree(kind).manifest.entries.is_empty()
                {
                    return Err(blocked("original absence differs"));
                }
                let HeldRoot::Absent { parent, name } = &held_source.tree(kind).root else {
                    return Err(blocked("missing source absence guard"));
                };
                OriginSlot {
                    parent: parent.clone(),
                    name: name.clone(),
                }
            } else {
                let proof = readmitted
                    .get(&kind)
                    .ok_or_else(|| blocked("missing same-object root readmission"))?;
                proof.verify(held_source, copy, user)?;
                let HeldRoot::Absent { parent, name } = &proof.live_location else {
                    return Err(blocked("original live slot is not absent"));
                };
                if proof.before != copy.manifest()?.source
                    || proof.after != held_source.tree(kind).manifest
                {
                    return Err(blocked(
                        "readmitted root does not describe original snapshot",
                    ));
                }
                OriginSlot {
                    parent: parent.clone(),
                    name: name.clone(),
                }
            };
            origin.absent().verify()?;
            origins.insert(kind, origin);
            retained.insert(kind, held_source.tree(kind).manifest.clone());
        }
        if readmitted.len()
            != expected
                .roots
                .iter()
                .filter(|root| !root.entries.is_empty())
                .count()
        {
            return Err(blocked("extra root readmission"));
        }
        Self::verify_parts(held_source, copies, &origins, &retained, user)?;
        safe(boundary.verify_live())?;
        // No fallible operation may follow the first custody transfer.
        let result = Self {
            source: source.take().expect("validated original custody"),
            copies: std::mem::take(copies),
            expected,
            origins,
            retained,
        };
        readmitted.clear();
        Ok(result)
    }
    /// Original absence is sealed historical evidence once fresh state exists.
    /// Live-slot existence is checked separately by fresh/preserve/restore steps.
    pub(crate) fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        Self::verify_parts(
            &self.source,
            &self.copies,
            &self.origins,
            &self.retained,
            user,
        )
    }
    fn verify_parts(
        source: &HeldContext,
        copies: &BTreeMap<RootKind, PrivateTreeCopy>,
        origins: &BTreeMap<RootKind, OriginSlot>,
        retained: &BTreeMap<RootKind, TreeManifest>,
        user: &CurrentUser,
    ) -> io::Result<()> {
        for kind in [RootKind::Desk, RootKind::WebView] {
            let copy = &copies[&kind];
            copy.verify(user)?;
            let expected = &retained[&kind];
            let actual = source.tree(kind);
            if actual.manifest != *expected
                || actual.manifest.entries != copy.manifest()?.source.entries
            {
                return Err(blocked("retained original context changed"));
            }
            if !expected.entries.is_empty() {
                actual.verify()?;
                if !actual.durably_flushed {
                    return Err(blocked("original tree has no current file flush evidence"));
                }
            }
            origins[&kind].parent.recheck()?;
        }
        Ok(())
    }
    pub(crate) fn snapshot(&self) -> &SnapshotManifest {
        &self.expected
    }
    pub(crate) fn record_preserved(
        &self,
        boundary: &SnapshotBoundary,
        fence: &ImageFence,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        self.admit_operation(boundary, fence, user, journal)?;
        require_phase(journal, &[JournalPhase::Reviewed])?;
        require_role(
            journal,
            ManifestRole::SourceContext,
            &safe(self.expected.digest())?,
        )?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            self.origins[&kind].absent().verify()?;
            let before = journal.retain(&self.copies[&kind].manifest()?.source)?;
            let after = journal.retain(&self.retained[&kind])?;
            let copy = journal.retain(self.copies[&kind].manifest()?)?;
            let current = self.source.tree(kind);
            let (retained, object, rotation_effect) = if let HeldRoot::Present(root) = &current.root
            {
                let ticket = self.copies[&kind]
                    .rotation
                    .as_ref()
                    .ok_or_else(|| blocked("source rotation ticket missing"))?;
                (
                    Some(RootSlotRecord {
                        parent: ticket.quarantine.directory().identity().clone(),
                        name: text(&ticket.quarantine_name)?,
                    }),
                    Some(root.identity().clone()),
                    Some(ticket.effect_id.clone()),
                )
            } else {
                (None, None, None)
            };
            let plan = PreservedSourcePlan {
                schema: 1,
                root: kind,
                context_id: journal.binding.source_context.clone(),
                snapshot: safe(self.expected.digest())?,
                before: before.clone(),
                after: after.clone(),
                copy: CopyReference {
                    parent: self.copies[&kind].parent.directory().identity().clone(),
                    name: text(&self.copies[&kind].name)?,
                    manifest: copy.clone(),
                },
                origin: self.origins[&kind].record()?,
                retained,
                object,
                rotation_effect,
            };
            let pending = journal.begin(
                EffectKind::PreserveRoot {
                    context: journal.binding.source_context.clone(),
                    root: kind,
                },
                &plan,
                &(&after, &copy),
            )?;
            self.admit_operation(boundary, fence, user, journal)?;
            self.origins[&kind].absent().verify()?;
            journal.applied(
                pending,
                &(&before, &after, &copy, "complete-original-retained"),
            )?;
            self.admit_operation(boundary, fence, user, journal)?;
        }
        Ok(())
    }
    /// Complete only the non-mutating preservation observations interrupted
    /// before SourceSealed. The same roots/copies and their protected plans are
    /// read again; no prior phase or filesystem action is invented or replayed.
    pub(crate) fn complete_preservation_for_return(
        &self,
        boundary: &SnapshotBoundary,
        fence: &ImageFence,
        no_launch: &super::super::source_failure::SourceNoLaunch,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        safe(no_launch.verify(&journal.binding, journal.store, journal.generation))?;
        self.complete_preservation_observations(boundary, fence, user, journal)
    }

    fn complete_preservation_observations(
        &self,
        boundary: &SnapshotBoundary,
        fence: &ImageFence,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        self.admit_operation(boundary, fence, user, journal)?;
        safe(
            journal
                .store
                .verify_no_source_launch(&journal.binding, journal.generation),
        )?;
        require_role(
            journal,
            ManifestRole::SourceContext,
            &safe(self.expected.digest())?,
        )?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            let before = journal.retain(&self.copies[&kind].manifest()?.source)?;
            let after = journal.retain(&self.retained[&kind])?;
            let copy = journal.retain(self.copies[&kind].manifest()?)?;
            let current = self.source.tree(kind);
            let (retained, object, rotation_effect) = if let HeldRoot::Present(root) = &current.root
            {
                let ticket = self.copies[&kind]
                    .rotation
                    .as_ref()
                    .ok_or_else(|| blocked("source rotation ticket missing"))?;
                (
                    Some(RootSlotRecord {
                        parent: ticket.quarantine.directory().identity().clone(),
                        name: text(&ticket.quarantine_name)?,
                    }),
                    Some(root.identity().clone()),
                    Some(ticket.effect_id.clone()),
                )
            } else {
                (None, None, None)
            };
            let actual = PreservedSourcePlan {
                schema: 1,
                root: kind,
                context_id: journal.binding.source_context.clone(),
                snapshot: safe(self.expected.digest())?,
                before: before.clone(),
                after: after.clone(),
                copy: CopyReference {
                    parent: self.copies[&kind].parent.directory().identity().clone(),
                    name: text(&self.copies[&kind].name)?,
                    manifest: copy.clone(),
                },
                origin: self.origins[&kind].record()?,
                retained,
                object,
                rotation_effect,
            };
            let observed = safe(journal.store.source_preservation(kind))?;
            let pending = if let Some((effect, generation, observation)) = observed {
                let plan: PreservedSourcePlan =
                    serde_json::from_slice(&safe(journal.store.read_manifest(&effect.before))?)?;
                if encoded(&plan)? != encoded(&actual)?
                    || safe(journal.store.read_manifest(&effect.expected_postconditions))?
                        != encoded(&(&after, &copy))?
                {
                    return Err(blocked(
                        "source preservation plan differs from retained owners",
                    ));
                }
                self.admit_operation(boundary, fence, user, journal)?;
                if observation == Some(Observation::Applied) {
                    continue;
                }
                if !matches!(observation, None | Some(Observation::Unknown))
                    || safe(journal.store.context_pending())?.as_ref().is_none_or(
                        |(pending, pending_generation)| {
                            pending.effect_id != effect.effect_id
                                || *pending_generation != generation
                        },
                    )
                {
                    return Err(blocked(
                        "source preservation is not the exact pending observation",
                    ));
                }
                PendingEffect {
                    id: effect.effect_id,
                    generation,
                }
            } else {
                require_phase(
                    journal,
                    &[JournalPhase::Reviewed, JournalPhase::RecoveryRequired],
                )?;
                // No fresh namespace may have been admitted while either
                // original preservation record is still missing.
                for origin in self.origins.values() {
                    origin.absent().verify()?;
                }
                journal.begin(
                    EffectKind::PreserveRoot {
                        context: journal.binding.source_context.clone(),
                        root: kind,
                    },
                    &actual,
                    &(&after, &copy),
                )?
            };
            self.admit_operation(boundary, fence, user, journal)?;
            for origin in self.origins.values() {
                origin.absent().verify()?;
            }
            copy_fault(CopyFault::BeforeReceipt)?;
            journal.applied(
                pending,
                &(&before, &after, &copy, "complete-original-retained"),
            )?;
            self.admit_operation(boundary, fence, user, journal)?;
        }
        Ok(())
    }
    fn admit_operation(
        &self,
        boundary: &SnapshotBoundary,
        fence: &ImageFence,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        safe(boundary.verify_live())?;
        if boundary.binding() != &journal.binding {
            return Err(blocked("context operation binding differs"));
        }
        journal.exclusive()?.verify_root(&journal.root)?;
        journal.verify()?;
        let bytes = safe(self.expected.encode())?;
        safe(SnapshotManifest::decode(
            &bytes,
            &sha256(&bytes),
            &journal.binding,
        ))?;
        fence.verify()?;
        self.verify(user)
    }
}

struct FreshRootAttempt {
    pending: PendingEffect,
    tree: Option<HeldTree>,
    complete: bool,
}
/// A create-new, one-attempt owner. Every partial root remains retained on error;
/// a failed attempt is observed by return recovery, never blindly created again.
pub(crate) struct FreshContextRoots {
    binding: JournalBinding,
    origins: BTreeMap<RootKind, OriginSlot>,
    attempts: BTreeMap<RootKind, FreshRootAttempt>,
    return_roots: BTreeMap<RootKind, HeldRoot>,
}
impl FreshContextRoots {
    pub(crate) fn new(
        originals: &RetainedContextRoots,
        binding: &JournalBinding,
    ) -> io::Result<Self> {
        if originals.expected.context_id != binding.source_context {
            return Err(blocked("fresh context source differs"));
        }
        Ok(Self {
            binding: binding.clone(),
            origins: originals.origins.clone(),
            attempts: BTreeMap::new(),
            return_roots: BTreeMap::new(),
        })
    }
    pub(crate) fn create(
        &mut self,
        originals: &RetainedContextRoots,
        boundary: &SnapshotBoundary,
        fence: &ImageFence,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        if self.binding != journal.binding
            || !self.attempts.is_empty()
            || !self.return_roots.is_empty()
        {
            return Err(blocked("fresh creation is one attempt"));
        }
        originals.admit_operation(boundary, fence, user, journal)?;
        safe(
            journal
                .store
                .admit_context_capacity(journal.generation, 2, 24),
        )?;
        require_phase(journal, &[JournalPhase::SourceSealed])?;
        require_role(
            journal,
            ManifestRole::SourceContext,
            &safe(originals.expected.digest())?,
        )?;
        for slot in self.origins.values() {
            slot.absent().verify()?;
        }
        for kind in [RootKind::Desk, RootKind::WebView] {
            let slot = self.origins[&kind].clone();
            originals.admit_operation(boundary, fence, user, journal)?;
            slot.absent().verify()?;
            let pending = journal.begin(
                EffectKind::CreateFreshRoot { root: kind },
                &(safe(originals.expected.digest())?, slot.record()?, "absent"),
                &(slot.record()?, "create-new-empty-private-root"),
            )?;
            self.attempts.insert(
                kind,
                FreshRootAttempt {
                    pending,
                    tree: None,
                    complete: false,
                },
            );
            let result = (|| {
                originals.admit_operation(boundary, fence, user, journal)?;
                copy_fault(CopyFault::BeforeFreshCreate)?;
                let created =
                    PrivateDirectory::create_renameable_new(slot.parent, slot.name, user)?;
                let root = created.directory().clone();
                // Retain the root before any later verification can fail.
                self.attempts
                    .get_mut(&kind)
                    .expect("fresh attempt retained")
                    .tree = Some(HeldTree {
                    root: HeldRoot::Present(root.clone()),
                    manifest: TreeManifest {
                        schema: 1,
                        location_identity: "unobserved".into(),
                        entries: vec![],
                    },
                    entries: BTreeMap::from([(String::new(), HeldEntry::Directory(root.clone()))]),
                    limits: SnapshotLimits::default(),
                    flush_required: false,
                    durably_flushed: false,
                    detached_image: None,
                    fenced_location: None,
                });
                copy_fault(CopyFault::AfterFreshCreate)?;
                let tree = HeldTree::capture_private(root, SnapshotLimits::default(), user)?;
                if tree.manifest.entries.len() != 1 {
                    return Err(blocked("fresh root contains unexpected data"));
                }
                self.attempts
                    .get_mut(&kind)
                    .expect("fresh attempt retained")
                    .tree = Some(tree);
                originals.admit_operation(boundary, fence, user, journal)?;
                let attempt = &self.attempts[&kind];
                copy_fault(CopyFault::BeforeFreshReceipt)?;
                journal.applied(
                    PendingEffect {
                        id: attempt.pending.id.clone(),
                        generation: attempt.pending.generation,
                    },
                    &attempt.tree.as_ref().expect("fresh root captured").manifest,
                )?;
                originals.admit_operation(boundary, fence, user, journal)?;
                self.attempts
                    .get_mut(&kind)
                    .expect("fresh attempt retained")
                    .complete = true;
                Ok(())
            })();
            if let Err(error) = result {
                let pending = &self.attempts[&kind].pending;
                let _ = journal.unknown(PendingEffect {
                    id: pending.id.clone(),
                    generation: pending.generation,
                });
                return Err(error);
            }
        }
        self.verify(originals, user)?;
        Ok(())
    }
    pub(crate) fn verify(
        &self,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
    ) -> io::Result<()> {
        originals.verify(user)?;
        if self.attempts.len() != 2 {
            return Err(blocked("fresh context is incomplete"));
        }
        for kind in [RootKind::Desk, RootKind::WebView] {
            let attempt = &self.attempts[&kind];
            if !attempt.complete {
                return Err(blocked("fresh outcome is uncertain"));
            }
            let tree = attempt
                .tree
                .as_ref()
                .ok_or_else(|| blocked("fresh root guard missing"))?;
            verify_private_tree(tree, user)?;
            let HeldRoot::Present(root) = &tree.root else {
                return Err(blocked("fresh root is absent"));
            };
            if tree.manifest.entries.len() != 1
                || !occupies(root, &self.origins[&kind].parent, &self.origins[&kind].name)?
            {
                return Err(blocked("fresh context changed"));
            }
        }
        Ok(())
    }
    pub(crate) fn manifest_bytes(
        &self,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
    ) -> io::Result<Vec<u8>> {
        self.verify(originals, user)?;
        encoded(&(
            1u32,
            &self.binding,
            &self.binding.target_context,
            [RootKind::Desk, RootKind::WebView]
                .into_iter()
                .map(|kind| {
                    (
                        kind,
                        &self.attempts[&kind]
                            .tree
                            .as_ref()
                            .expect("verified fresh root")
                            .manifest,
                    )
                })
                .collect::<Vec<_>>(),
        ))
    }
}
fn require_phase(journal: &mut ContextJournal<'_>, phases: &[JournalPhase]) -> io::Result<()> {
    journal.verify()?;
    let observed = safe(journal.store.inspect(&journal.binding))?;
    let current = observed
        .last_valid
        .ok_or_else(|| blocked("missing context journal"))?;
    if observed.blocked
        || current.generation() != journal.generation
        || !phases.contains(&current.phase())
    {
        return Err(blocked("context phase differs"));
    }
    Ok(())
}
fn require_role(
    journal: &mut ContextJournal<'_>,
    role: ManifestRole,
    digest: &str,
) -> io::Result<()> {
    journal.verify()?;
    let observed = safe(journal.store.inspect(&journal.binding))?;
    let current = observed
        .last_valid
        .ok_or_else(|| blocked("missing context journal"))?;
    if observed.blocked
        || current.generation() != journal.generation
        || current.manifest(role) != Some(digest)
    {
        return Err(blocked("context manifest role differs"));
    }
    Ok(())
}

/// Full actual later state, including a partial/foreign fresh-root occupant.
/// These objects are retained as user data; they are never silently called fresh.
pub(crate) struct LaterContextRoots {
    binding: JournalBinding,
    context: HeldContext,
    origins: BTreeMap<RootKind, OriginSlot>,
    copies: BTreeMap<RootKind, PrivateTreeCopy>,
    complete_copies: BTreeMap<RootKind, u64>,
    moved: BTreeMap<RootKind, ContextMove>,
    quarantine: Arc<PrivateDirectory>,
    absent_preserved: BTreeSet<RootKind>,
    absent_attempts: BTreeMap<RootKind, PendingEffect>,
    previous_copies: BTreeMap<u64, HeldTree>,
}
struct ContextMove {
    pending: PendingEffect,
    prior: TreeManifest,
    private_name: ComponentName,
    object: FileIdentity,
    complete: bool,
}
impl FreshContextRoots {
    pub(crate) fn release_for_launch(
        self,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        self.verify_for_launch(originals, user, journal)
    }
    /// Keep all created-root guards held if any launch check fails. The caller
    /// releases this owner only after this complete validation succeeds.
    pub(crate) fn verify_for_launch(
        &self,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        self.verify(originals, user)?;
        require_phase(journal, &[JournalPhase::FreshReady])?;
        require_role(
            journal,
            ManifestRole::FreshTargetContext,
            &sha256(&self.manifest_bytes(originals, user)?),
        )?;
        journal.verify()?;
        Ok(())
    }
    /// Transfer retained create handles into a new complete current observation.
    /// No created object is deleted and an unacknowledged root is not replayed.
    pub(crate) fn observe_for_return(
        self,
        originals: &RetainedContextRoots,
        quarantine: Arc<PrivateDirectory>,
        boundary: &SnapshotBoundary,
        user: &CurrentUser,
    ) -> io::Result<LaterContextRoots> {
        let mut fresh = Some(self);
        let mut later = None;
        Self::observe_for_return_retaining(
            &mut fresh, &mut later, originals, quarantine, boundary, user,
        )?;
        Ok(later.expect("observed later context retained"))
    }
    /// Preserve every original create handle until a complete durable later
    /// observation has been stored and checked. A failed read must not consume
    /// either the partial fresh owner or a newly captured later owner.
    pub(crate) fn observe_for_return_retaining(
        fresh: &mut Option<Self>,
        later: &mut Option<LaterContextRoots>,
        originals: &RetainedContextRoots,
        quarantine: Arc<PrivateDirectory>,
        boundary: &SnapshotBoundary,
        user: &CurrentUser,
    ) -> io::Result<()> {
        if later.is_some() {
            return Err(blocked("later observation already retained"));
        }
        let source = fresh
            .as_mut()
            .ok_or_else(|| blocked("fresh create custody missing"))?;
        if &source.binding != boundary.binding() {
            return Err(blocked("fresh return boundary differs"));
        }
        safe(boundary.verify_live())?;
        originals.verify(user)?;
        quarantine.verify(user)?;
        let mut roots = BTreeMap::new();
        for kind in [RootKind::Desk, RootKind::WebView] {
            if let Some(root) = source.return_roots.get(&kind) {
                roots.insert(kind, root.clone());
                continue;
            }
            let held = source
                .attempts
                .get(&kind)
                .and_then(|attempt| attempt.tree.as_ref())
                .map(|tree| tree.root.clone());
            let root = match held {
                Some(HeldRoot::Present(root)) => HeldRoot::Present(root),
                _ => observe_renameable(&source.origins[&kind])?,
            };
            source.return_roots.insert(kind, root.clone());
            roots.insert(kind, root);
        }
        let context = HeldContext::capture_durable(
            roots.remove(&RootKind::Desk).expect("Desk observed"),
            roots.remove(&RootKind::WebView).expect("UDF observed"),
            SnapshotLimits::default(),
        )?;
        *later = Some(LaterContextRoots {
            binding: source.binding.clone(),
            context,
            origins: source.origins.clone(),
            copies: BTreeMap::new(),
            complete_copies: BTreeMap::new(),
            moved: BTreeMap::new(),
            quarantine,
            absent_preserved: BTreeSet::new(),
            absent_attempts: BTreeMap::new(),
            previous_copies: BTreeMap::new(),
        });
        later
            .as_ref()
            .expect("later observation retained before verification")
            .verify_actual(originals, user)?;
        safe(boundary.verify_live())?;
        drop(fresh.take());
        Ok(())
    }
}
fn observe_renameable(slot: &OriginSlot) -> io::Result<HeldRoot> {
    let observed = slot.observe()?;
    match observed {
        HeldRoot::Absent { .. } => Ok(observed),
        HeldRoot::Present(root) => {
            let expected = root.identity().clone();
            drop(root);
            // No mutation crosses this observation gap. Parent stays held and
            // the newly acquired root must be the exact positively observed ID.
            Ok(HeldRoot::Present(
                slot.parent.open_for_rename(slot.name.clone(), &expected)?,
            ))
        }
    }
}
impl LaterContextRoots {
    pub(crate) fn root_identities(&self) -> BTreeMap<RootKind, String> {
        self.context.root_identities()
    }
    pub(crate) fn capture_after_exit(
        originals: &RetainedContextRoots,
        desk: HeldRoot,
        webview: HeldRoot,
        quarantine: Arc<PrivateDirectory>,
        boundary: &ReturnBoundary,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        safe(boundary.verify_current_image())?;
        let context = HeldContext::capture_durable(desk, webview, SnapshotLimits::default())?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            if boundary.snapshot().root_identity(kind)
                != Some(context.tree(kind).manifest.location_identity.as_str())
            {
                return Err(blocked("later capture differs from actual quiescent roots"));
            }
        }
        let result = Self {
            binding: boundary.binding().clone(),
            context,
            origins: originals.origins.clone(),
            copies: BTreeMap::new(),
            complete_copies: BTreeMap::new(),
            moved: BTreeMap::new(),
            quarantine,
            absent_preserved: BTreeSet::new(),
            absent_attempts: BTreeMap::new(),
            previous_copies: BTreeMap::new(),
        };
        result.verify_actual(originals, user)?;
        safe(boundary.verify_current_image())?;
        Ok(result)
    }
    fn verify_actual(
        &self,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
    ) -> io::Result<()> {
        originals.verify(user)?;
        self.verify_copy_history(user)?;
        self.context.verify_durable()?;
        self.quarantine.verify(user)?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            let tree = self.context.tree(kind);
            verify_confidential_tree(tree, user)?;
            let origin = &self.origins[&kind];
            match &tree.root {
                HeldRoot::Absent { parent, name } => {
                    if self.moved.contains_key(&kind)
                        || parent.identity() != origin.parent.identity()
                        || !same_component(name, &origin.name)
                    {
                        return Err(blocked("later absence is outside original slot"));
                    }
                }
                HeldRoot::Present(root) => {
                    root.require_renameable()?;
                    let in_origin = occupies(root, &origin.parent, &origin.name)?;
                    let in_private = self
                        .moved
                        .get(&kind)
                        .is_some_and(|move_| root.identity() == &move_.object)
                        && self
                            .moved
                            .get(&kind)
                            .map(|move_| {
                                occupies(root, self.quarantine.directory(), &move_.private_name)
                            })
                            .transpose()?
                            .unwrap_or(false);
                    if !in_origin && !in_private {
                        return Err(blocked("later root occupies unrecorded location"));
                    }
                    if originals.source.trees.values().any(|original| matches!(&original.root, HeldRoot::Present(held) if held.identity() == root.identity())) {
                        return Err(blocked("later context aliases retained original"));
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) fn admit_preinstall_return(
        &self,
        originals: &RetainedContextRoots,
        boundary: &SnapshotBoundary,
        fence: &ImageFence,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        let evidence = PreinstallReturnEvidence {
            originals,
            later: self,
            boundary,
            fence,
            user,
            records: &journal.root,
            lease: journal.exclusive()?,
            binding: &journal.binding,
            generation: journal.generation,
        };
        journal.generation = safe(journal.store.admit_preinstall_return(&evidence))?;
        journal.verify()?;
        originals.admit_operation(boundary, fence, user, journal)?;
        Ok(())
    }
    /// Return before any installer or historical launch was ever admitted.
    pub(crate) fn preserve(
        &mut self,
        originals: &RetainedContextRoots,
        boundary: &SnapshotBoundary,
        fence: &ImageFence,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        self.preserve_with(
            originals,
            ContextAdmission::BeforeInstall { boundary, fence },
            user,
            journal,
        )
    }
    pub(crate) fn preserve_after_exit(
        &mut self,
        originals: &RetainedContextRoots,
        boundary: &ReturnBoundary,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        self.preserve_with(
            originals,
            ContextAdmission::AfterExit(boundary),
            user,
            journal,
        )
    }
    fn preserve_with(
        &mut self,
        originals: &RetainedContextRoots,
        admission: ContextAdmission<'_>,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        admission.verify_store(journal.store)?;
        journal.verify()?;
        self.recapture_current(admission)?;
        // A failed later root may retain a created file whose manifest entry
        // was never acknowledged. Normalize every such attempt before the
        // first root can verify the complete cross-root history.
        self.normalize_partial_copies(originals, user, journal)?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            if self.moved.get(&kind).is_some_and(|move_| !move_.complete) {
                self.reobserve_later(kind, originals, admission, user, journal)?;
            } else if self.absent_attempts.contains_key(&kind)
                && !self.absent_preserved.contains(&kind)
            {
                self.reobserve_absence(kind, originals, admission, user, journal)?;
            }
        }
        admission.admit(originals, user, journal)?;
        self.verify_actual(originals, user)?;
        // Both exact trees will be retained in one role manifest after moves;
        // location identities are fixed-length digests, so preflight that full
        // wrapper before any namespace mutation.
        encoded(&(
            1u32,
            &self.binding,
            &self.binding.target_context,
            self.context
                .trees
                .iter()
                .map(|(kind, tree)| (*kind, &tree.manifest))
                .collect::<Vec<_>>(),
        ))?;
        require_phase(
            journal,
            &[
                JournalPhase::RecoveryRequired,
                JournalPhase::HistoricalActive,
                JournalPhase::InstalledUnconfirmed,
            ],
        )?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            let reusable = self
                .copies
                .get(&kind)
                .and_then(|copy| copy.manifest.as_ref())
                .is_some_and(|manifest| {
                    manifest.source.entries == self.context.tree(kind).manifest.entries
                        && (self.moved.contains_key(&kind)
                            || manifest.source.location_identity
                                == self.context.tree(kind).manifest.location_identity)
                });
            if !reusable {
                let previous = safe(journal.store.context_later_backup(kind))?;
                if let Some((generation, plan)) = &previous {
                    let observed =
                        self.observe_previous_copy(kind, plan, originals, user, journal)?;
                    self.previous_copies.insert(*generation, observed);
                }
                let name = ComponentName::new(OsStr::new(&format!(
                    "later-copy-{}",
                    uuid::Uuid::new_v4()
                )))?;
                let parent = originals.copies[&kind].parent.clone();
                let destination = HeldRoot::Absent {
                    parent: parent.directory().clone(),
                    name: name.clone(),
                };
                let evidence = LaterBackupEvidence {
                    originals,
                    later: self,
                    root: kind,
                    destination: &destination,
                    previous: previous
                        .as_ref()
                        .and_then(|(generation, _)| self.previous_copies.get(generation)),
                    admission,
                    user,
                    records: &journal.root,
                    lease: journal.exclusive()?,
                    binding: &journal.binding,
                    generation: journal.generation,
                };
                let (generation, digest) = safe(journal.store.prepare_later_backup(&evidence))?;
                journal.generation = generation;
                journal.verify()?;
                self.complete_copies.remove(&kind);
                self.copies.insert(kind, PrivateTreeCopy::new(parent, name));
                self.copies
                    .get_mut(&kind)
                    .expect("new later copy retained")
                    .copy_from_plan(
                        self.context.tree(kind),
                        user,
                        journal,
                        Some((generation, digest)),
                    )?;
            }
            self.copies[&kind].verify(user)?;
            self.record_complete(kind, originals, admission, user, journal)?;
        }
        self.verify_recorded_history(originals, user, journal.store)?;
        safe(
            journal
                .store
                .admit_context_capacity(journal.generation, 2, 24),
        )?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            if self.absent_preserved.contains(&kind)
                || self.moved.get(&kind).is_some_and(|move_| move_.complete)
            {
                continue;
            }
            if self.moved.contains_key(&kind) {
                return Err(blocked("later root move requires positive re-observation"));
            }
            admission.admit(originals, user, journal)?;
            self.verify_actual(originals, user)?;
            let before = self.context.tree(kind).manifest.clone();
            let copy = journal.retain(self.copies[&kind].manifest()?)?;
            let prior = journal.retain(&before)?;
            let private_name =
                ComponentName::new(OsStr::new(&format!("later-root-{}", uuid::Uuid::new_v4())))?;
            let destination = HeldRoot::Absent {
                parent: self.quarantine.directory().clone(),
                name: private_name.clone(),
            };
            destination.verify()?;
            let root = match &self.context.tree(kind).root {
                HeldRoot::Present(root) => Some(root.clone()),
                HeldRoot::Absent { .. } => None,
            };
            if let Some(root) = &root {
                root.require_same_volume(self.quarantine.directory())?;
            }
            let pending = journal.begin(
                EffectKind::PreserveRoot {
                    context: self.binding.target_context.clone(),
                    root: kind,
                },
                &ContextLocationPlan {
                    schema: 1,
                    root: kind,
                    context_id: self.binding.target_context.clone(),
                    source: prior.clone(),
                    copy: CopyReference {
                        parent: self.copies[&kind].parent.directory().identity().clone(),
                        name: text(&self.copies[&kind].name)?,
                        manifest: copy.clone(),
                    },
                    origin: self.origins[&kind].record()?,
                    retained: root.as_ref().map(|_| RootSlotRecord {
                        parent: self.quarantine.directory().identity().clone(),
                        name: text(&private_name).expect("validated name"),
                    }),
                    object: root.as_ref().map(|root| root.identity().clone()),
                },
                &(
                    destination.location_identity()?,
                    root.as_ref().map(|root| root.identity()),
                ),
            )?;
            if let Some(root) = root {
                self.moved.insert(
                    kind,
                    ContextMove {
                        pending,
                        prior: before.clone(),
                        private_name: private_name.clone(),
                        object: root.identity().clone(),
                        complete: false,
                    },
                );
                let tree = self.context.trees.get_mut(&kind).expect("later root");
                tree.durably_flushed = false;
                tree.entries.clear();
                let result = (|| {
                    admission.verify_live()?;
                    originals.verify(user)?;
                    journal.verify()?;
                    root.rename_to(self.quarantine.directory().clone(), private_name)?;
                    copy_fault(CopyFault::AfterLaterMove)?;
                    let mut budget = Budget::new(self.context.tree(kind).limits)?;
                    budget.flush_files = true;
                    let observed = HeldTree::admit(HeldRoot::Present(root), &mut budget, None)?;
                    self.context.trees.insert(kind, observed);
                    if self.context.tree(kind).manifest.entries != before.entries {
                        return Err(blocked("later root changed during guard gap"));
                    }
                    self.origins[&kind].absent().verify()?;
                    self.copies[&kind].verify(user)?;
                    let after = journal.retain(&self.context.tree(kind).manifest)?;
                    let pending = &self.moved[&kind].pending;
                    journal.applied(
                        PendingEffect {
                            id: pending.id.clone(),
                            generation: pending.generation,
                        },
                        &(&prior, &after, &copy),
                    )?;
                    admission.admit(originals, user, journal)?;
                    self.moved
                        .get_mut(&kind)
                        .expect("later move retained")
                        .complete = true;
                    Ok(())
                })();
                if let Err(error) = result {
                    let pending = &self.moved[&kind].pending;
                    let _ = journal.unknown(PendingEffect {
                        id: pending.id.clone(),
                        generation: pending.generation,
                    });
                    return Err(error);
                }
            } else {
                self.absent_attempts.insert(
                    kind,
                    PendingEffect {
                        id: pending.id.clone(),
                        generation: pending.generation,
                    },
                );
                self.origins[&kind].absent().verify()?;
                journal.applied(pending, &(&prior, &copy, "later-root-absent"))?;
                self.absent_preserved.insert(kind);
            }
        }
        self.verify_preserved(originals, user)
    }
    fn recapture_current(&mut self, admission: ContextAdmission<'_>) -> io::Result<()> {
        admission.verify_live()?;
        let roots: BTreeMap<_, _> = self
            .context
            .trees
            .iter()
            .map(|(kind, tree)| (*kind, tree.root.clone()))
            .collect();
        for (kind, root) in &roots {
            if let HeldRoot::Present(root) = root {
                if let Some(ticket) = self.moved.get(kind) {
                    if occupies(root, self.quarantine.directory(), &ticket.private_name)? {
                        root.reconcile_location(
                            self.quarantine.directory().clone(),
                            ticket.private_name.clone(),
                        )?;
                    } else if occupies(root, &self.origins[kind].parent, &self.origins[kind].name)?
                    {
                        root.reconcile_location(
                            self.origins[kind].parent.clone(),
                            self.origins[kind].name.clone(),
                        )?;
                    } else {
                        return Err(blocked("later object is outside recorded slots"));
                    }
                }
            }
        }
        for tree in self.context.trees.values_mut() {
            tree.durably_flushed = false;
            tree.entries.clear();
        }
        self.context = HeldContext::capture_durable(
            roots[&RootKind::Desk].clone(),
            roots[&RootKind::WebView].clone(),
            SnapshotLimits::default(),
        )?;
        admission.verify_live()?;
        Ok(())
    }
    fn normalize_partial_copies(
        &mut self,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        for root in [RootKind::Desk, RootKind::WebView] {
            if self
                .copies
                .get(&root)
                .is_some_and(|copy| copy.attempted && copy.manifest.is_none())
            {
                let (generation, plan) = safe(journal.store.context_later_backup(root))?
                    .ok_or_else(|| blocked("partial later copy has no owned plan"))?;
                if self.copies[&root].plan_generation != Some(generation) {
                    return Err(blocked("partial later copy belongs to a stale plan"));
                }
                let observed = self.observe_previous_copy(root, &plan, originals, user, journal)?;
                self.previous_copies.insert(generation, observed);
                self.complete_copies.remove(&root);
            }
        }
        Ok(())
    }
    fn observe_previous_copy(
        &mut self,
        kind: RootKind,
        plan: &crate::version_history::journal::LaterBackupPlan,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<HeldTree> {
        let destination: RecoveryDestination =
            serde_json::from_slice(&safe(journal.store.read_manifest(&plan.destination))?)?;
        let expected_parent = originals.copies[&kind].parent.clone();
        if expected_parent.directory().identity() != &destination.parent {
            return Err(blocked("previous later parent differs"));
        }
        let expected_name = ComponentName::new(OsStr::new(&destination.name))?;
        let old = self
            .copies
            .remove(&kind)
            .unwrap_or_else(|| PrivateTreeCopy::new(expected_parent.clone(), expected_name));
        if old.parent.directory().identity() != &destination.parent
            || text(&old.name)? != destination.name
        {
            self.copies.insert(kind, old);
            return Err(blocked("previous later destination differs"));
        }
        let mut old = old;
        let root = match old.tree.as_ref().map(|tree| tree.root.clone()) {
            Some(HeldRoot::Present(root)) => HeldRoot::Present(root),
            _ => HeldRoot::observe(old.parent.directory().clone(), old.name.clone())?,
        };
        if let Some(tree) = old.tree.as_mut() {
            tree.entries.clear();
        }
        drop(old);
        let observed = HeldTree::admit(root, &mut Budget::new(SnapshotLimits::default())?, None)?;
        verify_private_tree(&observed, user)?;
        let history = safe(journal.store.context_later_history(kind))?;
        let record = history
            .last()
            .ok_or_else(|| blocked("missing prior later plan"))?;
        if &record.plan != plan {
            return Err(blocked("prior later plan changed"));
        }
        verify_later_attempt(
            record,
            &observed,
            None,
            &expected_parent,
            user,
            journal.store,
        )?;
        Ok(observed)
    }
    fn reobserve_later(
        &mut self,
        kind: RootKind,
        originals: &RetainedContextRoots,
        admission: ContextAdmission<'_>,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        let tree = self
            .context
            .trees
            .get_mut(&kind)
            .expect("later root retained");
        let HeldRoot::Present(root) = &tree.root else {
            return Err(blocked("later object missing"));
        };
        let root = root.clone();
        let ticket = &self.moved[&kind];
        if root.identity() != &ticket.object {
            return Err(blocked("later object identity differs"));
        }
        if occupies(&root, self.quarantine.directory(), &ticket.private_name)? {
            root.reconcile_location(
                self.quarantine.directory().clone(),
                ticket.private_name.clone(),
            )?;
        } else if occupies(
            &root,
            &self.origins[&kind].parent,
            &self.origins[&kind].name,
        )? {
            root.reconcile_location(
                self.origins[&kind].parent.clone(),
                self.origins[&kind].name.clone(),
            )?;
        } else {
            return Err(blocked("later object is outside recorded slots"));
        }
        let mut budget = Budget::new(tree.limits)?;
        budget.flush_files = true;
        tree.durably_flushed = false;
        tree.entries.clear();
        *tree = HeldTree::admit(HeldRoot::Present(root), &mut budget, None)?;
        let evidence = ContextRootEvidence {
            originals,
            later: self,
            restore: None,
            later_move: Some((kind, &self.moved[&kind])),
            later_absence: None,
            admission,
            user,
            records: &journal.root,
            lease: journal.exclusive()?,
            binding: &journal.binding,
            generation: journal.generation,
        };
        let request = evidence.verify(journal.store)?;
        let state = safe(journal.store.inspect(&journal.binding))?
            .last_valid
            .ok_or_else(|| blocked("missing later journal"))?;
        if state.effect_observation(&request.effect_id) == Some(Observation::Applied) {
            if !request.completed {
                return Err(blocked("applied later move is no longer at destination"));
            }
        } else {
            journal.generation = safe(journal.store.confirm_context_root(&evidence))?;
        }
        journal.verify()?;
        if request.completed {
            self.moved
                .get_mut(&kind)
                .expect("later move retained")
                .complete = true;
        } else {
            self.moved.remove(&kind);
        }
        Ok(())
    }
    pub(crate) fn verify_preserved(
        &self,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
    ) -> io::Result<()> {
        self.verify_actual(originals, user)?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            self.origins[&kind].absent().verify()?;
            let copy = self
                .copies
                .get(&kind)
                .ok_or_else(|| blocked("later copy is incomplete"))?;
            copy.verify(user)?;
            if self.complete_copies.get(&kind).copied() != copy.plan_generation
                || copy.plan_generation.is_none()
            {
                return Err(blocked("later complete mapping is not durably published"));
            }
            if copy.manifest()?.source.entries != self.context.tree(kind).manifest.entries {
                return Err(blocked("later copy contents differ"));
            }
            if !self.context.tree(kind).manifest.entries.is_empty()
                && self.moved.get(&kind).is_none_or(|move_| !move_.complete)
            {
                return Err(blocked("later root outcome is uncertain"));
            }
        }
        Ok(())
    }
    pub(crate) fn manifest_bytes(
        &self,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
    ) -> io::Result<Vec<u8>> {
        self.verify_preserved(originals, user)?;
        encoded(&(
            1u32,
            &self.binding,
            &self.binding.target_context,
            self.context
                .trees
                .iter()
                .map(|(kind, tree)| (*kind, &tree.manifest))
                .collect::<Vec<_>>(),
        ))
    }
}

pub(crate) struct PreinstallReturnEvidence<'a> {
    originals: &'a RetainedContextRoots,
    later: &'a LaterContextRoots,
    boundary: &'a SnapshotBoundary,
    fence: &'a ImageFence,
    user: &'a CurrentUser,
    records: &'a PrivateDirectory,
    lease: &'a ExclusiveLease,
    binding: &'a JournalBinding,
    generation: u64,
}
pub(crate) struct PreinstallReturnRequest {
    pub(crate) generation: u64,
    pub(crate) roots: BTreeMap<RootKind, Vec<u8>>,
    pub(crate) pending: Option<(String, u64)>,
}
impl PreinstallReturnEvidence<'_> {
    pub(crate) fn verify(&self, store: &mut JournalStore) -> io::Result<PreinstallReturnRequest> {
        safe(self.boundary.verify_live())?;
        self.fence.verify()?;
        self.lease.verify_root(self.records)?;
        safe(store.verify_windows_binding(self.records, self.binding, self.generation))?;
        self.later.verify_actual(self.originals, self.user)?;
        if self.boundary.binding() != self.binding || &self.later.binding != self.binding {
            return Err(blocked("early context return binding differs"));
        }
        let pending = safe(store.context_pending())?;
        if let Some((effect, _)) = &pending {
            let EffectKind::CreateFreshRoot { root } = effect.kind else {
                return Err(blocked("pending operation is not fresh creation"));
            };
            let (manifest, slot, label): (String, RootSlotRecord, String) =
                serde_json::from_slice(&safe(store.read_manifest(&effect.before))?)?;
            let origin = self.originals.origins[&root].record()?;
            if manifest != safe(self.originals.expected.digest())?
                || slot.parent != origin.parent
                || slot.name != origin.name
                || label != "absent"
            {
                return Err(blocked("fresh creation selectors differ"));
            }
        }
        Ok(PreinstallReturnRequest {
            generation: self.generation,
            roots: self
                .later
                .context
                .trees
                .iter()
                .map(|(kind, tree)| Ok((*kind, tree.manifest.encode()?)))
                .collect::<io::Result<_>>()?,
            pending: pending.map(|(effect, generation)| (effect.effect_id, generation)),
        })
    }
}

struct SourceRestoreMove {
    pending: PendingEffect,
    prior: Option<OriginSlot>,
    object: Option<FileIdentity>,
}
pub(crate) struct ContextRestoration {
    originals: RetainedContextRoots,
    later: LaterContextRoots,
    moves: BTreeMap<RootKind, SourceRestoreMove>,
    completed: BTreeSet<RootKind>,
}
/// Actual final same-object/readback proof. Its complete originals and later
/// state stay held when consumed by the coordinator's terminal proof factory.
pub(crate) struct RestoredContextRoots {
    originals: RetainedContextRoots,
    later: LaterContextRoots,
}
impl ContextRestoration {
    pub(crate) fn new(
        originals: RetainedContextRoots,
        later: LaterContextRoots,
    ) -> io::Result<Self> {
        Self::new_retaining(&mut Some(originals), &mut Some(later))
    }
    /// Both owners remain in their original slots if admission is rejected.
    pub(crate) fn new_retaining(
        originals: &mut Option<RetainedContextRoots>,
        later: &mut Option<LaterContextRoots>,
    ) -> io::Result<Self> {
        let held_originals = originals
            .as_ref()
            .ok_or_else(|| blocked("original return custody missing"))?;
        let held_later = later
            .as_ref()
            .ok_or_else(|| blocked("later return custody missing"))?;
        if held_originals.expected.context_id != held_later.binding.source_context {
            return Err(blocked("return context binding differs"));
        }
        Ok(Self {
            originals: originals.take().expect("validated original return custody"),
            later: later.take().expect("validated later return custody"),
            moves: BTreeMap::new(),
            completed: BTreeSet::new(),
        })
    }
    pub(crate) fn restore(
        &mut self,
        boundary: &SnapshotBoundary,
        fence: &ImageFence,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        self.restore_with(
            ContextAdmission::BeforeInstall { boundary, fence },
            user,
            journal,
        )
    }
    pub(crate) fn restore_after_exit(
        &mut self,
        boundary: &ReturnBoundary,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        self.restore_with(ContextAdmission::AfterExit(boundary), user, journal)
    }
    fn restore_with(
        &mut self,
        admission: ContextAdmission<'_>,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        admission.verify_store(journal.store)?;
        journal.exclusive()?.verify_root(&journal.root)?;
        journal.verify()?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            if self.moves.contains_key(&kind) && !self.completed.contains(&kind) {
                self.readmit_original(kind, user)?;
            }
        }
        admission.admit(&self.originals, user, journal)?;
        self.later.verify_retained(&self.originals, user)?;
        require_phase(journal, &[JournalPhase::Restoring])?;
        require_role(
            journal,
            ManifestRole::SourceContext,
            &safe(self.originals.expected.digest())?,
        )?;
        require_role(
            journal,
            ManifestRole::RetainedTargetContext,
            &sha256(&self.later.retained_manifest_bytes(&self.originals, user)?),
        )?;
        safe(
            journal
                .store
                .admit_context_capacity(journal.generation, 2, 24),
        )?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            if self.completed.contains(&kind) {
                continue;
            }
            if self.moves.contains_key(&kind) {
                self.reobserve_source(kind, admission, user, journal)?;
                if self.completed.contains(&kind) {
                    continue;
                }
            }
            admission.admit(&self.originals, user, journal)?;
            self.later.verify_retained(&self.originals, user)?;
            let origin = self.originals.origins[&kind].clone();
            origin.absent().verify()?;
            let before = self.originals.source.tree(kind).manifest.clone();
            let before_digest = journal.retain(&before)?;
            let target_digest = journal.retain(&self.later.context.tree(kind).manifest)?;
            let copy_digest = journal.retain(self.originals.copies[&kind].manifest()?)?;
            let root = match &self.originals.source.tree(kind).root {
                HeldRoot::Present(root) => Some(root.clone()),
                HeldRoot::Absent { .. } => None,
            };
            if let Some(root) = &root {
                root.require_same_volume(&origin.parent)?;
            }
            let pending = journal.begin(
                EffectKind::RestoreSourceRoot { root: kind },
                &ContextLocationPlan {
                    schema: 1,
                    root: kind,
                    context_id: journal.binding.source_context.clone(),
                    source: before_digest.clone(),
                    copy: CopyReference {
                        parent: self.originals.copies[&kind]
                            .parent
                            .directory()
                            .identity()
                            .clone(),
                        name: text(&self.originals.copies[&kind].name)?,
                        manifest: copy_digest.clone(),
                    },
                    origin: origin.record()?,
                    retained: root
                        .as_ref()
                        .map(|root| {
                            root.held_location()
                                .and_then(|(parent, name)| OriginSlot { parent, name }.record())
                        })
                        .transpose()?,
                    object: root.as_ref().map(|root| root.identity().clone()),
                },
                &(
                    safe(self.originals.expected.digest())?,
                    origin.record()?,
                    "same-original-object-or-original-absence",
                ),
            )?;
            if let Some(root) = root {
                let ticket = self.originals.copies[&kind]
                    .rotation
                    .as_ref()
                    .ok_or_else(|| blocked("original rotation ownership missing"))?;
                self.moves.insert(
                    kind,
                    SourceRestoreMove {
                        pending,
                        prior: Some(OriginSlot {
                            parent: ticket.quarantine.directory().clone(),
                            name: ticket.quarantine_name.clone(),
                        }),
                        object: Some(root.identity().clone()),
                    },
                );
                let tree = self
                    .originals
                    .source
                    .trees
                    .get_mut(&kind)
                    .expect("original root retained");
                tree.durably_flushed = false;
                tree.entries.clear();
                let result = (|| {
                    admission.verify_store(journal.store)?;
                    journal.verify()?;
                    self.originals.copies[&kind].verify(user)?;
                    copy_fault(CopyFault::BeforeSourceRestoreMove)?;
                    root.rename_to(origin.parent, origin.name)?;
                    copy_fault(CopyFault::AfterSourceRestoreMove)?;
                    self.readmit_original(kind, user)?;
                    self.verify_original_at_origin(kind, user)?;
                    self.later.verify_retained(&self.originals, user)?;
                    let after = journal.retain(&self.originals.source.tree(kind).manifest)?;
                    let pending = &self.moves[&kind].pending;
                    journal.applied(
                        PendingEffect {
                            id: pending.id.clone(),
                            generation: pending.generation,
                        },
                        &(&before_digest, &after, &copy_digest, &target_digest),
                    )?;
                    admission.admit(&self.originals, user, journal)?;
                    self.completed.insert(kind);
                    Ok(())
                })();
                if let Err(error) = result {
                    let pending = &self.moves[&kind].pending;
                    let _ = journal.unknown(PendingEffect {
                        id: pending.id.clone(),
                        generation: pending.generation,
                    });
                    return Err(error);
                }
            } else {
                self.moves.insert(
                    kind,
                    SourceRestoreMove {
                        pending,
                        prior: None,
                        object: None,
                    },
                );
                origin.absent().verify()?;
                self.originals.copies[&kind].verify(user)?;
                let pending = &self.moves[&kind].pending;
                journal.applied(
                    PendingEffect {
                        id: pending.id.clone(),
                        generation: pending.generation,
                    },
                    &(
                        &before_digest,
                        &copy_digest,
                        &target_digest,
                        "original-absence-restored",
                    ),
                )?;
                self.completed.insert(kind);
                admission.admit(&self.originals, user, journal)?;
            }
        }
        self.verify_final(user)
    }
    fn readmit_original(&mut self, kind: RootKind, user: &CurrentUser) -> io::Result<()> {
        let source = self
            .originals
            .source
            .trees
            .get_mut(&kind)
            .expect("original root retained");
        let ticket = &self.moves[&kind];
        let origin = &self.originals.origins[&kind];
        let HeldRoot::Present(root) = &source.root else {
            if ticket.object.is_some()
                || ticket.prior.is_some()
                || !source.manifest.entries.is_empty()
            {
                return Err(blocked("missing original object"));
            }
            origin.absent().verify()?;
            return self.originals.verify(user);
        };
        let root = root.clone();
        if Some(root.identity()) != ticket.object.as_ref() {
            return Err(blocked("original restore object differs"));
        }
        let prior = ticket
            .prior
            .as_ref()
            .ok_or_else(|| blocked("missing original prior slot"))?;
        if occupies(&root, &origin.parent, &origin.name)? {
            root.reconcile_location(origin.parent.clone(), origin.name.clone())?;
        } else if occupies(&root, &prior.parent, &prior.name)? {
            root.reconcile_location(prior.parent.clone(), prior.name.clone())?;
        } else {
            return Err(blocked("original object is outside recorded return slots"));
        }
        let mut budget = Budget::new(source.limits)?;
        budget.flush_files = true;
        source.durably_flushed = false;
        source.entries.clear();
        let observed = HeldTree::admit(HeldRoot::Present(root), &mut budget, None)?;
        // Keep all actual contents even if comparison fails. Never substitute C0.
        *source = observed;
        self.originals
            .retained
            .insert(kind, source.manifest.clone());
        self.originals.verify(user)
    }
    fn verify_original_at_origin(&self, kind: RootKind, user: &CurrentUser) -> io::Result<()> {
        self.originals.verify(user)?;
        let expected = self
            .originals
            .expected
            .roots
            .iter()
            .find(|root| root.root == kind)
            .expect("validated original root");
        let actual = self.originals.source.tree(kind);
        if expected.entries != actual.manifest.entries
            || expected.location_identity != actual.manifest.location_identity
        {
            return Err(blocked(
                "restored original bytes, permissions, or location differ",
            ));
        }
        if expected.entries.is_empty() {
            self.originals.origins[&kind].absent().verify()?;
        } else {
            let HeldRoot::Present(root) = &actual.root else {
                return Err(blocked("restored original is absent"));
            };
            if !occupies(
                root,
                &self.originals.origins[&kind].parent,
                &self.originals.origins[&kind].name,
            )? {
                return Err(blocked("original has not returned"));
            }
        }
        Ok(())
    }
    fn reobserve_source(
        &mut self,
        kind: RootKind,
        admission: ContextAdmission<'_>,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        self.readmit_original(kind, user)?;
        let ticket = &self.moves[&kind];
        let evidence = ContextRootEvidence {
            originals: &self.originals,
            later: &self.later,
            restore: Some((kind, ticket)),
            later_move: None,
            later_absence: None,
            admission,
            user,
            records: &journal.root,
            lease: journal.exclusive()?,
            binding: &journal.binding,
            generation: journal.generation,
        };
        let request = evidence.verify(journal.store)?;
        let state = safe(journal.store.inspect(&journal.binding))?
            .last_valid
            .ok_or_else(|| blocked("missing return journal"))?;
        if state.effect_observation(&ticket.pending.id) == Some(Observation::Applied) {
            if !request.completed {
                return Err(blocked("applied restore no longer matches original slot"));
            }
        } else {
            journal.generation = safe(journal.store.confirm_context_root(&evidence))?;
        }
        journal.verify()?;
        if request.completed {
            self.completed.insert(kind);
        } else {
            // A fresh, verified prior-slot observation permits a new intent;
            // the old Unknown remains in the journal and is never replayed.
            self.moves.remove(&kind);
        }
        Ok(())
    }
    fn verify_final(&self, user: &CurrentUser) -> io::Result<()> {
        if self.completed.len() != 2 {
            return Err(blocked("original context return is incomplete"));
        }
        self.later.verify_retained(&self.originals, user)?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            self.verify_original_at_origin(kind, user)?;
        }
        Ok(())
    }
    pub(crate) fn finish(self, user: &CurrentUser) -> io::Result<RestoredContextRoots> {
        Self::finish_retaining(&mut Some(self), user)
    }
    /// A failed final readback must retain the entire restoration, including
    /// incomplete moves and both original and later user-state owners.
    pub(crate) fn finish_retaining(
        restoration: &mut Option<Self>,
        user: &CurrentUser,
    ) -> io::Result<RestoredContextRoots> {
        restoration
            .as_ref()
            .ok_or_else(|| blocked("context restoration custody missing"))?
            .verify_final(user)?;
        let restored = restoration.take().expect("validated context restoration");
        Ok(RestoredContextRoots {
            originals: restored.originals,
            later: restored.later,
        })
    }
}
impl RestoredContextRoots {
    pub(crate) fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        self.originals.verify(user)?;
        self.later.verify_retained(&self.originals, user)?;
        for expected in &self.originals.expected.roots {
            let actual = self.originals.source.tree(expected.root);
            actual.verify()?;
            if actual.manifest.entries != expected.entries
                || actual.manifest.location_identity != expected.location_identity
            {
                return Err(blocked("final original context changed"));
            }
        }
        Ok(())
    }
    pub(crate) fn original_snapshot(&self) -> &SnapshotManifest {
        &self.originals.expected
    }
}
impl LaterContextRoots {
    fn verify_retained(
        &self,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
    ) -> io::Result<()> {
        originals.verify(user)?;
        self.verify_copy_history(user)?;
        self.quarantine.verify(user)?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            let copy = self
                .copies
                .get(&kind)
                .ok_or_else(|| blocked("later private copy missing"))?;
            copy.verify(user)?;
            if self.complete_copies.get(&kind).copied() != copy.plan_generation
                || copy.plan_generation.is_none()
            {
                return Err(blocked("retained later complete mapping is not published"));
            }
            let tree = self.context.tree(kind);
            if tree.manifest.entries != copy.manifest()?.source.entries {
                return Err(blocked("retained later contents changed"));
            }
            if let HeldRoot::Present(root) = &tree.root {
                tree.verify()?;
                let move_ = self
                    .moved
                    .get(&kind)
                    .ok_or_else(|| blocked("later rotation ownership missing"))?;
                if !move_.complete
                    || root.identity() != &move_.object
                    || !occupies(root, self.quarantine.directory(), &move_.private_name)?
                {
                    return Err(blocked("later context is not certainly preserved"));
                }
            } else if !tree.manifest.entries.is_empty() || !self.absent_preserved.contains(&kind) {
                return Err(blocked("later absence differs or is not durably preserved"));
            }
        }
        Ok(())
    }
    fn retained_manifest_bytes(
        &self,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
    ) -> io::Result<Vec<u8>> {
        self.verify_retained(originals, user)?;
        encoded(&(
            1u32,
            &self.binding,
            &self.binding.target_context,
            self.context
                .trees
                .iter()
                .map(|(kind, tree)| (*kind, &tree.manifest))
                .collect::<Vec<_>>(),
        ))
    }
}
pub(crate) struct ContextRootEvidence<'a> {
    originals: &'a RetainedContextRoots,
    later: &'a LaterContextRoots,
    restore: Option<(RootKind, &'a SourceRestoreMove)>,
    later_move: Option<(RootKind, &'a ContextMove)>,
    later_absence: Option<(RootKind, &'a PendingEffect)>,
    admission: ContextAdmission<'a>,
    user: &'a CurrentUser,
    records: &'a PrivateDirectory,
    lease: &'a ExclusiveLease,
    binding: &'a JournalBinding,
    generation: u64,
}
pub(crate) struct ContextRootRequest {
    pub(crate) generation: u64,
    pub(crate) effect_id: String,
    pub(crate) intent_generation: u64,
    pub(crate) current: Vec<u8>,
    pub(crate) completed: bool,
}
impl ContextRootEvidence<'_> {
    pub(crate) fn verify(&self, store: &mut JournalStore) -> io::Result<ContextRootRequest> {
        self.admission.verify_store(store)?;
        self.lease.verify_root(self.records)?;
        safe(store.verify_windows_binding(self.records, self.binding, self.generation))?;
        self.originals.verify(self.user)?;
        if let Some((kind, pending)) = self.later_absence {
            self.later.verify_actual(self.originals, self.user)?;
            let tree = self.later.context.tree(kind);
            if !tree.manifest.entries.is_empty() || !matches!(tree.root, HeldRoot::Absent { .. }) {
                return Err(blocked("later absent-root observation changed"));
            }
            self.later.origins[&kind].absent().verify()?;
            self.later.copies[&kind].verify(self.user)?;
            let (effect, generation) = safe(store.context_rotation(&pending.id))?;
            if effect.kind
                != (EffectKind::PreserveRoot {
                    context: self.binding.target_context.clone(),
                    root: kind,
                })
                || generation != pending.generation
            {
                return Err(blocked("later absence effect differs"));
            }
            let plan: ContextLocationPlan =
                serde_json::from_slice(&safe(store.read_manifest(&effect.before))?)?;
            validate_location_plan(
                &plan,
                kind,
                &self.binding.target_context,
                &self.later.origins[&kind],
                &self.originals.copies[&kind].parent,
                &self.later.quarantine,
            )?;
            if plan.object.is_some() || plan.retained.is_some() {
                return Err(blocked("absence plan differs"));
            }
            return Ok(ContextRootRequest {
                generation: self.generation,
                effect_id: pending.id.clone(),
                intent_generation: pending.generation,
                current: tree.manifest.encode()?,
                completed: true,
            });
        }
        if let Some((kind, ticket)) = self.later_move {
            self.later.verify_actual(self.originals, self.user)?;
            if self.admission.binding() != self.binding {
                return Err(blocked("later root boundary differs"));
            }
            let tree = self.later.context.tree(kind);
            let HeldRoot::Present(root) = &tree.root else {
                return Err(blocked("later root object missing"));
            };
            self.later.copies[&kind].verify(self.user)?;
            if root.identity() != &ticket.object {
                return Err(blocked("later re-observation object differs"));
            }
            // This confirms only the actual namespace outcome. Prior C0 remains
            // retained evidence, and changed current data must receive a distinct
            // complete private copy before verify_preserved can succeed.
            let (effect, generation) = safe(store.context_rotation(&ticket.pending.id))?;
            if effect.kind
                != (EffectKind::PreserveRoot {
                    context: self.binding.target_context.clone(),
                    root: kind,
                })
                || generation != ticket.pending.generation
            {
                return Err(blocked("later preservation effect differs"));
            }
            let plan: ContextLocationPlan =
                serde_json::from_slice(&safe(store.read_manifest(&effect.before))?)?;
            validate_location_plan(
                &plan,
                kind,
                &self.binding.target_context,
                &self.later.origins[&kind],
                &self.originals.copies[&kind].parent,
                &self.later.quarantine,
            )?;
            if plan.object.as_ref() != Some(&ticket.object)
                || plan.source != ticket.prior.digest()?
                || plan.retained.as_ref().map(|slot| slot.name.as_str())
                    != Some(text(&ticket.private_name)?.as_str())
            {
                return Err(blocked("later observation selectors differ"));
            }
            let completed = occupies(
                root,
                self.later.quarantine.directory(),
                &ticket.private_name,
            )?;
            if completed {
                self.later.origins[&kind].absent().verify()?;
            } else if !occupies(
                root,
                &self.later.origins[&kind].parent,
                &self.later.origins[&kind].name,
            )? {
                return Err(blocked("later object occupies unrecorded slot"));
            }
            return Ok(ContextRootRequest {
                generation: self.generation,
                effect_id: ticket.pending.id.clone(),
                intent_generation: ticket.pending.generation,
                current: tree.manifest.encode()?,
                completed,
            });
        }
        self.later.verify_retained(self.originals, self.user)?;
        if self.admission.binding() != self.binding {
            return Err(blocked("root return boundary differs"));
        }
        let (kind, ticket) = self
            .restore
            .ok_or_else(|| blocked("missing concrete root restore witness"))?;
        let tree = self.originals.source.tree(kind);
        tree.verify()?;
        let (effect, generation) = safe(store.context_rotation(&ticket.pending.id))?;
        if effect.kind != (EffectKind::RestoreSourceRoot { root: kind })
            || generation != ticket.pending.generation
        {
            return Err(blocked("root restore effect differs"));
        }
        let origin = &self.originals.origins[&kind];
        let HeldRoot::Present(root) = &tree.root else {
            if ticket.object.is_some()
                || ticket.prior.is_some()
                || !tree.manifest.entries.is_empty()
            {
                return Err(blocked("original absence witness differs"));
            }
            origin.absent().verify()?;
            return Ok(ContextRootRequest {
                generation: self.generation,
                effect_id: ticket.pending.id.clone(),
                intent_generation: ticket.pending.generation,
                current: tree.manifest.encode()?,
                completed: true,
            });
        };
        if Some(root.identity()) != ticket.object.as_ref() {
            return Err(blocked("root restore identity differs"));
        }
        let completed = occupies(root, &origin.parent, &origin.name)?;
        if !completed {
            let prior = ticket
                .prior
                .as_ref()
                .ok_or_else(|| blocked("original prior slot missing"))?;
            if !occupies(root, &prior.parent, &prior.name)? {
                return Err(blocked("root occupies unknown restore location"));
            }
            origin.absent().verify()?;
        } else {
            let expected = self
                .originals
                .expected
                .roots
                .iter()
                .find(|root| root.root == kind)
                .expect("validated source root");
            if tree.manifest.entries != expected.entries
                || tree.manifest.location_identity != expected.location_identity
            {
                return Err(blocked(
                    "returned source differs from complete original snapshot",
                ));
            }
        }
        Ok(ContextRootRequest {
            generation: self.generation,
            effect_id: ticket.pending.id.clone(),
            intent_generation: ticket.pending.generation,
            current: tree.manifest.encode()?,
            completed,
        })
    }
}

impl RetainedContextRoots {
    /// Rebuild observation ownership from this exact journal, supplied held
    /// original parents, and private storage. Serialized selectors never supply
    /// an absolute path or substitute for fresh file flush/readback.
    pub(crate) fn reopen_observation(
        original_parents: BTreeMap<RootKind, Arc<Directory>>,
        copies_parent: Arc<PrivateDirectory>,
        quarantine: Arc<PrivateDirectory>,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        journal.exclusive()?.verify_root(&journal.root)?;
        journal.verify()?;
        copies_parent.verify(user)?;
        quarantine.verify(user)?;
        let inspection = safe(journal.store.inspect(&journal.binding))?;
        let state = inspection
            .last_valid
            .ok_or_else(|| blocked("source checkpoint missing"))?;
        let snapshot_digest = state
            .manifest(ManifestRole::SourceContext)
            .ok_or_else(|| blocked("source context is not sealed"))?;
        let expected = safe(SnapshotManifest::decode(
            &safe(journal.store.read_manifest(snapshot_digest))?,
            snapshot_digest,
            &journal.binding,
        ))?;
        if expected.context_id != journal.binding.source_context || original_parents.len() != 2 {
            return Err(blocked("source reopen binding differs"));
        }
        let mut trees = BTreeMap::new();
        let mut copies = BTreeMap::new();
        let mut origins = BTreeMap::new();
        let mut retained = BTreeMap::new();
        let mut budget = Budget::new(SnapshotLimits::default())?;
        budget.flush_files = true;
        for kind in [RootKind::Desk, RootKind::WebView] {
            let (effect, _) = safe(
                journal
                    .store
                    .context_preservation(&journal.binding.source_context, kind),
            )?;
            let plan: PreservedSourcePlan =
                serde_json::from_slice(&safe(journal.store.read_manifest(&effect.before))?)?;
            let parent = original_parents
                .get(&kind)
                .ok_or_else(|| blocked("source parent missing"))?;
            parent.recheck()?;
            if plan.schema != 1
                || plan.root != kind
                || plan.context_id != journal.binding.source_context
                || plan.snapshot != snapshot_digest
                || plan.origin.parent != *parent.identity()
                || plan.copy.parent != *copies_parent.directory().identity()
            {
                return Err(blocked("source selectors differ from held authority"));
            }
            let origin = OriginSlot {
                parent: parent.clone(),
                name: ComponentName::new(OsStr::new(&plan.origin.name))?,
            };
            let saved: PrivateCopyManifest =
                serde_json::from_slice(&safe(journal.store.read_manifest(&plan.copy.manifest))?)?;
            let before: TreeManifest =
                serde_json::from_slice(&safe(journal.store.read_manifest(&plan.before))?)?;
            let historical_retained: TreeManifest =
                serde_json::from_slice(&safe(journal.store.read_manifest(&plan.after))?)?;
            let expected_root = expected
                .roots
                .iter()
                .find(|root| root.root == kind)
                .expect("decoded complete source manifest");
            if before.schema != 1
                || historical_retained.schema != 1
                || saved.source != before
                || before.entries != expected_root.entries
                || before.location_identity != expected_root.location_identity
                || historical_retained.entries != before.entries
            {
                return Err(blocked("source snapshot and private mapping differ"));
            }
            let (copy, tree) = if before.entries.is_empty() {
                if plan.object.is_some()
                    || plan.retained.is_some()
                    || plan.rotation_effect.is_some()
                    || before.location_identity
                        != identity(&(
                            "absent",
                            parent.identity(),
                            parent
                                .path()?
                                .into_string()
                                .map_err(|_| blocked("invalid source parent"))?,
                            text(&origin.name)?,
                        ))?
                {
                    return Err(blocked("sealed source absence differs"));
                }
                let copy = PrivateTreeCopy::reopen(
                    copies_parent.clone(),
                    ComponentName::new(OsStr::new(&plan.copy.name))?,
                    saved,
                    user,
                    SnapshotLimits::default(),
                )?;
                // This is only sealed historical absence; live-slot checks are
                // performed separately while fresh/later state may occupy it.
                let tree = HeldTree {
                    root: origin.absent(),
                    manifest: before,
                    entries: BTreeMap::new(),
                    limits: SnapshotLimits::default(),
                    flush_required: false,
                    durably_flushed: false,
                    detached_image: None,
                    fenced_location: None,
                };
                (copy, tree)
            } else {
                let object = plan
                    .object
                    .as_ref()
                    .ok_or_else(|| blocked("retained source identity missing"))?;
                let retained_slot = plan
                    .retained
                    .as_ref()
                    .ok_or_else(|| blocked("retained source slot missing"))?;
                if retained_slot.parent != *quarantine.directory().identity() {
                    return Err(blocked("retained source parent differs"));
                }
                let private = OriginSlot {
                    parent: quarantine.directory().clone(),
                    name: ComponentName::new(OsStr::new(&retained_slot.name))?,
                };
                let root = find_expected_root(&[private, origin.clone()], object)?;
                let tree = HeldTree::admit(HeldRoot::Present(root), &mut budget, None)?;
                if tree.manifest.entries != before.entries {
                    return Err(blocked(
                        "retained source current bytes or permissions changed",
                    ));
                }
                let rotation = plan
                    .rotation_effect
                    .as_deref()
                    .ok_or_else(|| blocked("source rotation effect missing"))?;
                let copy = PrivateTreeCopy::reopen_rotation(
                    copies_parent.clone(),
                    parent.clone(),
                    quarantine.clone(),
                    rotation,
                    user,
                    SnapshotLimits::default(),
                    journal,
                )?;
                if copy.manifest()? != &saved || text(&copy.name)? != plan.copy.name {
                    return Err(blocked("retained source copy selector differs"));
                }
                (copy, tree)
            };
            retained.insert(kind, tree.manifest.clone());
            trees.insert(kind, tree);
            copies.insert(kind, copy);
            origins.insert(kind, origin);
        }
        let result = Self {
            source: HeldContext { trees },
            copies,
            expected,
            origins,
            retained,
        };
        result.verify(user)?;
        journal.verify()?;
        Ok(result)
    }
}
fn find_expected_root(slots: &[OriginSlot], expected: &FileIdentity) -> io::Result<Arc<Directory>> {
    for slot in slots {
        let observed = slot.observe()?;
        if let HeldRoot::Present(root) = observed {
            if root.identity() == expected {
                drop(root);
                return slot.parent.open_for_rename(slot.name.clone(), expected);
            }
        }
    }
    Err(blocked("original object is absent from all recorded slots"))
}

/// Exact current later-root copy admission. Original source C0, quiescence,
/// both current roots, prior private attempt, journal and lease stay held.
pub(crate) struct LaterBackupEvidence<'a> {
    originals: &'a RetainedContextRoots,
    later: &'a LaterContextRoots,
    root: RootKind,
    destination: &'a HeldRoot,
    previous: Option<&'a HeldTree>,
    admission: ContextAdmission<'a>,
    user: &'a CurrentUser,
    records: &'a PrivateDirectory,
    lease: &'a ExclusiveLease,
    binding: &'a JournalBinding,
    generation: u64,
}
pub(crate) struct LaterBackupRequest {
    pub(crate) root: RootKind,
    pub(crate) generation: u64,
    pub(crate) source: Vec<u8>,
    pub(crate) destination: Vec<u8>,
    pub(crate) previous: Option<Vec<u8>>,
    pub(crate) previous_generation: Option<u64>,
    pub(crate) abandoned_effect: Option<(String, u64)>,
    pub(crate) source_reservation: String,
    pub(crate) effects: usize,
}
impl LaterBackupEvidence<'_> {
    pub(crate) fn verify(&self, store: &mut JournalStore) -> io::Result<LaterBackupRequest> {
        self.admission.verify_store(store)?;
        self.lease.verify_root(self.records)?;
        safe(store.verify_windows_binding(self.records, self.binding, self.generation))?;
        self.later.verify_actual(self.originals, self.user)?;
        self.later
            .verify_recorded_history(self.originals, self.user, store)?;
        if self.admission.binding() != self.binding || &self.later.binding != self.binding {
            return Err(blocked("later copy binding differs"));
        }
        let original = safe(self.originals.expected.encode())?;
        safe(SnapshotManifest::decode(
            &original,
            &sha256(&original),
            self.binding,
        ))?;
        let tree = self.later.context.tree(self.root);
        let source = tree.manifest.encode()?;
        if source
            .len()
            .checked_mul(2)
            .and_then(|bytes| {
                bytes.checked_add(tree.manifest.entries.len().saturating_mul(1024) + 4096)
            })
            .is_none_or(|bytes| bytes > MAX_MANIFEST_BYTES)
        {
            return Err(blocked("later private copy wrapper exceeds capacity"));
        }
        self.destination.verify()?;
        require_distinct_roots(&tree.root, self.destination)?;
        let HeldRoot::Absent { parent, name } = self.destination else {
            return Err(blocked("later copy destination is not absent"));
        };
        if parent.identity()
            != self.originals.copies[&self.root]
                .parent
                .directory()
                .identity()
        {
            return Err(blocked(
                "later copy parent is not the retained private parent",
            ));
        }
        self.originals.copies[&self.root].parent.verify(self.user)?;
        let prior = safe(store.context_later_backup(self.root))?;
        let previous = match (&prior, self.previous) {
            (None, None) => None,
            (Some((_, plan)), Some(observed)) => {
                verify_private_tree(observed, self.user)?;
                let saved: RecoveryDestination =
                    serde_json::from_slice(&safe(store.read_manifest(&plan.destination))?)?;
                let old_name = ComponentName::new(OsStr::new(&saved.name))?;
                if saved.parent != *parent.identity() || same_component(name, &old_name) {
                    return Err(blocked("later copy would reuse a prior destination"));
                }
                let matches = match &observed.root {
                    HeldRoot::Present(root) => occupies(root, parent, &old_name)?,
                    HeldRoot::Absent {
                        parent: old_parent,
                        name: old,
                    } => {
                        old_parent.identity() == parent.identity() && same_component(old, &old_name)
                    }
                };
                if !matches {
                    return Err(blocked("previous later copy observation differs"));
                }
                Some(observed.manifest.encode()?)
            }
            _ => return Err(blocked("prior later copy observation missing")),
        };
        let abandoned_effect = if let Some((effect, generation)) = safe(store.context_pending())? {
            if !matches!(&effect.kind, EffectKind::PrivateBackupEntry { plan_generation,manifest,.. }
                if prior.as_ref().is_some_and(|(old,plan)| old == plan_generation && &plan.source_manifest == manifest))
            {
                return Err(blocked("pending effect does not belong to this later copy"));
            }
            Some((effect.effect_id, generation))
        } else {
            None
        };
        Ok(LaterBackupRequest {
            root: self.root,
            generation: self.generation,
            source,
            destination: encoded(&RecoveryDestination {
                parent: parent.identity().clone(),
                name: text(name)?,
            })?,
            previous,
            previous_generation: prior.map(|(generation, _)| generation),
            abandoned_effect,
            source_reservation: self.originals.copies[&self.root]
                .manifest()?
                .source
                .digest()?,
            effects: tree.manifest.entries.len(),
        })
    }
}

fn validate_location_plan(
    plan: &ContextLocationPlan,
    kind: RootKind,
    context: &str,
    origin: &OriginSlot,
    copies: &PrivateDirectory,
    quarantine: &PrivateDirectory,
) -> io::Result<()> {
    origin.parent.recheck()?;
    if plan.schema != 1
        || plan.root != kind
        || plan.context_id != context
        || plan.origin.parent != *origin.parent.identity()
        || plan.origin.name != text(&origin.name)?
        || plan.copy.parent != *copies.directory().identity()
        || plan.object.is_some() != plan.retained.is_some()
        || plan
            .retained
            .as_ref()
            .is_some_and(|slot| slot.parent != *quarantine.directory().identity())
    {
        return Err(blocked(
            "context location selectors differ from retained authority",
        ));
    }
    ComponentName::new(OsStr::new(&plan.copy.name))?;
    if let Some(slot) = &plan.retained {
        ComponentName::new(OsStr::new(&slot.name))?;
    }
    Ok(())
}
impl LaterContextRoots {
    fn reobserve_absence(
        &mut self,
        kind: RootKind,
        originals: &RetainedContextRoots,
        admission: ContextAdmission<'_>,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        let evidence = ContextRootEvidence {
            originals,
            later: self,
            restore: None,
            later_move: None,
            later_absence: Some((kind, &self.absent_attempts[&kind])),
            admission,
            user,
            records: &journal.root,
            lease: journal.exclusive()?,
            binding: &journal.binding,
            generation: journal.generation,
        };
        journal.generation = safe(journal.store.confirm_context_root(&evidence))?;
        journal.verify()?;
        self.absent_preserved.insert(kind);
        Ok(())
    }
    /// Rehydrates actual owned objects through backend-held parents. It grants
    /// no return admission: every later mutation still needs a fresh boundary.
    pub(crate) fn reopen_observation(
        originals: &RetainedContextRoots,
        quarantine: Arc<PrivateDirectory>,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        journal.verify()?;
        journal.exclusive()?.verify_root(&journal.root)?;
        originals.verify(user)?;
        quarantine.verify(user)?;
        let state = safe(journal.store.inspect(&journal.binding))?
            .last_valid
            .ok_or_else(|| blocked("missing context return journal"))?;
        let restoring = matches!(
            state.phase(),
            JournalPhase::Restoring | JournalPhase::Restored
        );
        let mut trees = BTreeMap::new();
        let LaterCopyGuards {
            copies,
            previous: previous_copies,
        } = reopen_later_history(originals, user, journal.store)?;
        let mut moved = BTreeMap::new();
        let mut absent_preserved = BTreeSet::new();
        let mut absent_attempts = BTreeMap::new();
        let mut budget = Budget::new(SnapshotLimits::default())?;
        budget.flush_files = true;
        for kind in [RootKind::Desk, RootKind::WebView] {
            let origin = &originals.origins[&kind];
            let latest = safe(journal.store.latest_context_root_effect(
                &EffectKind::PreserveRoot {
                    context: journal.binding.target_context.clone(),
                    root: kind,
                },
            ))?;
            let Some((effect, generation, complete)) = latest else {
                if restoring {
                    return Err(blocked("restoring context has no preservation intent"));
                }
                trees.insert(
                    kind,
                    HeldTree::admit(observe_renameable(origin)?, &mut budget, None)?,
                );
                continue;
            };
            let plan: ContextLocationPlan =
                serde_json::from_slice(&safe(journal.store.read_manifest(&effect.before))?)?;
            validate_location_plan(
                &plan,
                kind,
                &journal.binding.target_context,
                origin,
                &originals.copies[&kind].parent,
                &quarantine,
            )?;
            let prior: TreeManifest =
                serde_json::from_slice(&safe(journal.store.read_manifest(&plan.source))?)?;
            let saved: PrivateCopyManifest =
                serde_json::from_slice(&safe(journal.store.read_manifest(&plan.copy.manifest))?)?;
            if prior != saved.source {
                return Err(blocked("later preservation source mapping differs"));
            }
            let history = safe(journal.store.context_later_history(kind))?;
            let mut known = false;
            for record in &history {
                if record.complete.as_deref() == Some(plan.copy.manifest.as_str())
                    && record.plan.source_manifest == plan.source
                {
                    let destination: RecoveryDestination = serde_json::from_slice(&safe(
                        journal.store.read_manifest(&record.plan.destination),
                    )?)?;
                    known = destination.parent == plan.copy.parent
                        && destination.name == plan.copy.name;
                    if known {
                        break;
                    }
                }
            }
            if !known {
                return Err(blocked(
                    "later move copy mapping is not in acknowledged plan history",
                ));
            }
            let pending = PendingEffect {
                id: effect.effect_id,
                generation,
            };
            let tree = if let Some(object) = plan.object {
                let slot = plan
                    .retained
                    .ok_or_else(|| blocked("later retained selector missing"))?;
                let private_name = ComponentName::new(OsStr::new(&slot.name))?;
                let private = OriginSlot {
                    parent: quarantine.directory().clone(),
                    name: private_name.clone(),
                };
                let root = find_expected_root(&[private, origin.clone()], &object)?;
                let tree = HeldTree::admit(HeldRoot::Present(root.clone()), &mut budget, None)?;
                if complete && !occupies(&root, quarantine.directory(), &private_name)? {
                    return Err(blocked("applied later preservation object is not retained"));
                }
                moved.insert(
                    kind,
                    ContextMove {
                        pending,
                        prior,
                        private_name,
                        object,
                        complete,
                    },
                );
                tree
            } else {
                if !prior.entries.is_empty() {
                    return Err(blocked("later absence manifest has objects"));
                }
                if complete {
                    absent_preserved.insert(kind);
                } else {
                    absent_attempts.insert(kind, pending);
                }
                if restoring && complete {
                    // This is historical later absence. The original root may
                    // now correctly occupy its former location.
                    HeldTree {
                        root: origin.absent(),
                        manifest: prior,
                        entries: BTreeMap::new(),
                        limits: SnapshotLimits::default(),
                        detached_image: None,
                        fenced_location: None,
                        flush_required: false,
                        durably_flushed: false,
                    }
                } else {
                    HeldTree::admit(origin.absent(), &mut budget, None)?
                }
            };
            trees.insert(kind, tree);
        }
        let complete_copies = copies
            .iter()
            .map(|(root, copy)| {
                Ok((
                    *root,
                    copy.plan_generation
                        .ok_or_else(|| blocked("reopened copy generation missing"))?,
                ))
            })
            .collect::<io::Result<_>>()?;
        let value = Self {
            binding: journal.binding.clone(),
            context: HeldContext { trees },
            origins: originals.origins.clone(),
            copies,
            complete_copies,
            moved,
            quarantine,
            absent_preserved,
            absent_attempts,
            previous_copies,
        };
        value.verify_recorded_history(originals, user, journal.store)?;
        if restoring {
            value.verify_retained(originals, user)?;
        } else {
            value.verify_actual(originals, user)?;
        }
        journal.verify()?;
        Ok(value)
    }
}
impl ContextRestoration {
    /// Consumes fresh complete observations, retaining exact prior intents.
    /// A lost rename acknowledgement is observed before any new mutation.
    pub(crate) fn reopen_observation(
        originals: RetainedContextRoots,
        later: LaterContextRoots,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        journal.verify()?;
        journal.exclusive()?.verify_root(&journal.root)?;
        originals.verify(user)?;
        later.verify_retained(&originals, user)?;
        let mut value = Self::new(originals, later)?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            let latest = safe(
                journal
                    .store
                    .latest_context_root_effect(&EffectKind::RestoreSourceRoot { root: kind }),
            )?;
            let Some((effect, generation, complete)) = latest else {
                continue;
            };
            let plan: ContextLocationPlan =
                serde_json::from_slice(&safe(journal.store.read_manifest(&effect.before))?)?;
            let original = &value.originals.copies[&kind];
            let quarantine = original
                .rotation
                .as_ref()
                .map(|ticket| &ticket.quarantine)
                .unwrap_or(&value.later.quarantine);
            validate_location_plan(
                &plan,
                kind,
                &journal.binding.source_context,
                &value.originals.origins[&kind],
                &original.parent,
                quarantine,
            )?;
            let before: TreeManifest =
                serde_json::from_slice(&safe(journal.store.read_manifest(&plan.source))?)?;
            let saved: PrivateCopyManifest =
                serde_json::from_slice(&safe(journal.store.read_manifest(&plan.copy.manifest))?)?;
            if saved != *original.manifest()?
                || before.entries != saved.source.entries
                || plan.copy.name != text(&original.name)?
            {
                return Err(blocked("source restoration mapping differs"));
            }
            let prior = plan
                .retained
                .map(|slot| -> io::Result<OriginSlot> {
                    Ok(OriginSlot {
                        parent: quarantine.directory().clone(),
                        name: ComponentName::new(OsStr::new(&slot.name))?,
                    })
                })
                .transpose()?;
            value.moves.insert(
                kind,
                SourceRestoreMove {
                    pending: PendingEffect {
                        id: effect.effect_id,
                        generation,
                    },
                    prior,
                    object: plan.object,
                },
            );
            if complete {
                value.verify_original_at_origin(kind, user)?;
                value.completed.insert(kind);
            }
        }
        journal.verify()?;
        Ok(value)
    }
}

/// Revalidate the original acknowledged bytes/IDs/ACLs as well as any later
/// complete observation. Unacknowledged objects may be observed once; a saved
/// predecessor snapshot is never replaced by an arbitrary current tree.
fn verify_later_attempt(
    record: &crate::version_history::journal::LaterBackupRecord,
    tree: &HeldTree,
    next_observation: Option<&str>,
    parent: &PrivateDirectory,
    user: &CurrentUser,
    store: &JournalStore,
) -> io::Result<()> {
    parent.verify(user)?;
    verify_private_tree(tree, user)?;
    let destination: RecoveryDestination =
        serde_json::from_slice(&safe(store.read_manifest(&record.plan.destination))?)?;
    if destination.parent != *parent.directory().identity() {
        return Err(blocked("later history parent differs"));
    }
    let name = ComponentName::new(OsStr::new(&destination.name))?;
    let exact = match &tree.root {
        HeldRoot::Present(root) => occupies(root, parent.directory(), &name)?,
        HeldRoot::Absent {
            parent: actual,
            name: actual_name,
        } => {
            actual.identity() == parent.directory().identity() && same_component(actual_name, &name)
        }
    };
    if !exact {
        return Err(blocked("later history location differs"));
    }
    let source: TreeManifest =
        serde_json::from_slice(&safe(store.read_manifest(&record.plan.source_manifest))?)?;
    if source.schema != 1 || record.plan.effects as usize != source.entries.len().max(1) {
        return Err(blocked("later history source count differs"));
    }
    let actual: BTreeMap<_, _> = tree
        .manifest
        .entries
        .iter()
        .map(|entry| (entry.metadata.path.as_str(), entry))
        .collect();
    let mut known = BTreeMap::new();
    for (index, digest) in &record.applied {
        let expected = source
            .entries
            .get(*index as usize)
            .ok_or_else(|| blocked("later receipt index differs"))?;
        let acknowledged: ManifestEntry =
            serde_json::from_slice(&safe(store.read_manifest(digest))?)?;
        if acknowledged.metadata.path != expected.metadata.path
            || acknowledged.metadata.kind != expected.metadata.kind
            || acknowledged.metadata.size != expected.metadata.size
            || acknowledged.sha256 != expected.sha256
            || acknowledged.metadata.object_identity == expected.metadata.object_identity
            || known
                .insert(acknowledged.metadata.path.clone(), acknowledged.clone())
                .is_some()
            || actual.get(acknowledged.metadata.path.as_str()).copied() != Some(&acknowledged)
        {
            return Err(blocked(
                "acknowledged later copy object, bytes, or permissions changed",
            ));
        }
    }
    if known.len() == source.entries.len()
        && (tree.manifest.entries.len() != known.len()
            || tree
                .manifest
                .entries
                .iter()
                .any(|entry| known.get(&entry.metadata.path) != Some(entry)))
    {
        return Err(blocked(
            "completed later copy namespace changed before publication",
        ));
    }
    if let Some(digest) = &record.complete {
        let complete: PrivateCopyManifest =
            serde_json::from_slice(&safe(store.read_manifest(digest))?)?;
        verify_copy_mapping(&complete)?;
        if complete.source != source || complete.copy != tree.manifest {
            return Err(blocked("completed later copy changed"));
        }
    }
    if let Some(digest) = next_observation {
        let expected: TreeManifest = serde_json::from_slice(&safe(store.read_manifest(digest))?)?;
        if expected != tree.manifest {
            return Err(blocked("saved prior later-copy observation changed"));
        }
    }
    Ok(())
}
impl LaterContextRoots {
    fn verify_copy_history(&self, user: &CurrentUser) -> io::Result<()> {
        for tree in self.previous_copies.values() {
            verify_private_tree(tree, user)?;
        }
        Ok(())
    }
    fn verify_recorded_history(
        &self,
        originals: &RetainedContextRoots,
        user: &CurrentUser,
        store: &JournalStore,
    ) -> io::Result<()> {
        self.verify_copy_history(user)?;
        for root in [RootKind::Desk, RootKind::WebView] {
            let history = safe(store.context_later_history(root))?;
            for (index, record) in history.iter().enumerate() {
                let prior = index.checked_sub(1).map(|index| history[index].generation);
                if record.plan.previous_generation != prior
                    || record.plan.previous_observation.is_some() != prior.is_some()
                {
                    return Err(blocked("later copy predecessor chain differs"));
                }
                let tree = self
                    .copies
                    .get(&root)
                    .filter(|copy| copy.plan_generation == Some(record.generation))
                    .and_then(|copy| copy.tree.as_ref())
                    .or_else(|| self.previous_copies.get(&record.generation))
                    .ok_or_else(|| blocked("later copy history guard missing"))?;
                let next = history
                    .get(index + 1)
                    .and_then(|next| next.plan.previous_observation.as_deref());
                verify_later_attempt(
                    record,
                    tree,
                    next,
                    &originals.copies[&root].parent,
                    user,
                    store,
                )?;
            }
        }
        Ok(())
    }
    fn record_complete(
        &mut self,
        root: RootKind,
        originals: &RetainedContextRoots,
        admission: ContextAdmission<'_>,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        let history = safe(journal.store.context_later_history(root))?;
        let latest = history
            .last()
            .ok_or_else(|| blocked("missing current later copy plan"))?;
        self.verify_recorded_history(originals, user, journal.store)?;
        if latest.complete.is_some() {
            self.complete_copies.insert(root, latest.generation);
            return Ok(());
        }
        copy_fault(CopyFault::BeforeLaterComplete)?;
        let evidence = LaterCompleteEvidence {
            originals,
            later: self,
            root,
            admission,
            user,
            records: &journal.root,
            lease: journal.exclusive()?,
            binding: &journal.binding,
            generation: journal.generation,
        };
        journal.generation = safe(journal.store.complete_later_backup(&evidence))?;
        journal.verify()?;
        self.verify_recorded_history(originals, user, journal.store)?;
        self.complete_copies.insert(root, latest.generation);
        Ok(())
    }
}
pub(crate) struct LaterCompleteEvidence<'a> {
    originals: &'a RetainedContextRoots,
    later: &'a LaterContextRoots,
    root: RootKind,
    admission: ContextAdmission<'a>,
    user: &'a CurrentUser,
    records: &'a PrivateDirectory,
    lease: &'a ExclusiveLease,
    binding: &'a JournalBinding,
    generation: u64,
}
pub(crate) struct LaterCompleteRequest {
    pub(crate) root: RootKind,
    pub(crate) generation: u64,
    pub(crate) plan_generation: u64,
    pub(crate) copy: Vec<u8>,
}
impl LaterCompleteEvidence<'_> {
    pub(crate) fn verify(&self, store: &mut JournalStore) -> io::Result<LaterCompleteRequest> {
        self.admission.verify_store(store)?;
        self.lease.verify_root(self.records)?;
        safe(store.verify_windows_binding(self.records, self.binding, self.generation))?;
        self.later.verify_actual(self.originals, self.user)?;
        self.later
            .verify_recorded_history(self.originals, self.user, store)?;
        if self.admission.binding() != self.binding || &self.later.binding != self.binding {
            return Err(blocked("later completion binding differs"));
        }
        let history = safe(store.context_later_history(self.root))?;
        let latest = history
            .last()
            .ok_or_else(|| blocked("later completion plan missing"))?;
        let copy = self
            .later
            .copies
            .get(&self.root)
            .ok_or_else(|| blocked("later complete copy missing"))?;
        copy.verify(self.user)?;
        if latest.complete.is_some()
            || copy.plan_generation != Some(latest.generation)
            || copy.manifest()?.source != self.later.context.tree(self.root).manifest
            || copy.manifest()?.source.digest()? != latest.plan.source_manifest
            || latest.applied.len() != copy.manifest()?.source.entries.len()
        {
            return Err(blocked(
                "later completion does not match all acknowledged entries",
            ));
        }
        Ok(LaterCompleteRequest {
            root: self.root,
            generation: self.generation,
            plan_generation: latest.generation,
            copy: encoded(copy.manifest()?)?,
        })
    }
}

/// Every predecessor is opened from the retained private parent. The latest
/// complete mapping is selected independently from any older namespace intent.
struct LaterCopyGuards {
    copies: BTreeMap<RootKind, PrivateTreeCopy>,
    previous: BTreeMap<u64, HeldTree>,
}
fn reopen_later_history(
    originals: &RetainedContextRoots,
    user: &CurrentUser,
    store: &JournalStore,
) -> io::Result<LaterCopyGuards> {
    let mut copies = BTreeMap::new();
    let mut previous = BTreeMap::new();
    for root in [RootKind::Desk, RootKind::WebView] {
        let parent = &originals.copies[&root].parent;
        parent.verify(user)?;
        let history = safe(store.context_later_history(root))?;
        for (index, record) in history.iter().enumerate() {
            let expected_prior = index.checked_sub(1).map(|index| history[index].generation);
            if record.plan.previous_generation != expected_prior
                || record.plan.previous_observation.is_some() != expected_prior.is_some()
            {
                return Err(blocked("later history is incomplete"));
            }
            let destination: RecoveryDestination =
                serde_json::from_slice(&safe(store.read_manifest(&record.plan.destination))?)?;
            if destination.parent != *parent.directory().identity() {
                return Err(blocked("later copy parent is outside retained authority"));
            }
            let name = ComponentName::new(OsStr::new(&destination.name))?;
            let next = history
                .get(index + 1)
                .and_then(|record| record.plan.previous_observation.as_deref());
            if let Some(completion) = record
                .complete
                .as_ref()
                .filter(|_| index + 1 == history.len())
            {
                let complete: PrivateCopyManifest =
                    serde_json::from_slice(&safe(store.read_manifest(completion))?)?;
                let mut copy = PrivateTreeCopy::reopen(
                    parent.clone(),
                    name,
                    complete,
                    user,
                    SnapshotLimits::default(),
                )?;
                copy.plan_generation = Some(record.generation);
                verify_later_attempt(
                    record,
                    copy.tree.as_ref().expect("reopened copy tree"),
                    next,
                    parent,
                    user,
                    store,
                )?;
                copies.insert(root, copy);
            } else {
                let held = HeldRoot::observe(parent.directory().clone(), name)?;
                let tree =
                    HeldTree::admit(held, &mut Budget::new(SnapshotLimits::default())?, None)?;
                verify_later_attempt(record, &tree, next, parent, user, store)?;
                previous.insert(record.generation, tree);
            }
        }
    }
    Ok(LaterCopyGuards { copies, previous })
}
