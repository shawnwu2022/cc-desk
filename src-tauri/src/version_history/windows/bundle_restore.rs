//! Installation return is a retained-object transaction, never a path/digest
//! copier. Original and later byte copies keep their own actual file identities;
//! restoration compares the original logical names, bytes and security contract.
//! No failure path removes a copy, moved object, partially restored file, or log.
use super::super::{
    coordinator_evidence::{CurrentImageEvidence, ReturnBoundary},
    manager_bundle::{ManagerRecord, ManagerRecordReference},
    scope::RegisteredInstallation,
    security::{restored_file_descriptor_matches, ValidatedDescriptor},
};
use super::*;
use crate::version_history::journal::{FilesystemOperation, JournalPhase, ManifestRole};
use std::{fs::File, os::windows::ffi::OsStrExt, path::Path};
use windows::Win32::{
    Security::{
        Authorization::{SetSecurityInfo, SE_FILE_OBJECT},
        GetSecurityDescriptorDacl, GetSecurityDescriptorGroup, GetSecurityDescriptorOwner,
        IsValidAcl, IsValidSid, ACL, PSID,
    },
    Storage::FileSystem::{
        FileBasicInfo, GetDiskFreeSpaceExW, SetFileInformationByHandle, DELETE, FILE_ACCESS_RIGHTS,
        FILE_ADD_SUBDIRECTORY, FILE_ALL_ACCESS, FILE_BASIC_INFO, FILE_LIST_DIRECTORY,
        FILE_SHARE_WRITE, FILE_TRAVERSE, FILE_WRITE_ATTRIBUTES, WRITE_DAC, WRITE_OWNER,
    },
};
use windows_core::PCWSTR;

const SOURCE_RECORD: &str = "source-installation.json";
const SOURCE_COPY: &str = "source-installation-copy";
const RETURN_PLAN: &str = "installation-return-plan.json";
const LATER_COPY: &str = "later-installation-copy";
const LATER_OBJECTS: &str = "later-installation-objects";
const RETURN_RESULT: &str = "installation-return-result.json";

fn component(value: &str) -> io::Result<ComponentName> {
    ComponentName::new(OsStr::new(value))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallationSlot {
    /// Exact registered drive spelling, protected by the record and checked
    /// against both parent and directory file IDs on every reopen.
    registered_path: String,
    parent: FileIdentity,
    name: String,
    directory: FileIdentity,
}
impl InstallationSlot {
    fn capture(directory: &Arc<Directory>, registered_path: &Path) -> io::Result<Self> {
        directory.recheck()?;
        let (parent, name) = directory.held_location()?;
        let result = Self {
            registered_path: registered_path
                .to_str()
                .ok_or_else(|| blocked("unrepresentable registered installation"))?
                .into(),
            parent: parent.identity().clone(),
            name: text(&name)?,
            directory: directory.identity().clone(),
        };
        result.verify(directory)?;
        Ok(result)
    }
    fn verify(&self, directory: &Arc<Directory>) -> io::Result<()> {
        directory.recheck()?;
        let (parent, name) = directory.held_location()?;
        if self.directory != *directory.identity()
            || self.parent != *parent.identity()
            || self.name != text(&name)?
        {
            return Err(blocked("registered installation slot changed"));
        }
        Ok(())
    }
    fn reopen(&self) -> io::Result<Arc<Directory>> {
        let directory = Directory::open_absolute(Path::new(&self.registered_path))?;
        self.verify(&directory)?;
        Ok(directory)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceInstallationRecord {
    schema: u32,
    transaction: String,
    data_root: FileIdentity,
    slot: InstallationSlot,
    source: InstalledBundleManifest,
    copy: PrivateCopyManifest,
}

/// Authentication comes from the live registered installation at capture and
/// the exact protected private record at reopen. It is not Deserialize-able.
/// Read-only original backup observation. This owns exact protected record and
/// private copy contents, without any current installed-directory, source-exit,
/// Return, process or launch capability. Ordinary installers may replace the
/// live directory while these independent source backups remain intact.
pub(crate) struct ObservedInstallationBackup {
    data: Arc<PrivateDirectory>,
    copy: PrivateTreeCopy,
    record: ManagerRecord,
    saved: SourceInstallationRecord,
    binding: JournalBinding,
}
impl ObservedInstallationBackup {
    pub(crate) fn reopen(
        data: Arc<PrivateDirectory>,
        expected: &ManagerRecordReference,
        user: &CurrentUser,
        binding: &JournalBinding,
    ) -> io::Result<Self> {
        let record = ManagerRecord::open(data.clone(), SOURCE_RECORD, expected, user)?;
        let saved: SourceInstallationRecord = record.decode(user)?;
        if saved.schema != 1
            || saved.transaction != binding.transaction_id
            || saved.data_root != *data.directory().identity()
            || saved.source.logical_digest()? != binding.source_bundle
        {
            return Err(blocked("original backup belongs to another transaction"));
        }
        let copy = PrivateTreeCopy::reopen(
            data.clone(),
            component(SOURCE_COPY)?,
            saved.copy.clone(),
            user,
            SnapshotLimits::default(),
        )?;
        let observation = Self {
            data,
            copy,
            record,
            saved,
            binding: binding.clone(),
        };
        observation.verify(user)?;
        Ok(observation)
    }
    pub(crate) fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        self.data.verify(user)?;
        self.record.verify(user)?;
        self.copy.verify(user)?;
        if self.saved.schema != 1
            || self.saved.transaction != self.binding.transaction_id
            || self.saved.data_root != *self.data.directory().identity()
            || self.saved.source.logical_digest()? != self.binding.source_bundle
            || self.saved.copy != *self.copy.manifest()?
            || self.saved.copy.source != self.saved.source.tree
        {
            return Err(blocked("original backup mapping changed"));
        }
        validate_restorable(&self.saved.source.tree, user)
    }
}

pub(crate) struct RetainedInstallationBundle {
    data: Arc<PrivateDirectory>,
    directory: Arc<Directory>,
    copy: PrivateTreeCopy,
    record: ManagerRecord,
    saved: SourceInstallationRecord,
}
impl RetainedInstallationBundle {
    pub(crate) fn preserve(
        installation: &RegisteredInstallation,
        source: &HeldBundle,
        data: Arc<PrivateDirectory>,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        installation
            .recheck()
            .map_err(|_| blocked("registered source changed"))?;
        let result = Self::preserve_observed(
            installation.directory().clone(),
            installation.original_path(),
            source,
            data,
            user,
            journal,
        )?;
        installation
            .recheck()
            .map_err(|_| blocked("registered source changed"))?;
        Ok(result)
    }
    fn preserve_observed(
        directory: Arc<Directory>,
        registered_path: &Path,
        source: &HeldBundle,
        data: Arc<PrivateDirectory>,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        journal.verify()?;
        require_phase(journal, &[JournalPhase::Reviewed])?;
        source.tree.verify()?;
        verify_bundle_confidential(&source.tree, user)?;
        data.verify(user)?;
        directory.require_disjoint(&[data.directory().clone()])?;
        if source.manifest.logical_digest()? != journal.binding.source_bundle
            || !matches!(&source.tree.root, HeldRoot::Present(root) if root.identity() == directory.identity())
        {
            return Err(blocked("source copy is outside admitted installation"));
        }
        validate_restorable(&source.manifest.tree, user)?;
        let slot = InstallationSlot::capture(&directory, registered_path)?;
        let rights = open_root_permissions(&directory)?;
        drop(rights);
        // This is in addition to the manager's independent runtime copy. Check
        // each actual volume before allocating this complete original copy.
        require_space(data.directory(), bytes_in(&source.manifest.tree)?)?;
        let mut copy = PrivateTreeCopy::new(data.clone(), component(SOURCE_COPY)?);
        copy.copy_from(&source.tree, user, journal)?;
        let copy = seal_copy(copy, user)?;
        let saved = SourceInstallationRecord {
            schema: 1,
            transaction: journal.binding.transaction_id.clone(),
            data_root: data.directory().identity().clone(),
            slot,
            source: source.manifest.clone(),
            copy: copy.manifest()?.clone(),
        };
        let record = ManagerRecord::create(data.clone(), SOURCE_RECORD, &saved, user)?;
        let result = Self {
            data,
            directory,
            copy,
            record,
            saved,
        };
        source.tree.verify()?;
        result.verify(user)?;
        journal.verify()?;
        Ok(result)
    }
    pub(crate) fn reopen(
        data: Arc<PrivateDirectory>,
        expected: &ManagerRecordReference,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        journal.verify()?;
        let record = ManagerRecord::open(data.clone(), SOURCE_RECORD, expected, user)?;
        let saved: SourceInstallationRecord = record.decode(user)?;
        if saved.schema != 1
            || saved.transaction != journal.binding.transaction_id
            || saved.data_root != *data.directory().identity()
            || saved.source.logical_digest()? != journal.binding.source_bundle
        {
            return Err(blocked(
                "original bundle record belongs to another installation",
            ));
        }
        let directory = saved.slot.reopen()?;
        let copy = PrivateTreeCopy::reopen(
            data.clone(),
            component(SOURCE_COPY)?,
            saved.copy.clone(),
            user,
            SnapshotLimits::default(),
        )?;
        let result = Self {
            data,
            directory,
            copy,
            record,
            saved,
        };
        result.verify(user)?;
        Ok(result)
    }
    pub(crate) fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        self.data.verify(user)?;
        self.record.verify(user)?;
        self.saved.slot.verify(&self.directory)?;
        self.copy.verify(user)?;
        if self.saved.schema != 1
            || self.saved.data_root != *self.data.directory().identity()
            || self.saved.copy != *self.copy.manifest()?
            || self.saved.copy.source != self.saved.source.tree
        {
            return Err(blocked("original installation mapping changed"));
        }
        validate_restorable(&self.saved.source.tree, user)
    }
    pub(crate) fn reference(&self) -> &ManagerRecordReference {
        self.record.reference()
    }
    pub(crate) fn source_manifest(&self) -> &InstalledBundleManifest {
        &self.saved.source
    }
    pub(crate) fn directory(&self) -> &Arc<Directory> {
        &self.directory
    }

    /// This aggregate is recorded only after the actual quiescent, fenced
    /// original inventory is checked against the private copy's ORIGINAL IDs.
    pub(crate) fn record_preserved(
        &self,
        current: &HeldBundle,
        boundary: &SnapshotBoundary,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        safe(boundary.verify_live())?;
        self.verify(user)?;
        current.tree.verify()?;
        self.verify_journal(journal)?;
        require_phase(journal, &[JournalPhase::Reviewed])?;
        if boundary.binding() != &journal.binding
            || current.manifest.tree.entries != self.saved.source.tree.entries
            || current.manifest.tree.location_identity != self.saved.source.tree.location_identity
            || current.manifest.logical_digest()? != journal.binding.source_bundle
        {
            return Err(blocked("fenced original differs from retained source"));
        }
        let manifest = journal.retain(&self.saved.source)?;
        require_source_role(journal, &manifest)?;
        let pending = journal.begin(
            EffectKind::VerifySourceBundleCopy,
            &(&self.saved.slot, &manifest),
            &(self.reference(), &self.saved.copy),
        )?;
        self.verify(user)?;
        current.tree.verify()?;
        safe(boundary.verify_live())?;
        journal.applied(pending, &(self.reference(), &self.saved.copy))
    }
    fn verify_journal(&self, journal: &mut ContextJournal<'_>) -> io::Result<()> {
        journal.verify_transaction(&self.saved.transaction)?;
        if journal.binding.source_bundle != self.saved.source.logical_digest()? {
            return Err(blocked("original bundle differs from transaction"));
        }
        Ok(())
    }
}

fn seal_copy(mut copy: PrivateTreeCopy, user: &CurrentUser) -> io::Result<PrivateTreeCopy> {
    let expected = copy.manifest()?.clone();
    copy.verify(user)?;
    drop(copy.tree.take());
    PrivateTreeCopy::reopen(
        copy.parent.clone(),
        copy.name.clone(),
        expected,
        user,
        SnapshotLimits::default(),
    )
}

fn verify_bundle_confidential(tree: &HeldTree, user: &CurrentUser) -> io::Result<()> {
    tree.verify()?;
    for entry in tree.entries.values() {
        match entry {
            HeldEntry::Directory(root) => user.verify_confidential_source(root.raw(), true)?,
            HeldEntry::File(FileGuard::Ordinary(file)) => {
                user.verify_confidential_source(handle(&file.lock().file), false)?
            }
            HeldEntry::File(FileGuard::Fenced(fence)) => fence.lock().verify_confidential(user)?,
        }
    }
    Ok(())
}
fn validate_restorable(tree: &TreeManifest, user: &CurrentUser) -> io::Result<()> {
    for entry in &tree.entries {
        let PermissionRecord::Windows { descriptor, .. } = &entry.metadata.permissions else {
            return Err(blocked("installation has unsupported permissions"));
        };
        ValidatedDescriptor::from_bytes(descriptor)?.require_assignable(user)?;
    }
    if tree.entries.first().is_none_or(|entry| {
        !entry.metadata.path.is_empty() || entry.metadata.kind != EntryType::Directory
    }) {
        return Err(blocked("installation root is not present"));
    }
    Ok(())
}
fn open_root_permissions(root: &Arc<Directory>) -> io::Result<File> {
    let (parent, name) = root.held_location()?;
    let file = parent.open_relative(
        &name,
        FILE_LIST_DIRECTORY
            | FILE_TRAVERSE
            | FILE_READ_ATTRIBUTES
            | FILE_WRITE_DATA
            | FILE_ADD_SUBDIRECTORY
            | FILE_WRITE_ATTRIBUTES
            | READ_CONTROL
            | WRITE_DAC
            | WRITE_OWNER
            | SYNCHRONIZE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        FILE_OPEN,
        true,
        None,
    )?;
    if super::super::files::metadata(handle(&file))?.identity != *root.identity() {
        return Err(blocked("installation permission handle changed"));
    }
    Ok(file)
}
fn bytes_in(tree: &TreeManifest) -> io::Result<u64> {
    tree.entries.iter().try_fold(0u64, |sum, entry| {
        sum.checked_add(entry.metadata.size)
            .ok_or_else(|| blocked("installation copy size overflow"))
    })
}
fn require_space(root: &Directory, bytes: u64) -> io::Result<()> {
    root.recheck()?;
    let mut path: Vec<u16> = root.path()?.encode_wide().collect();
    path.push(0);
    let mut available = 0;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(path.as_ptr()), Some(&mut available), None, None) }
        .map_err(win_error)?;
    // Space is a preflight, not a promise. Every copy also checks writes/flushes.
    if available
        < bytes
            .checked_add(64 * 1024 * 1024)
            .ok_or_else(|| blocked("copy capacity overflow"))?
    {
        return Err(blocked(
            "insufficient space for complete retained installation",
        ));
    }
    root.recheck()
}
fn require_phase(journal: &mut ContextJournal<'_>, phases: &[JournalPhase]) -> io::Result<()> {
    journal.verify()?;
    let inspected = safe(journal.store.inspect(&journal.binding))?;
    let state = inspected
        .last_valid
        .ok_or_else(|| blocked("installation journal missing"))?;
    if inspected.blocked || !phases.contains(&state.phase()) || state.requires_reconciliation() {
        return Err(blocked("installation transaction requires reconciliation"));
    }
    Ok(())
}
fn require_source_role(journal: &mut ContextJournal<'_>, expected: &str) -> io::Result<()> {
    let inspected = safe(journal.store.inspect(&journal.binding))?;
    if inspected.blocked
        || inspected
            .last_valid
            .as_ref()
            .and_then(|state| state.manifest(ManifestRole::SourceBundle))
            != Some(expected)
    {
        return Err(blocked("complete source bundle role differs"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleSeed {
    schema: u32,
    transaction: String,
    data_root: FileIdentity,
    original: ManagerRecordReference,
    slot: InstallationSlot,
    current: TreeManifest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleSeedReference {
    generation: u64,
    digest: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleEffectBefore {
    schema: u32,
    attempt: AttemptNames,
    plan: ManagerRecordReference,
    state: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct AttemptNames {
    id: Option<String>,
}
impl AttemptNames {
    fn fresh() -> Self {
        Self {
            id: Some(uuid::Uuid::new_v4().to_string()),
        }
    }
    fn validate(&self) -> io::Result<()> {
        if let Some(id) = &self.id {
            if uuid::Uuid::parse_str(id)
                .map_err(|_| blocked("invalid bundle attempt"))?
                .to_string()
                != *id
            {
                return Err(blocked("noncanonical bundle attempt"));
            }
        }
        Ok(())
    }
    fn name(&self, base: &str) -> io::Result<String> {
        self.validate()?;
        Ok(match &self.id {
            Some(id) => format!("{base}-{id}"),
            None => base.into(),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistorySnapshot {
    attempt: AttemptNames,
    copy: TreeManifest,
    quarantine: TreeManifest,
}
struct HistoryTrees {
    attempt: AttemptNames,
    copy: HeldTree,
    quarantine: HeldTree,
}
#[derive(Default)]
struct BundleHistory {
    trees: Vec<HistoryTrees>,
}
impl BundleHistory {
    fn observe(
        data: &Arc<PrivateDirectory>,
        attempts: &[AttemptNames],
        user: &CurrentUser,
    ) -> io::Result<Self> {
        data.verify(user)?;
        if attempts.len() > 64 {
            return Err(blocked("bundle attempt limit exceeded"));
        }
        let mut distinct = BTreeSet::new();
        let mut trees = Vec::new();
        for attempt in attempts {
            attempt.validate()?;
            if !distinct.insert(attempt.id.clone()) {
                return Err(blocked("duplicate bundle attempt"));
            }
            let observe = |base| -> io::Result<HeldTree> {
                let root =
                    HeldRoot::observe(data.directory().clone(), component(&attempt.name(base)?)?)?;
                if let HeldRoot::Present(root) = &root {
                    user.verify_private_file(root.raw(), true)?;
                }
                let tree =
                    HeldTree::admit(root, &mut Budget::new(SnapshotLimits::default())?, None)?;
                verify_bundle_confidential(&tree, user)?;
                Ok(tree)
            };
            trees.push(HistoryTrees {
                attempt: attempt.clone(),
                copy: observe(LATER_COPY)?,
                quarantine: observe(LATER_OBJECTS)?,
            });
        }
        let result = Self { trees };
        result.verify()?;
        Ok(result)
    }
    /// A lost guard never turns previously retained data into a new baseline.
    /// Older attempts were frozen by the latest recovery admission. The newest
    /// copy is anchored by its individual Applied receipts, including the gap
    /// before a complete ready-plan record can be published.
    fn verify_retained_evidence(
        &self,
        seed: &BundleSeed,
        seed_reference: &BundleSeedReference,
        active: Option<&(u64, crate::version_history::journal::BundleBackupPlan)>,
        store: &JournalStore,
    ) -> io::Result<()> {
        self.verify()?;
        let newest = self
            .trees
            .last()
            .ok_or_else(|| blocked("bundle history missing"))?;
        let (generation, source_digest, source) = if let Some((generation, plan)) = active {
            let (saved_seed, expected): (BundleSeedReference, Vec<HistorySnapshot>) =
                serde_json::from_slice(&safe(store.read_manifest(&plan.previous_observation))?)?;
            let actual = self.snapshots();
            if saved_seed != *seed_reference
                || expected.len() + 1 != actual.len()
                || actual[..expected.len()] != expected
            {
                return Err(blocked("previously retained bundle history changed"));
            }
            let source: TreeManifest =
                serde_json::from_slice(&safe(store.read_manifest(&plan.current_manifest))?)?;
            (*generation, plan.current_manifest.clone(), source)
        } else {
            if self.trees.len() != 1 || newest.attempt != AttemptNames::default() {
                return Err(blocked("initial bundle history selector differs"));
            }
            (
                seed_reference.generation,
                seed.current.digest()?,
                seed.current.clone(),
            )
        };
        let receipts = safe(store.bundle_copy_receipts(generation, &source_digest))?;
        let mut copied = BTreeSet::new();
        let actual: BTreeMap<_, _> = newest
            .copy
            .manifest
            .entries
            .iter()
            .map(|entry| (entry.metadata.path.as_str(), entry))
            .collect();
        for (index, digest) in &receipts {
            let original = source
                .entries
                .get(*index as usize)
                .ok_or_else(|| blocked("retained copy receipt index differs"))?;
            let saved: ManifestEntry = serde_json::from_slice(&safe(store.read_manifest(digest))?)?;
            if !copied.insert(*index)
                || saved.metadata.path != original.metadata.path
                || saved.metadata.kind != original.metadata.kind
                || saved.metadata.size != original.metadata.size
                || saved.sha256 != original.sha256
                || actual.get(saved.metadata.path.as_str()).copied() != Some(&saved)
            {
                return Err(blocked("previously copied bundle object changed"));
            }
        }
        if receipts.len() == source.entries.len() {
            // Complete copy receipts prohibit disappeared or additional entries;
            // only an unresolved partial copy can admit freshly observed data.
            verify_copy_mapping(&PrivateCopyManifest {
                schema: 1,
                source,
                copy: newest.copy.manifest.clone(),
            })?;
        }
        Ok(())
    }
    fn verify(&self) -> io::Result<()> {
        for tree in &self.trees {
            tree.copy.verify()?;
            tree.quarantine.verify()?;
        }
        Ok(())
    }
    fn snapshots(&self) -> Vec<HistorySnapshot> {
        self.trees
            .iter()
            .map(|tree| HistorySnapshot {
                attempt: tree.attempt.clone(),
                copy: tree.copy.manifest.clone(),
                quarantine: tree.quarantine.manifest.clone(),
            })
            .collect()
    }
    fn attempts(&self) -> Vec<AttemptNames> {
        self.trees.iter().map(|tree| tree.attempt.clone()).collect()
    }
    fn reopen(
        data: &Arc<PrivateDirectory>,
        expected: &[HistorySnapshot],
        user: &CurrentUser,
    ) -> io::Result<Self> {
        let actual = Self::observe(
            data,
            &expected
                .iter()
                .map(|saved| saved.attempt.clone())
                .collect::<Vec<_>>(),
            user,
        )?;
        if actual.snapshots() != expected {
            return Err(blocked("prior bundle attempt data changed"));
        }
        Ok(actual)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BundleReturnReference {
    attempt: AttemptNames,
    record: ManagerRecordReference,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReturnPlan {
    schema: u32,
    #[serde(default)]
    attempt: AttemptNames,
    #[serde(default)]
    history: Vec<HistorySnapshot>,
    transaction: String,
    source: ManagerRecordReference,
    slot: InstallationSlot,
    current: TreeManifest,
    later_copy: PrivateCopyManifest,
    /// The exclusive current image can already be at a protected quarantine
    /// selector. The logical original filename remains in the complete copy.
    detached_image: Option<String>,
    image_identity: Option<FileIdentity>,
}

enum MovedObject {
    File(PinnedFile),
    Directory(Arc<Directory>, Option<Box<HeldTree>>),
    Fenced(Arc<Mutex<ImageFence>>),
}
impl MovedObject {
    fn verify(&self) -> io::Result<()> {
        match self {
            Self::File(file) => file.verify(),
            Self::Directory(directory, tree) => {
                directory.recheck()?;
                if let Some(tree) = tree {
                    tree.verify()?;
                }
                Ok(())
            }
            Self::Fenced(fence) => fence.lock().verify(),
        }
    }
}

/// Owns every preparation observation and partial private copy before the
/// first durable bundle-start intent. Failure consumes this attempt but leaves
/// its native custody with the coordinator; no missing handle implies rollback.
pub(crate) struct BundlePreparationAttempt {
    original: Arc<RetainedInstallationBundle>,
    boundary: Arc<ReturnBoundary>,
    current: Option<HeldTree>,
    root_write: Option<File>,
    later: Option<PrivateTreeCopy>,
    sealing_root: Option<HeldRoot>,
    prepared: Option<BundleRestoration>,
    attempted: bool,
}
impl BundlePreparationAttempt {
    pub(crate) fn new(
        original: Arc<RetainedInstallationBundle>,
        boundary: Arc<ReturnBoundary>,
    ) -> Self {
        Self {
            original,
            boundary,
            current: None,
            root_write: None,
            later: None,
            sealing_root: None,
            prepared: None,
            attempted: false,
        }
    }
    pub(crate) fn prepare(
        &mut self,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<BundleRestoration> {
        if self.attempted {
            return Err(blocked("bundle preparation requires reconciliation"));
        }
        self.attempted = true;
        let original = &self.original;
        let boundary = &self.boundary;
        original.verify(user)?;
        original.verify_journal(journal)?;
        safe(boundary.verify_live())?;
        safe(boundary.verify_current_image())?;
        journal.exclusive()?.verify_root(&journal.root)?;
        require_phase(
            journal,
            &[
                JournalPhase::RecoveryRequired,
                JournalPhase::HistoricalActive,
                JournalPhase::InstalledUnconfirmed,
            ],
        )?;
        if boundary.binding() != &journal.binding
            || boundary.installation().identity() != original.directory.identity()
            || text(boundary.image_name())? != original.saved.source.original_image_name
        {
            return Err(blocked(
                "return evidence differs from registered installation",
            ));
        }
        original
            .directory
            .require_same_volume(original.data.directory())?;
        let fenced = match boundary.current_image() {
            CurrentImageEvidence::Fenced(fence) => {
                Some((boundary.image_name().clone(), fence.clone()))
            }
            CurrentImageEvidence::Absent(absence) => {
                absence.verify()?;
                None
            }
        };
        self.current = Some(HeldTree::admit(
            HeldRoot::Present(original.directory.clone()),
            &mut Budget::new(SnapshotLimits::default())?,
            fenced,
        )?);
        let current = self.current.as_ref().expect("retained current bundle");
        if matches!(boundary.current_image(), CurrentImageEvidence::Absent(_))
            && current.manifest.entries.iter().any(|entry| {
                entry
                    .metadata
                    .path
                    .eq_ignore_ascii_case(&original.saved.source.original_image_name)
            })
        {
            return Err(blocked("current executable absence changed"));
        }
        verify_bundle_confidential(current, user)?;
        require_space(original.data.directory(), bytes_in(&current.manifest)?)?;
        require_space(&original.directory, bytes_in(&original.saved.source.tree)?)?;
        self.root_write = Some(open_root_permissions(&original.directory)?);
        let seed = BundleSeed {
            schema: 1,
            transaction: journal.binding.transaction_id.clone(),
            data_root: original.data.directory().identity().clone(),
            original: original.reference().clone(),
            slot: original.saved.slot.clone(),
            current: current.manifest.clone(),
        };
        let evidence = BundleStartEvidence {
            original,
            boundary,
            current,
            seed: &seed,
            user,
            root: &journal.root,
            lease: journal.exclusive()?,
            generation: journal.generation,
        };
        let (generation, digest) = safe(journal.store.admit_bundle_start(&evidence))?;
        journal.generation = generation;
        self.later = Some(PrivateTreeCopy::new(
            original.data.clone(),
            component(LATER_COPY)?,
        ));
        let later = self.later.as_mut().expect("retained partial bundle copy");
        later.copy_from_plan(current, user, journal, Some((generation, digest)))?;
        let expected = later.manifest()?.clone();
        later.verify(user)?;
        // Writable descendants must be closed for read-only admission, but the
        // actual private root and incomplete copy owner remain retained.
        self.sealing_root = Some(
            later
                .tree
                .as_ref()
                .ok_or_else(|| blocked("missing copy tree"))?
                .root
                .clone(),
        );
        drop(later.tree.take());
        let sealed = PrivateTreeCopy::reopen(
            later.parent.clone(),
            later.name.clone(),
            expected,
            user,
            SnapshotLimits::default(),
        )?;
        *later = sealed;
        self.sealing_root = None;
        current.verify()?;
        safe(boundary.verify_current_image())?;
        let plan = ReturnPlan {
            schema: 1,
            attempt: AttemptNames::default(),
            history: Vec::new(),
            transaction: journal.binding.transaction_id.clone(),
            source: original.reference().clone(),
            slot: original.saved.slot.clone(),
            current: current.manifest.clone(),
            later_copy: later.manifest()?.clone(),
            detached_image: current.detached_image.clone(),
            image_identity: match boundary.current_image() {
                CurrentImageEvidence::Fenced(fence) => Some(fence.lock().identity().clone()),
                CurrentImageEvidence::Absent(_) => None,
            },
        };
        let plan_record = ManagerRecord::create(original.data.clone(), RETURN_PLAN, &plan, user)?;
        let mut writes = BTreeMap::new();
        writes.insert(
            String::new(),
            self.root_write.take().expect("retained root permissions"),
        );
        self.prepared = Some(BundleRestoration {
            directories: BTreeMap::from([(String::new(), original.directory.clone())]),
            original: self.original.clone(),
            boundary: self.boundary.clone(),
            later: self.later.take().expect("sealed later copy"),
            current: self.current.take(),
            plan_record,
            plan,
            quarantine: None,
            moved: BTreeMap::new(),
            writes,
            attempted: false,
            history: BundleHistory::default(),
            admitted_generation: None,
            pending_readback: None,
            pending_result_record: None,
            pending_restored: None,
        });
        self.prepared
            .as_ref()
            .expect("retained prepared bundle")
            .verify_dependencies(user, journal)?;
        Ok(self.prepared.take().expect("verified prepared bundle"))
    }
}

/// One admitted return attempt. An uncertain effect permanently consumes this
/// attempt; its recorded exact selectors remain inspectable after restart.
pub(crate) struct BundleRestoration {
    original: Arc<RetainedInstallationBundle>,
    boundary: Arc<ReturnBoundary>,
    later: PrivateTreeCopy,
    current: Option<HeldTree>,
    plan_record: ManagerRecord,
    plan: ReturnPlan,
    quarantine: Option<Arc<PrivateDirectory>>,
    moved: BTreeMap<String, MovedObject>,
    writes: BTreeMap<String, File>,
    directories: BTreeMap<String, Arc<Directory>>,
    attempted: bool,
    history: BundleHistory,
    admitted_generation: Option<u64>,
    pending_readback: Option<HeldTree>,
    pending_result_record: Option<ManagerRecord>,
    pending_restored: Option<RestoredInstallationBundle>,
}
impl BundleRestoration {
    /// 封存仅接受已完整保存、且没有开始任何恢复效果的原始计划。
    pub(crate) fn verify_return_checkpoint(
        &self,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        if self.attempted
            || self.quarantine.is_some()
            || !self.moved.is_empty()
            || self.writes.len() != 1
            || !self.writes.contains_key("")
            || self.directories.len() != 1
            || !self.directories.contains_key("")
            || self.pending_readback.is_some()
            || self.pending_result_record.is_some()
            || self.pending_restored.is_some()
        {
            return Err(blocked("installation restoration has already started"));
        }
        let root = &self.directories[""];
        root.recheck()?;
        if root.identity() != self.original.directory.identity()
            || super::super::files::metadata(handle(&self.writes[""]))?.identity != *root.identity()
        {
            return Err(blocked("prepared installation root owners changed"));
        }
        self.verify_dependencies(user, journal)?;
        self.current
            .as_ref()
            .ok_or_else(|| blocked("current installation guards missing"))?
            .verify()
    }
    /// The return factory has reconciled the exact installer, historical App,
    /// browser and owned children and minted fresh lease/quiescence evidence.
    /// A source SnapshotBoundary cannot be substituted here.
    pub(crate) fn prepare(
        original: Arc<RetainedInstallationBundle>,
        boundary: Arc<ReturnBoundary>,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        BundlePreparationAttempt::new(original, boundary).prepare(user, journal)
    }
    /// Reacquire only the original, complete, sealed preparation. The caller
    /// separately verifies the real checkpoint marker and return boundary;
    /// this constructor neither claims execution nor repeats preparation.
    pub(crate) fn reopen_prepared(
        original: Arc<RetainedInstallationBundle>,
        boundary: Arc<ReturnBoundary>,
        expected: &ManagerRecordReference,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        journal.verify()?;
        safe(
            journal
                .store
                .verify_unclaimed_return_checkpoint_bundle_plan(&journal.binding, expected),
        )?;
        let reopened = Self::reopen_prepared_objects(original, boundary, expected, user, journal)?;
        safe(
            journal
                .store
                .verify_unclaimed_return_checkpoint_bundle_plan(&journal.binding, expected),
        )?;
        Ok(reopened)
    }

    fn reopen_prepared_objects(
        original: Arc<RetainedInstallationBundle>,
        boundary: Arc<ReturnBoundary>,
        expected: &ManagerRecordReference,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        original.verify(user)?;
        original.verify_journal(journal)?;
        require_phase(journal, &[JournalPhase::Restoring])?;
        journal.exclusive()?.verify_root(&journal.root)?;
        safe(boundary.verify_current_image())?;
        if boundary.binding() != &journal.binding
            || boundary.installation().identity() != original.directory.identity()
            || text(boundary.image_name())? != original.saved.source.original_image_name
            || safe(journal.store.latest_bundle_backup())?.is_some()
        {
            return Err(blocked("prepared installation boundary or attempt changed"));
        }
        let plan_record = ManagerRecord::open(original.data.clone(), RETURN_PLAN, expected, user)?;
        let plan: ReturnPlan = plan_record.decode(user)?;
        if plan.schema != 1
            || plan.attempt != AttemptNames::default()
            || !plan.history.is_empty()
            || plan.transaction != journal.binding.transaction_id
            || plan.source != *original.reference()
            || plan.slot != original.saved.slot
            || plan.current != plan.later_copy.source
        {
            return Err(blocked("original prepared installation plan changed"));
        }
        plan.slot.verify(&original.directory)?;
        original
            .directory
            .require_same_volume(original.data.directory())?;
        for name in [LATER_OBJECTS, RETURN_RESULT] {
            HeldRoot::Absent {
                parent: original.data.directory().clone(),
                name: component(name)?,
            }
            .verify()?;
        }
        let (seed_generation, seed_digest) = safe(journal.store.latest_bundle_seed())?
            .ok_or_else(|| blocked("prepared installation seed missing"))?;
        let seed: BundleSeed =
            serde_json::from_slice(&safe(journal.store.read_manifest(&seed_digest))?)?;
        if seed.schema != 1
            || seed.transaction != plan.transaction
            || seed.data_root != *original.data.directory().identity()
            || seed.original != plan.source
            || seed.slot != plan.slot
            || seed.current != plan.current
        {
            return Err(blocked("prepared installation seed differs"));
        }
        let later = PrivateTreeCopy::reopen(
            original.data.clone(),
            component(LATER_COPY)?,
            plan.later_copy.clone(),
            user,
            SnapshotLimits::default(),
        )?;
        let receipts = safe(
            journal
                .store
                .bundle_copy_receipts(seed_generation, &seed.current.digest()?),
        )?;
        let mut copied = BTreeSet::new();
        for (index, digest) in &receipts {
            let saved: ManifestEntry =
                serde_json::from_slice(&safe(journal.store.read_manifest(digest))?)?;
            let source = plan
                .current
                .entries
                .get(*index as usize)
                .ok_or_else(|| blocked("prepared installation copy index changed"))?;
            if !copied.insert(*index)
                || saved.metadata.path != source.metadata.path
                || plan.later_copy.copy.entries.get(*index as usize) != Some(&saved)
            {
                return Err(blocked("prepared installation copy receipt changed"));
            }
        }
        if receipts.len() != plan.current.entries.len() {
            return Err(blocked("prepared installation copy is incomplete"));
        }
        let (fenced, image_identity) = match boundary.current_image() {
            CurrentImageEvidence::Fenced(fence) => (
                Some((boundary.image_name().clone(), fence.clone())),
                Some(fence.lock().identity().clone()),
            ),
            CurrentImageEvidence::Absent(absence) => {
                absence.verify()?;
                (None, None)
            }
        };
        let current = HeldTree::admit(
            HeldRoot::Present(original.directory.clone()),
            &mut Budget::new(SnapshotLimits::default())?,
            fenced,
        )?;
        if current.manifest != plan.current
            || current.detached_image != plan.detached_image
            || image_identity != plan.image_identity
            || (image_identity.is_none()
                && current.manifest.entries.iter().any(|entry| {
                    entry
                        .metadata
                        .path
                        .eq_ignore_ascii_case(&original.saved.source.original_image_name)
                }))
        {
            return Err(blocked("prepared installation objects changed"));
        }
        verify_bundle_confidential(&current, user)?;
        let writes = BTreeMap::from([(String::new(), open_root_permissions(&original.directory)?)]);
        let directories = BTreeMap::from([(String::new(), original.directory.clone())]);
        let reopened = Self {
            original,
            boundary,
            later,
            current: Some(current),
            plan_record,
            plan,
            quarantine: None,
            moved: BTreeMap::new(),
            writes,
            directories,
            attempted: false,
            history: BundleHistory::default(),
            admitted_generation: None,
            pending_readback: None,
            pending_result_record: None,
            pending_restored: None,
        };
        reopened.verify_return_checkpoint(user, journal)?;
        Ok(reopened)
    }
    pub(crate) fn plan_reference(&self) -> &ManagerRecordReference {
        self.plan_record.reference()
    }
    pub(crate) fn later_manifest(&self) -> &PrivateCopyManifest {
        &self.plan.later_copy
    }

    fn verify_dependencies(
        &self,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        self.original.verify(user)?;
        self.original.verify_journal(journal)?;
        self.plan_record.verify(user)?;
        self.later.verify(user)?;
        self.history.verify()?;
        if self.history.snapshots() != self.plan.history {
            return Err(blocked("retained attempt history differs"));
        }
        let active = safe(journal.store.latest_bundle_backup())?;
        if active.as_ref().map(|(generation, _)| *generation) != self.admitted_generation {
            return Err(blocked("bundle attempt was superseded"));
        }
        safe(self.boundary.verify_live())?;
        journal.exclusive()?.verify_root(&journal.root)?;
        if self.plan.transaction != journal.binding.transaction_id
            || self.plan.source != *self.original.reference()
            || self.plan.later_copy != *self.later.manifest()?
            || self.plan.later_copy.source != self.plan.current
            || self.boundary.binding() != &journal.binding
        {
            return Err(blocked("installation return dependencies changed"));
        }
        if let Some(quarantine) = &self.quarantine {
            quarantine.verify(user)?;
        }
        for moved in self.moved.values() {
            moved.verify()?;
        }
        Ok(())
    }
    fn effect(
        &self,
        operation: FilesystemOperation,
        index: usize,
        before: &impl Serialize,
        expected: &impl Serialize,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<PendingEffect> {
        self.verify_dependencies(user, journal)?;
        require_phase(journal, &[JournalPhase::Restoring])?;
        let source = journal.retain(&self.original.saved.source)?;
        require_source_role(journal, &source)?;
        let bound_before = BundleEffectBefore {
            schema: 1,
            attempt: self.plan.attempt.clone(),
            plan: self.plan_record.reference().clone(),
            state: serde_json::to_value(before)?,
        };
        journal.begin(
            EffectKind::RecoveryFilesystemEntry {
                context: journal.binding.source_context.clone(),
                operation,
                manifest: source,
                entry_index: index as u32,
            },
            &bound_before,
            expected,
        )
    }

    /// Consumes all conflicting mutable/read guards on completion. Every
    /// unknown/partial result remains retained and the attempt cannot be run
    /// again, including after reopening its protected return plan.
    pub(crate) fn restore(
        &mut self,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<RestoredInstallationBundle> {
        if self.attempted {
            return Err(blocked("installation return requires reconciliation"));
        }
        self.verify_dependencies(user, journal)?;
        require_phase(journal, &[JournalPhase::Restoring])?;
        self.current
            .as_ref()
            .ok_or_else(|| blocked("current installation guards missing"))?
            .verify()?;
        let count = self.original.saved.source.tree.entries.len() as u64;
        let top = self
            .plan
            .current
            .entries
            .iter()
            .filter(|entry| !entry.metadata.path.is_empty() && !entry.metadata.path.contains('/'))
            .count() as u64;
        let effects = count
            .checked_mul(4)
            .and_then(|n| n.checked_add(top + 4))
            .ok_or_else(|| blocked("installation restore capacity overflow"))?;
        safe(journal.store.admit_bundle_capacity(
            journal.generation,
            effects,
            (effects as usize).saturating_mul(4).saturating_add(16),
        ))?;
        self.attempted = true;
        self.create_quarantine(user, journal)?;
        self.evacuate(user, journal)?;
        let entries = self.original.saved.source.tree.entries.clone();
        // Create all objects with private ACLs and retain their granted write
        // handles. After bytes are verified, restore parent security before
        // children so unprotected DACLs inherit from the ORIGINAL parent ACL.
        for (index, entry) in entries.iter().enumerate().skip(1) {
            self.create_original(index, entry, user, journal)?;
        }
        for (index, entry) in entries.iter().enumerate() {
            self.restore_security(index, entry, user, journal)?;
        }
        // Read-only readmission closes the writable-handle gap and compares
        // every actual name, byte, owner/group/DACL and attribute again.
        self.writes.clear();
        self.directories.clear();
        bundle_fault(BundleFault::BeforeReadmission)?;
        self.pending_readback = Some(HeldTree::admit(
            HeldRoot::Present(self.original.directory.clone()),
            &mut Budget::new(SnapshotLimits::default())?,
            None,
        )?);
        let held = self
            .pending_readback
            .as_ref()
            .expect("retained restored readback");
        verify_logical_restore(&self.original.saved.source.tree, &held.manifest)?;
        self.verify_dependencies(user, journal)?;
        let before = BundleEffectBefore {
            schema: 1,
            attempt: self.plan.attempt.clone(),
            plan: self.plan_record.reference().clone(),
            state: serde_json::to_value(self.original.reference())?,
        };
        let pending = journal.begin(
            EffectKind::VerifySourceBundleRestore,
            &before,
            &self.original.saved.source,
        )?;
        held.verify()?;
        let result = ReturnResult {
            schema: 1,
            attempt: self.plan.attempt.clone(),
            history: self.plan.history.clone(),
            transaction: journal.binding.transaction_id.clone(),
            plan: self.plan_record.reference().clone(),
            source: self.original.reference().clone(),
            restored: held.manifest.clone(),
            later: self.plan.later_copy.clone(),
            evacuated: self.evacuated_manifest()?,
            effect_id: pending.id.clone(),
            quarantine: self
                .quarantine
                .as_ref()
                .ok_or_else(|| blocked("later objects missing"))?
                .directory()
                .identity()
                .clone(),
        };
        self.pending_result_record = Some(ManagerRecord::create(
            self.original.data.clone(),
            &self.plan.attempt.name(RETURN_RESULT)?,
            &result,
            user,
        )?);
        let record = self
            .pending_result_record
            .as_ref()
            .expect("retained restored record");
        bundle_fault(BundleFault::BeforeFinalReceipt)?;
        journal.applied(pending, &(record.reference(), &result))?;
        self.verify_dependencies(user, journal)?;
        held.verify()?;
        // Finish all fallible readmission before transferring any actual
        // restored or evacuated owner out of the retained executor.
        let later = PrivateTreeCopy::reopen(
            self.original.data.clone(),
            component(&self.plan.attempt.name(LATER_COPY)?)?,
            self.plan.later_copy.clone(),
            user,
            SnapshotLimits::default(),
        )?;
        let quarantine = self
            .quarantine
            .clone()
            .ok_or_else(|| blocked("later objects missing"))?;
        let restored = RestoredInstallationBundle {
            original: self.original.clone(),
            boundary: self.boundary.clone(),
            held: self
                .pending_readback
                .take()
                .expect("retained restored readback"),
            later,
            quarantine,
            moved: std::mem::take(&mut self.moved),
            history: std::mem::take(&mut self.history),
            record: self
                .pending_result_record
                .take()
                .expect("retained restored record"),
            result,
        };
        self.pending_restored = Some(restored);
        self.pending_restored
            .as_ref()
            .expect("retained final bundle")
            .verify(user)?;
        // The durable pending aggregate forbids replay if record publication
        // or its journal receipt was interrupted.
        Ok(self.pending_restored.take().expect("verified final bundle"))
    }

    fn create_quarantine(
        &mut self,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        let absent = HeldRoot::Absent {
            parent: self.original.data.directory().clone(),
            name: component(&self.plan.attempt.name(LATER_OBJECTS)?)?,
        };
        absent.verify()?;
        let pending = self.effect(
            FilesystemOperation::CreateDirectory,
            0,
            &absent.location_identity()?,
            &("private-later-objects", &self.plan.current),
            user,
            journal,
        )?;
        let quarantine = Arc::new(PrivateDirectory::create_new(
            self.original.data.directory().clone(),
            component(&self.plan.attempt.name(LATER_OBJECTS)?)?,
            user,
        )?);
        self.quarantine = Some(quarantine.clone());
        quarantine.verify(user)?;
        journal.applied(pending, quarantine.directory().identity())
    }

    fn evacuate(&mut self, user: &CurrentUser, journal: &mut ContextJournal<'_>) -> io::Result<()> {
        let current = self
            .current
            .as_ref()
            .ok_or_else(|| blocked("later tree missing"))?;
        current.verify()?;
        let entries: Vec<_> = current
            .manifest
            .entries
            .iter()
            .filter(|entry| !entry.metadata.path.is_empty() && !entry.metadata.path.contains('/'))
            .cloned()
            .collect();
        // The exact complete later copy and protected plan predate every guard
        // release. A release does not authorize guessing any replacement name.
        for entry in &entries {
            let path = entry.metadata.path.clone();
            if self.plan.detached_image.as_ref() == Some(&path) {
                continue;
            }
            self.verify_dependencies(user, journal)?;
            let destination = self
                .quarantine
                .as_ref()
                .ok_or_else(|| blocked("quarantine missing"))?
                .directory()
                .clone();
            let name = component(&path)?;
            HeldRoot::Absent {
                parent: destination.clone(),
                name: name.clone(),
            }
            .verify()?;
            // The root-indexed namespace effect includes the complete actual
            // later subtree in before, including companions absent from C0.
            let subtree: Vec<_> = self
                .plan
                .current
                .entries
                .iter()
                .filter(|candidate| {
                    candidate.metadata.path == path
                        || candidate.metadata.path.starts_with(&format!("{path}/"))
                })
                .cloned()
                .collect();
            let pending = self.effect(
                FilesystemOperation::Rename,
                0,
                &(&self.plan.slot, &subtree, self.plan_record.reference()),
                &(destination.identity(), &path, &subtree),
                user,
                journal,
            )?;
            if self.current.is_some() {
                // The first exact rename intent is already durable. Release
                // conflicting readers and acquire EVERY actual DELETE handle
                // before any namespace mutation, then recheck all later bytes.
                self.current.take();
                self.admit_all_moves(&entries)?;
            }
            bundle_fault(BundleFault::AfterGuardRelease)?;
            self.move_actual(&path, destination, name)?;
            bundle_fault(BundleFault::AfterMove)?;
            self.verify_moved_subtree(&path, &subtree)?;
            HeldRoot::Absent {
                parent: self.original.directory.clone(),
                name: component(&path)?,
            }
            .verify()?;
            journal.applied(pending, &(&path, &subtree, "same-objects-retained"))?;
            self.verify_dependencies(user, journal)?;
        }
        // Drop only guards, never contents. The current image fence is owned by
        // the return boundary even when it was detached before this operation.
        self.current.take();
        if !self
            .original
            .directory
            .read_child_entries(SnapshotLimits::default().max_entries)?
            .is_empty()
        {
            return Err(blocked("unretained newer installation names remain"));
        }
        Ok(())
    }

    fn admit_all_moves(&mut self, entries: &[ManifestEntry]) -> io::Result<()> {
        let mut expected_names = Vec::new();
        for entry in entries {
            let path = &entry.metadata.path;
            if self.plan.detached_image.as_ref() == Some(path) {
                continue;
            }
            let name = component(path)?;
            let object = if path == &self.original.saved.source.original_image_name {
                match self.boundary.current_image() {
                    CurrentImageEvidence::Fenced(fence) => {
                        let image = fence.lock();
                        if !image.context_named_child(&self.original.directory, &name)?
                            || convert_metadata(
                                path,
                                image.context_metadata()?,
                                image.context_descriptor()?,
                            )? != entry.metadata
                        {
                            return Err(blocked("current image differs before namespace mutation"));
                        }
                        image.verify()?;
                        drop(image);
                        MovedObject::Fenced(fence.clone())
                    }
                    CurrentImageEvidence::Absent(_) => {
                        return Err(blocked("unexpected current canonical image"))
                    }
                }
            } else if entry.metadata.kind == EntryType::Directory {
                let read = self.original.directory.open_directory(name.clone())?;
                if identity(read.identity())? != entry.metadata.object_identity {
                    return Err(blocked("later directory was replaced"));
                }
                let expected = read.identity().clone();
                drop(read);
                let root = self.original.directory.open_for_rename(name, &expected)?;
                if entry_metadata(path, root.raw())? != entry.metadata {
                    return Err(blocked("later directory security changed"));
                }
                MovedObject::Directory(root, None)
            } else {
                // DELETE/read is sufficient for same-object native rename. Do
                // not demand a data writer merely to retain a readonly file.
                let raw = self.original.directory.open_relative(
                    &name,
                    FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | DELETE | SYNCHRONIZE,
                    windows::Win32::Storage::FileSystem::FILE_SHARE_MODE(0),
                    FILE_OPEN,
                    false,
                    None,
                )?;
                let file = PinnedFile::from_file(self.original.directory.clone(), name, raw)?;
                if entry_metadata(path, handle(&file.file))? != entry.metadata
                    || Some(file.digest()?) != entry.sha256
                {
                    return Err(blocked("later file changed before namespace mutation"));
                }
                MovedObject::File(file)
            };
            self.moved.insert(path.clone(), object);
            let subtree: Vec<_> = self
                .plan
                .current
                .entries
                .iter()
                .filter(|entry| {
                    entry.metadata.path == *path
                        || entry.metadata.path.starts_with(&format!("{path}/"))
                })
                .cloned()
                .collect();
            self.verify_moved_subtree(path, &subtree)?;
            expected_names.push(path.clone());
        }
        let mut actual = self
            .original
            .directory
            .read_child_entries(SnapshotLimits::default().max_entries)?
            .into_iter()
            .map(|entry| text(&entry.name))
            .collect::<io::Result<Vec<_>>>()?;
        actual.sort();
        expected_names.sort();
        if actual != expected_names {
            return Err(blocked("later namespace changed before evacuation"));
        }
        for held in self.moved.values() {
            held.verify()?;
        }
        Ok(())
    }
    fn move_actual(
        &mut self,
        path: &str,
        destination: Arc<Directory>,
        name: ComponentName,
    ) -> io::Result<()> {
        let held = self
            .moved
            .get_mut(path)
            .ok_or_else(|| blocked("evacuation handle missing"))?;
        held.verify()?;
        match held {
            MovedObject::Directory(root, tree) => {
                // Keep the actual DELETE-capable root across the descendant
                // sharing gap and every uncertain rename result.
                drop(tree.take());
                root.rename_to(destination, name)?;
            }
            MovedObject::File(file) => {
                file.rename_to(destination, name)?;
            }
            MovedObject::Fenced(fence) => {
                fence.lock().rename_to(destination, name)?;
            }
        }
        Ok(())
    }

    fn verify_moved_subtree(&mut self, path: &str, expected: &[ManifestEntry]) -> io::Result<()> {
        let held = self
            .moved
            .get_mut(path)
            .ok_or_else(|| blocked("moved object missing"))?;
        held.verify()?;
        match held {
            MovedObject::Directory(root, retained) => {
                let observed = HeldTree::admit(
                    HeldRoot::Present(root.clone()),
                    &mut Budget::new(SnapshotLimits::default())?,
                    None,
                )?;
                let mut entries = observed.manifest.entries.clone();
                for entry in &mut entries {
                    entry.metadata.path = if entry.metadata.path.is_empty() {
                        path.into()
                    } else {
                        format!("{path}/{}", entry.metadata.path)
                    };
                }
                if entries != expected {
                    return Err(blocked("later subtree changed during evacuation"));
                }
                *retained = Some(Box::new(observed));
            }
            MovedObject::File(file) => {
                if expected.len() != 1
                    || entry_metadata(path, handle(&file.file))? != expected[0].metadata
                    || Some(file.digest()?) != expected[0].sha256
                {
                    return Err(blocked("evacuated file differs"));
                }
            }
            MovedObject::Fenced(fence) => {
                let fence = fence.lock();
                if expected.len() != 1
                    || convert_metadata(
                        path,
                        fence.context_metadata()?,
                        fence.context_descriptor()?,
                    )? != expected[0].metadata
                {
                    return Err(blocked("evacuated image differs"));
                }
                fence.verify()?;
            }
        }
        Ok(())
    }

    fn create_original(
        &mut self,
        index: usize,
        entry: &ManifestEntry,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        let path = &entry.metadata.path;
        let (parent_path, leaf) = path.rsplit_once('/').unwrap_or(("", path));
        let parent = self
            .directories
            .get(parent_path)
            .ok_or_else(|| blocked("original parent not created"))?
            .clone();
        let name = component(leaf)?;
        HeldRoot::Absent {
            parent: parent.clone(),
            name: name.clone(),
        }
        .verify()?;
        let directory = entry.metadata.kind == EntryType::Directory;
        let pending = self.effect(
            if directory {
                FilesystemOperation::CreateDirectory
            } else {
                FilesystemOperation::CopyFile
            },
            index,
            &(parent.identity(), leaf, "absent"),
            &("create-private-empty", entry),
            user,
            journal,
        )?;
        bundle_fault(BundleFault::BeforeCreate)?;
        let descriptor = user.descriptor(directory)?;
        let raw = parent.open_relative(
            &name,
            FILE_ACCESS_RIGHTS(FILE_ALL_ACCESS.0 & !DELETE.0),
            if directory {
                FILE_SHARE_READ | FILE_SHARE_WRITE
            } else {
                FILE_SHARE_READ
            },
            FILE_CREATE,
            directory,
            Some(&descriptor),
        )?;
        user.verify_private_file(handle(&raw), directory)?;
        self.writes.insert(path.clone(), raw);
        if directory {
            self.directories
                .insert(path.clone(), parent.open_directory(name)?);
        }
        let created = entry_metadata(path, handle(&self.writes[path]))?;
        journal.applied(pending, &created)?;
        if !directory {
            let copy_entry = self
                .original
                .saved
                .copy
                .copy
                .entries
                .get(index)
                .ok_or_else(|| blocked("original byte mapping missing"))?;
            let pending = self.effect(
                FilesystemOperation::CopyFile,
                index,
                &created,
                &(entry, copy_entry),
                user,
                journal,
            )?;
            let source = self
                .original
                .copy
                .tree
                .as_ref()
                .ok_or_else(|| blocked("original copy guards missing"))?;
            let mut reader = source.reader(&copy_entry.metadata)?;
            let mut output = &self.writes[path];
            let mut bytes = [0u8; 65536];
            loop {
                let count = reader.read(&mut bytes)?;
                if count == 0 {
                    break;
                }
                output.write_all(&bytes[..count])?;
                bundle_fault(BundleFault::AfterWrite)?;
            }
            unsafe { FlushFileBuffers(handle(&self.writes[path])) }.map_err(win_error)?;
            bundle_fault(BundleFault::AfterFlush)?;
            if raw_digest(&self.writes[path])?
                != entry
                    .sha256
                    .clone()
                    .ok_or_else(|| blocked("original file digest missing"))?
            {
                return Err(blocked("restored original bytes differ"));
            }
            journal.applied(
                pending,
                &(
                    entry_metadata(path, handle(&self.writes[path]))?,
                    &entry.sha256,
                ),
            )?;
        }
        Ok(())
    }

    fn restore_security(
        &mut self,
        index: usize,
        entry: &ManifestEntry,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        let path = &entry.metadata.path;
        let file = self
            .writes
            .get(path)
            .ok_or_else(|| blocked("permission object missing"))?;
        let PermissionRecord::Windows {
            descriptor,
            attributes,
        } = &entry.metadata.permissions
        else {
            return Err(blocked("unsupported original permissions"));
        };
        let validated = ValidatedDescriptor::from_bytes(descriptor)?;
        validated.require_assignable(user)?;
        let before = entry_metadata(path, handle(file))?;
        let pending = self.effect(
            FilesystemOperation::SetPermissions,
            index,
            &before,
            &("owner-group-dacl", descriptor),
            user,
            journal,
        )?;
        let mut owner = PSID::default();
        let mut group = PSID::default();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut defaulted = windows_core::BOOL::default();
        let mut present = windows_core::BOOL::default();
        unsafe {
            GetSecurityDescriptorOwner(validated.raw(), &mut owner, &mut defaulted)
                .map_err(win_error)?;
            GetSecurityDescriptorGroup(validated.raw(), &mut group, &mut defaulted)
                .map_err(win_error)?;
            GetSecurityDescriptorDacl(validated.raw(), &mut present, &mut dacl, &mut defaulted)
                .map_err(win_error)?;
        }
        // Reuse the reviewed shortcut pattern: accessors must point at the
        // exact bounded fields in this aligned, still-owned descriptor.
        let base = validated.raw().0.cast::<u8>();
        let field = |offset: usize| -> *mut core::ffi::c_void {
            let relative = u32::from_le_bytes(
                descriptor[offset..offset + 4]
                    .try_into()
                    .expect("validated descriptor field"),
            ) as usize;
            unsafe { base.add(relative).cast() }
        };
        if owner.0 != field(4)
            || group.0 != field(8)
            || dacl.cast() != field(16)
            || !present.as_bool()
            || owner.0.is_null()
            || group.0.is_null()
            || dacl.is_null()
            || !unsafe { IsValidSid(owner) }.as_bool()
            || !unsafe { IsValidSid(group) }.as_bool()
            || !unsafe { IsValidAcl(dacl) }.as_bool()
        {
            return Err(blocked("unsupported installation owner group or DACL"));
        }
        unsafe {
            SetSecurityInfo(
                handle(file),
                SE_FILE_OBJECT,
                validated.information(),
                Some(owner),
                Some(group),
                Some(dacl.cast_const()),
                None,
            )
            .ok()
        }
        .map_err(win_error)?;
        bundle_fault(BundleFault::AfterPermissions)?;
        let after = entry_metadata(path, handle(file))?;
        if !matches!(&after.permissions, PermissionRecord::Windows { descriptor: actual, .. } if restored_file_descriptor_matches(descriptor, actual))
        {
            #[cfg(test)]
            if let PermissionRecord::Windows {
                descriptor: actual, ..
            } = &after.permissions
            {
                super::super::shortcuts::probe_descriptor_difference("bundle", descriptor, actual);
            }
            return Err(blocked("restored owner group or DACL differs"));
        }
        journal.applied(pending, &after)?;
        let pending = self.effect(
            FilesystemOperation::SetPermissions,
            index,
            &after,
            &("attributes", attributes),
            user,
            journal,
        )?;
        let basic = FILE_BASIC_INFO {
            FileAttributes: *attributes,
            ..Default::default()
        };
        unsafe {
            SetFileInformationByHandle(
                handle(file),
                FileBasicInfo,
                (&basic as *const FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        }
        .map_err(win_error)?;
        if entry.metadata.kind == EntryType::File {
            unsafe { FlushFileBuffers(handle(file)) }.map_err(win_error)?;
        }
        let actual = entry_metadata(path, handle(file))?;
        if !restored_permissions_match(&entry.metadata.permissions, &actual.permissions) {
            return Err(blocked("restored attributes differ"));
        }
        journal.applied(pending, &actual)
    }

    fn evacuated_manifest(&self) -> io::Result<Vec<(String, String)>> {
        self.moved
            .iter()
            .map(|(name, held)| {
                held.verify()?;
                let id = match held {
                    MovedObject::File(file) => identity(file.identity())?,
                    MovedObject::Directory(root, _) => identity(root.identity())?,
                    MovedObject::Fenced(fence) => identity(fence.lock().identity())?,
                };
                Ok((name.clone(), id))
            })
            .collect()
    }
}

fn raw_digest(file: &File) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = file;
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
/// Restore verification has a narrower, directional Windows contract than the
/// raw PermissionRecord equality used for capture, custody and retained copies.
fn restored_permissions_match(expected: &PermissionRecord, actual: &PermissionRecord) -> bool {
    match (expected, actual) {
        (
            PermissionRecord::Windows {
                descriptor: expected,
                attributes: expected_attributes,
            },
            PermissionRecord::Windows {
                descriptor: actual,
                attributes: actual_attributes,
            },
        ) => {
            expected_attributes == actual_attributes
                && restored_file_descriptor_matches(expected, actual)
        }
        _ => false,
    }
}

fn verify_logical_restore(original: &TreeManifest, actual: &TreeManifest) -> io::Result<()> {
    if original.entries.len() != actual.entries.len() {
        return Err(blocked("restored installation namespace differs"));
    }
    for (expected, observed) in original.entries.iter().zip(&actual.entries) {
        if expected.metadata.path != observed.metadata.path
            || expected.metadata.kind != observed.metadata.kind
            || expected.metadata.size != observed.metadata.size
            || expected.metadata.link_count != observed.metadata.link_count
            || !restored_permissions_match(
                &expected.metadata.permissions,
                &observed.metadata.permissions,
            )
            || expected.sha256 != observed.sha256
        {
            return Err(blocked("restored installation logical contract differs"));
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReturnResult {
    schema: u32,
    #[serde(default)]
    attempt: AttemptNames,
    #[serde(default)]
    history: Vec<HistorySnapshot>,
    transaction: String,
    plan: ManagerRecordReference,
    source: ManagerRecordReference,
    restored: TreeManifest,
    later: PrivateCopyManifest,
    evacuated: Vec<(String, String)>,
    effect_id: String,
    quarantine: FileIdentity,
}

/// A live, nonserializable receipt. The manager's final proof must retain this
/// complete observation and call verify; the protected digest is not proof.
pub(crate) struct RestoredInstallationBundle {
    original: Arc<RetainedInstallationBundle>,
    boundary: Arc<ReturnBoundary>,
    held: HeldTree,
    later: PrivateTreeCopy,
    quarantine: Arc<PrivateDirectory>,
    moved: BTreeMap<String, MovedObject>,
    history: BundleHistory,
    record: ManagerRecord,
    result: ReturnResult,
}

/// Selectors read from an Applied result and matched to the actual canonical
/// image. These are observations only; acquire a fresh fence and reopen the
/// complete restored bundle before treating them as completion evidence.
pub(crate) struct BundleCompletionObservation {
    pub(crate) record: ManagerRecordReference,
    pub(crate) image_identity: FileIdentity,
    pub(crate) image_digest: String,
}
impl RestoredInstallationBundle {
    pub(crate) fn observe_completed(
        original: &RetainedInstallationBundle,
        expected_plan: &ManagerRecordReference,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<BundleCompletionObservation> {
        original.verify(user)?;
        original.verify_journal(journal)?;
        require_phase(journal, &[JournalPhase::Restoring, JournalPhase::Restored])?;
        if safe(journal.store.latest_bundle_backup())?.is_some() {
            return Err(blocked("completed original installation attempt changed"));
        }
        let (effect_id, bytes) = safe(
            journal
                .store
                .applied_effect_observation(&EffectKind::VerifySourceBundleRestore),
        )?;
        let (reference, observed): (ManagerRecordReference, ReturnResult) =
            serde_json::from_slice(&bytes)?;
        let record = ManagerRecord::open(original.data.clone(), RETURN_RESULT, &reference, user)?;
        let result: ReturnResult = record.decode(user)?;
        let plan_record =
            ManagerRecord::open(original.data.clone(), RETURN_PLAN, expected_plan, user)?;
        let plan: ReturnPlan = plan_record.decode(user)?;
        if encoded(&result)? != encoded(&observed)?
            || result.schema != 1
            || plan.schema != 1
            || result.effect_id != effect_id
            || result.transaction != journal.binding.transaction_id
            || plan.transaction != result.transaction
            || result.source != *original.reference()
            || plan.source != result.source
            || result.plan != *expected_plan
            || result.attempt != AttemptNames::default()
            || plan.attempt != result.attempt
            || !result.history.is_empty()
            || !plan.history.is_empty()
            || plan.slot != original.saved.slot
            || result.later != plan.later_copy
            || plan.current != plan.later_copy.source
        {
            return Err(blocked("completed installation receipt differs"));
        }
        plan.slot.verify(&original.directory)?;
        result.restored.encode()?;
        verify_logical_restore(&original.saved.source.tree, &result.restored)?;
        let image = result
            .restored
            .entries
            .iter()
            .find(|entry| entry.metadata.path == original.saved.source.original_image_name)
            .filter(|entry| entry.metadata.kind == EntryType::File)
            .ok_or_else(|| blocked("completed installation image missing"))?;
        let current = original.directory.open_file(
            component(&original.saved.source.original_image_name)?,
            FileAccess::Read,
        )?;
        let image_digest = current.digest()?;
        if image.metadata.object_identity != identity(current.identity())?
            || image.sha256.as_ref() != Some(&image_digest)
        {
            return Err(blocked("completed installation image changed"));
        }
        current.verify()?;
        journal.verify()?;
        Ok(BundleCompletionObservation {
            record: reference,
            image_identity: current.identity().clone(),
            image_digest,
        })
    }
    /// Reopen only a positively journaled final receipt. A result file written
    /// before a lost journal receipt cannot mint completion on restart.
    pub(crate) fn reopen(
        original: Arc<RetainedInstallationBundle>,
        boundary: Arc<ReturnBoundary>,
        expected: &ManagerRecordReference,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        Self::reopen_return(
            original,
            boundary,
            &BundleReturnReference {
                attempt: AttemptNames::default(),
                record: expected.clone(),
            },
            user,
            journal,
        )
    }
    pub(crate) fn reopen_return(
        original: Arc<RetainedInstallationBundle>,
        boundary: Arc<ReturnBoundary>,
        expected: &BundleReturnReference,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        original.verify(user)?;
        original.verify_journal(journal)?;
        safe(boundary.verify_live())?;
        if boundary.installation().identity() != original.directory.identity()
            || boundary.binding() != &journal.binding
        {
            return Err(blocked("reopened return boundary differs"));
        }
        let record = ManagerRecord::open(
            original.data.clone(),
            &expected.attempt.name(RETURN_RESULT)?,
            &expected.record,
            user,
        )?;
        let result: ReturnResult = record.decode(user)?;
        let plan_record = ManagerRecord::open(
            original.data.clone(),
            &result.attempt.name(RETURN_PLAN)?,
            &result.plan,
            user,
        )?;
        let plan: ReturnPlan = plan_record.decode(user)?;
        if result.attempt != expected.attempt
            || plan.attempt != result.attempt
            || plan.history != result.history
            || result.schema != 1
            || plan.schema != 1
            || result.transaction != journal.binding.transaction_id
            || plan.transaction != result.transaction
            || result.source != *original.reference()
            || plan.source != result.source
            || result.later != plan.later_copy
            || plan.slot != original.saved.slot
        {
            return Err(blocked("reopened return records differ"));
        }
        let inspected = safe(journal.store.inspect(&journal.binding))?;
        let state = inspected
            .last_valid
            .ok_or_else(|| blocked("return journal missing"))?;
        if inspected.blocked
            || state.requires_reconciliation()
            || state.effect_observation(&result.effect_id) != Some(Observation::Applied)
            || !matches!(
                state.phase(),
                JournalPhase::Restoring | JournalPhase::Restored
            )
        {
            return Err(blocked("return completion receipt is not applied"));
        }
        let fenced = match boundary.current_image() {
            CurrentImageEvidence::Fenced(fence) => {
                if !fence
                    .lock()
                    .context_named_child(&original.directory, boundary.image_name())?
                {
                    return Err(blocked("restored entrypoint does not occupy original name"));
                }
                Some((boundary.image_name().clone(), fence.clone()))
            }
            CurrentImageEvidence::Absent(_) => None,
        };
        let held = HeldTree::admit(
            HeldRoot::Present(original.directory.clone()),
            &mut Budget::new(SnapshotLimits::default())?,
            fenced,
        )?;
        let later = PrivateTreeCopy::reopen(
            original.data.clone(),
            component(&result.attempt.name(LATER_COPY)?)?,
            result.later.clone(),
            user,
            SnapshotLimits::default(),
        )?;
        let quarantine = Arc::new(PrivateDirectory::open_existing(
            original.data.directory().clone(),
            component(&result.attempt.name(LATER_OBJECTS)?)?,
            user,
        )?);
        let mut moved = BTreeMap::new();
        for (name, identity_) in &result.evacuated {
            let source = result
                .later
                .source
                .entries
                .iter()
                .find(|entry| &entry.metadata.path == name)
                .ok_or_else(|| blocked("evacuated selector is outside protected plan"))?;
            if &source.metadata.object_identity != identity_ {
                return Err(blocked("evacuated selector identity changed"));
            }
            let held = if source.metadata.kind == EntryType::Directory {
                let root = quarantine.directory().open_directory(component(name)?)?;
                let tree = HeldTree::admit(
                    HeldRoot::Present(root.clone()),
                    &mut Budget::new(SnapshotLimits::default())?,
                    None,
                )?;
                MovedObject::Directory(root, Some(Box::new(tree)))
            } else {
                MovedObject::File(
                    quarantine
                        .directory()
                        .open_file(component(name)?, FileAccess::Read)?,
                )
            };
            moved.insert(name.clone(), held);
        }
        let history = BundleHistory::reopen(&original.data, &result.history, user)?;
        let restored = Self {
            original,
            boundary,
            held,
            later,
            quarantine,
            moved,
            history,
            record,
            result,
        };
        restored.verify(user)?;
        Ok(restored)
    }
    pub(crate) fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        self.original.verify(user)?;
        safe(self.boundary.verify_live())?;
        self.record.verify(user)?;
        self.history.verify()?;
        if self.history.snapshots() != self.result.history {
            return Err(blocked("restored history differs"));
        }
        self.held.verify()?;
        self.later.verify(user)?;
        self.quarantine.verify(user)?;
        verify_evacuated(
            &self.quarantine,
            &self.moved,
            &self.result.evacuated,
            &self.result.later.source,
        )?;
        if self.result.quarantine != *self.quarantine.directory().identity()
            || self.result.schema != 1
            || self.result.source != *self.original.reference()
            || self.result.transaction != self.boundary.binding().transaction_id
            || self.held.manifest != self.result.restored
            || self.later.manifest()? != &self.result.later
        {
            return Err(blocked("restored bundle receipt changed"));
        }
        verify_logical_restore(&self.original.saved.source.tree, &self.held.manifest)
    }
    pub(crate) fn manifest(&self) -> &TreeManifest {
        &self.held.manifest
    }
    pub(crate) fn source_manifest(&self) -> &InstalledBundleManifest {
        &self.original.saved.source
    }
    pub(crate) fn directory(&self) -> &Arc<Directory> {
        &self.original.directory
    }
    pub(crate) fn later_manifest(&self) -> &PrivateCopyManifest {
        &self.result.later
    }
    #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
    pub(crate) fn acceptance_retained_directory(&self) -> io::Result<String> {
        let tree = self
            .later
            .tree
            .as_ref()
            .ok_or_else(|| blocked("retained bundle observation missing"))?;
        let HeldRoot::Present(root) = tree.root() else {
            return Err(blocked("retained bundle root absent"));
        };
        root.path()?
            .into_string()
            .map_err(|_| blocked("unrepresentable acceptance location"))
    }
    pub(crate) fn return_reference(&self) -> BundleReturnReference {
        BundleReturnReference {
            attempt: self.result.attempt.clone(),
            record: self.record.reference().clone(),
        }
    }
    pub(crate) fn reference(&self) -> &ManagerRecordReference {
        self.record.reference()
    }
}

/// Exact selectors are derived from canonical UUIDs retained by the journal,
/// never supplied paths. The first complete return plan remains the chain root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleRecoveryDestination {
    schema: u32,
    transaction: String,
    data_root: FileIdentity,
    original: ManagerRecordReference,
    seed: BundleSeedReference,
    attempts: Vec<AttemptNames>,
}
impl BundleRecoveryDestination {
    fn validate(
        &self,
        original: &RetainedInstallationBundle,
        seed: &BundleSeedReference,
    ) -> io::Result<()> {
        if self.schema != 1
            || self.transaction != original.saved.transaction
            || self.data_root != *original.data.directory().identity()
            || self.original != *original.reference()
            || self.seed != *seed
            || self.attempts.len() < 2
            || self.attempts.len() > 64
            || self.attempts[0] != AttemptNames::default()
        {
            return Err(blocked("bundle recovery selector differs"));
        }
        let mut distinct = BTreeSet::new();
        for (index, names) in self.attempts.iter().enumerate() {
            names.validate()?;
            if (index != 0 && names.id.is_none()) || !distinct.insert(names.id.clone()) {
                return Err(blocked("bundle recovery aliases an earlier attempt"));
            }
        }
        Ok(())
    }
}

/// Actual current installation and every prior partial copy/quarantine stay
/// held until a fresh independent attempt preserves the current state again.
/// This object never retries or completes an old unknown mutation.
pub(crate) struct InterruptedInstallationReturn {
    original: Arc<RetainedInstallationBundle>,
    boundary: Arc<ReturnBoundary>,
    seed: BundleSeed,
    seed_reference: BundleSeedReference,
    current: HeldTree,
    history: BundleHistory,
    previous_generation: Option<u64>,
    generation: u64,
}
impl InterruptedInstallationReturn {
    pub(crate) fn reopen(
        original: Arc<RetainedInstallationBundle>,
        boundary: Arc<ReturnBoundary>,
        expected_plan: &ManagerRecordReference,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        let record = ManagerRecord::open(original.data.clone(), RETURN_PLAN, expected_plan, user)?;
        let plan: ReturnPlan = record.decode(user)?;
        if plan.schema != 1
            || plan.source != *original.reference()
            || plan.attempt != AttemptNames::default()
        {
            return Err(blocked("initial return plan differs"));
        }
        Self::reopen_pending(original, boundary, user, journal)
    }
    pub(crate) fn reopen_pending(
        original: Arc<RetainedInstallationBundle>,
        boundary: Arc<ReturnBoundary>,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        original.verify(user)?;
        original.verify_journal(journal)?;
        safe(boundary.verify_current_image())?;
        if boundary.binding() != &journal.binding
            || boundary.installation().identity() != original.directory.identity()
        {
            return Err(blocked("restart return evidence differs"));
        }
        let (generation, digest) = safe(journal.store.latest_bundle_seed())?
            .ok_or_else(|| blocked("bundle return seed missing"))?;
        let seed_reference = BundleSeedReference { generation, digest };
        let seed: BundleSeed =
            serde_json::from_slice(&safe(journal.store.read_manifest(&seed_reference.digest))?)?;
        if seed.schema != 1
            || seed.transaction != journal.binding.transaction_id
            || seed.original != *original.reference()
            || seed.slot != original.saved.slot
            || seed.data_root != *original.data.directory().identity()
        {
            return Err(blocked("bundle return seed differs"));
        }
        let active = safe(journal.store.latest_bundle_backup())?;
        let attempts = if let Some((_, plan)) = &active {
            let selector: BundleRecoveryDestination =
                serde_json::from_slice(&safe(journal.store.read_manifest(&plan.destination))?)?;
            selector.validate(&original, &seed_reference)?;
            selector.attempts
        } else {
            vec![AttemptNames::default()]
        };
        let history = BundleHistory::observe(&original.data, &attempts, user)?;
        history.verify_retained_evidence(&seed, &seed_reference, active.as_ref(), journal.store)?;
        let fenced = match boundary.current_image() {
            CurrentImageEvidence::Fenced(fence) => {
                Some((boundary.image_name().clone(), fence.clone()))
            }
            CurrentImageEvidence::Absent(absence) => {
                absence.verify()?;
                None
            }
        };
        let current = HeldTree::admit(
            HeldRoot::Present(original.directory.clone()),
            &mut Budget::new(SnapshotLimits::default())?,
            fenced,
        )?;
        verify_bundle_confidential(&current, user)?;
        journal.verify()?;
        let result = Self {
            original,
            boundary,
            seed,
            seed_reference,
            current,
            history,
            previous_generation: active.map(|(generation, _)| generation),
            generation: journal.generation,
        };
        result.verify(user)?;
        Ok(result)
    }
    pub(crate) fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        self.original.verify(user)?;
        safe(self.boundary.verify_live())?;
        if self.seed.original != *self.original.reference()
            || self.seed.slot != self.original.saved.slot
        {
            return Err(blocked("bundle seed source differs"));
        }
        self.current.verify()?;
        verify_bundle_confidential(&self.current, user)?;
        self.history.verify()
    }
    pub(crate) fn current_manifest(&self) -> &TreeManifest {
        &self.current.manifest
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// A new UUID-named copy and a new series of effects, under the SAME journal
    /// and permanent marker. The old unknown receipt remains historical Unknown.
    /// If this copy is interrupted, reopen() discovers its exact protected
    /// selector from the journal even though no complete return-plan file exists.
    pub(crate) fn prepare_fresh_attempt(
        self,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<BundleRestoration> {
        self.verify(user)?;
        self.original.verify_journal(journal)?;
        require_space(
            self.original.data.directory(),
            bytes_in(&self.current.manifest)?,
        )?;
        require_space(
            &self.original.directory,
            bytes_in(&self.original.saved.source.tree)?,
        )?;
        let root_write = open_root_permissions(&self.original.directory)?;
        let attempt = AttemptNames::fresh();
        let mut attempts = self.history.attempts();
        attempts.push(attempt.clone());
        let destination = BundleRecoveryDestination {
            schema: 1,
            transaction: journal.binding.transaction_id.clone(),
            data_root: self.original.data.directory().identity().clone(),
            original: self.original.reference().clone(),
            seed: self.seed_reference.clone(),
            attempts,
        };
        let evidence = BundleRecoveryEvidence {
            observed: &self,
            destination: &destination,
            user,
            root: &journal.root,
            lease: journal.exclusive()?,
            generation: journal.generation,
        };
        let (generation, digest) = safe(journal.store.prepare_bundle_backup(&evidence))?;
        journal.generation = generation;
        journal.verify()?;
        let mut later = PrivateTreeCopy::new(
            self.original.data.clone(),
            component(&attempt.name(LATER_COPY)?)?,
        );
        later.copy_from_plan(&self.current, user, journal, Some((generation, digest)))?;
        let later = seal_copy(later, user)?;
        self.verify(user)?;
        let plan = ReturnPlan {
            schema: 1,
            attempt: attempt.clone(),
            history: self.history.snapshots(),
            transaction: journal.binding.transaction_id.clone(),
            source: self.original.reference().clone(),
            slot: self.original.saved.slot.clone(),
            current: self.current.manifest.clone(),
            later_copy: later.manifest()?.clone(),
            detached_image: self.current.detached_image.clone(),
            image_identity: match self.boundary.current_image() {
                CurrentImageEvidence::Fenced(fence) => Some(fence.lock().identity().clone()),
                CurrentImageEvidence::Absent(_) => None,
            },
        };
        let plan_record = ManagerRecord::create(
            self.original.data.clone(),
            &attempt.name(RETURN_PLAN)?,
            &plan,
            user,
        )?;
        let result = BundleRestoration {
            directories: BTreeMap::from([(String::new(), self.original.directory.clone())]),
            writes: BTreeMap::from([(String::new(), root_write)]),
            original: self.original,
            boundary: self.boundary,
            later,
            current: Some(self.current),
            plan_record,
            plan,
            quarantine: None,
            moved: BTreeMap::new(),
            attempted: false,
            history: self.history,
            admitted_generation: Some(generation),
            pending_readback: None,
            pending_result_record: None,
            pending_restored: None,
        };
        result.verify_dependencies(user, journal)?;
        Ok(result)
    }
}

/// Protected seed admission precedes the first later-copy effect, so a crash
/// cannot strand an attempt before its complete return-plan file exists.
pub(crate) struct BundleStartEvidence<'a> {
    original: &'a RetainedInstallationBundle,
    boundary: &'a ReturnBoundary,
    current: &'a HeldTree,
    seed: &'a BundleSeed,
    user: &'a CurrentUser,
    root: &'a Arc<PrivateDirectory>,
    lease: &'a ExclusiveLease,
    generation: u64,
}
pub(crate) struct BundleStartRequest {
    pub(crate) generation: u64,
    pub(crate) current: Vec<u8>,
    pub(crate) seed: Vec<u8>,
    pub(crate) effects: usize,
}
impl BundleStartEvidence<'_> {
    pub(crate) fn verify(&self, store: &mut JournalStore) -> io::Result<BundleStartRequest> {
        self.original.verify(self.user)?;
        safe(self.boundary.verify_current_image())?;
        self.current.verify()?;
        verify_bundle_confidential(self.current, self.user)?;
        self.lease.verify_root(self.root)?;
        safe(store.verify_windows_binding(self.root, self.boundary.binding(), self.generation))?;
        if safe(store.latest_bundle_seed())?.is_some() {
            return Err(blocked("initial bundle copy already admitted"));
        }
        if self.seed.schema != 1
            || self.seed.current != self.current.manifest
            || self.seed.original != *self.original.reference()
            || self.seed.slot != self.original.saved.slot
            || self.seed.transaction != self.boundary.binding().transaction_id
            || self.seed.data_root != *self.original.data.directory().identity()
            || self.boundary.installation().identity() != self.original.directory.identity()
        {
            return Err(blocked("bundle seed differs from live admission"));
        }
        let inspected = safe(store.inspect(self.boundary.binding()))?;
        let state = inspected
            .last_valid
            .ok_or_else(|| blocked("bundle journal missing"))?;
        if inspected.blocked
            || state.requires_reconciliation()
            || !matches!(
                state.phase(),
                JournalPhase::RecoveryRequired
                    | JournalPhase::HistoricalActive
                    | JournalPhase::InstalledUnconfirmed
            )
            || state.manifest(ManifestRole::SourceBundle)
                != Some(identity(&self.original.saved.source)?.as_str())
        {
            return Err(blocked("bundle seed phase or source role differs"));
        }
        for base in [LATER_COPY, LATER_OBJECTS, RETURN_PLAN, RETURN_RESULT] {
            HeldRoot::Absent {
                parent: self.original.data.directory().clone(),
                name: component(base)?,
            }
            .verify()?;
        }
        let current = self.current.manifest.encode()?;
        if current
            .len()
            .checked_mul(2)
            .and_then(|size| {
                size.checked_add(self.current.manifest.entries.len().saturating_mul(1024) + 4096)
            })
            .is_none_or(|size| size > MAX_MANIFEST_BYTES)
        {
            return Err(blocked("bundle copy wrapper exceeds capacity"));
        }
        Ok(BundleStartRequest {
            generation: self.generation,
            current,
            seed: encoded(self.seed)?,
            effects: self.current.manifest.entries.len(),
        })
    }
}

/// The journal can consume only this live capability; every field is private
/// and observations come from retained Windows handles, not a serialized proof.
pub(crate) struct BundleRecoveryEvidence<'a> {
    observed: &'a InterruptedInstallationReturn,
    destination: &'a BundleRecoveryDestination,
    user: &'a CurrentUser,
    root: &'a Arc<PrivateDirectory>,
    lease: &'a ExclusiveLease,
    generation: u64,
}
pub(crate) struct BundleRecoveryRequest {
    pub(crate) generation: u64,
    pub(crate) current: Vec<u8>,
    pub(crate) destination: Vec<u8>,
    pub(crate) previous: Vec<u8>,
    pub(crate) previous_generation: Option<u64>,
    pub(crate) abandoned_effect: Option<(String, u64)>,
    pub(crate) effects: usize,
}
impl BundleRecoveryEvidence<'_> {
    pub(crate) fn verify(&self, store: &mut JournalStore) -> io::Result<BundleRecoveryRequest> {
        self.observed.verify(self.user)?;
        self.lease.verify_root(self.root)?;
        safe(store.verify_windows_binding(
            self.root,
            self.observed.boundary.binding(),
            self.generation,
        ))?;
        if self.generation != self.observed.generation {
            return Err(blocked("bundle recovery observation is stale"));
        }
        let inspected = safe(store.inspect(self.observed.boundary.binding()))?;
        let state = inspected
            .last_valid
            .ok_or_else(|| blocked("bundle recovery journal missing"))?;
        if inspected.blocked
            || !matches!(
                state.phase(),
                JournalPhase::RecoveryRequired
                    | JournalPhase::HistoricalActive
                    | JournalPhase::InstalledUnconfirmed
                    | JournalPhase::Restoring
            )
        {
            return Err(blocked("bundle recovery phase changed"));
        }
        let active = safe(store.latest_bundle_backup())?;
        if active.as_ref().map(|(generation, _)| *generation) != self.observed.previous_generation {
            return Err(blocked("bundle recovery attempt was superseded"));
        }
        self.observed.history.verify_retained_evidence(
            &self.observed.seed,
            &self.observed.seed_reference,
            active.as_ref(),
            store,
        )?;
        if let Some((_, plan)) = &active {
            let selector: BundleRecoveryDestination =
                serde_json::from_slice(&safe(store.read_manifest(&plan.destination))?)?;
            selector.validate(&self.observed.original, &self.observed.seed_reference)?;
            if selector.attempts != self.observed.history.attempts() {
                return Err(blocked("bundle recovery history selector changed"));
            }
        }
        self.destination
            .validate(&self.observed.original, &self.observed.seed_reference)?;
        let previous = self.observed.history.attempts();
        if self.destination.attempts.len() != previous.len() + 1
            || self.destination.attempts[..previous.len()] != previous
        {
            return Err(blocked(
                "fresh bundle attempt does not retain every predecessor",
            ));
        }
        let attempt = self
            .destination
            .attempts
            .last()
            .ok_or_else(|| blocked("missing fresh attempt"))?;
        for base in [LATER_COPY, LATER_OBJECTS, RETURN_PLAN, RETURN_RESULT] {
            HeldRoot::Absent {
                parent: self.observed.original.data.directory().clone(),
                name: component(&attempt.name(base)?)?,
            }
            .verify()?;
        }
        let seed =
            safe(store.latest_bundle_seed())?.ok_or_else(|| blocked("bundle seed missing"))?;
        if seed
            != (
                self.observed.seed_reference.generation,
                self.observed.seed_reference.digest.clone(),
            )
            || safe(store.read_manifest(&seed.1))? != encoded(&self.observed.seed)?
        {
            return Err(blocked("bundle seed was replaced"));
        }
        let pending = safe(store.context_pending())?;
        if let Some((effect, _)) = &pending {
            match &effect.kind {
                EffectKind::PrivateBackupEntry {
                    plan_generation,
                    manifest,
                    ..
                } => {
                    let (expected_generation, expected_manifest) =
                        if let Some((generation, plan)) = &active {
                            (*generation, plan.current_manifest.clone())
                        } else {
                            (
                                self.observed.seed_reference.generation,
                                self.observed.seed.current.digest()?,
                            )
                        };
                    if *plan_generation != expected_generation || *manifest != expected_manifest {
                        return Err(blocked("pending bundle copy belongs to another attempt"));
                    }
                }
                EffectKind::RecoveryFilesystemEntry { .. }
                | EffectKind::VerifySourceBundleRestore => {
                    if let EffectKind::RecoveryFilesystemEntry {
                        context, manifest, ..
                    } = &effect.kind
                    {
                        if context != &self.observed.boundary.binding().source_context
                            || *manifest != identity(&self.observed.original.saved.source)?
                        {
                            return Err(blocked("pending mutation source role differs"));
                        }
                    }
                    let before: BundleEffectBefore =
                        serde_json::from_slice(&safe(store.read_manifest(&effect.before))?)?;
                    let expected = self
                        .observed
                        .history
                        .attempts()
                        .last()
                        .cloned()
                        .ok_or_else(|| blocked("missing prior attempt"))?;
                    if before.schema != 1 || before.attempt != expected {
                        return Err(blocked(
                            "pending bundle mutation belongs to another attempt",
                        ));
                    }
                    let record = ManagerRecord::open(
                        self.observed.original.data.clone(),
                        &expected.name(RETURN_PLAN)?,
                        &before.plan,
                        self.user,
                    )?;
                    let plan: ReturnPlan = record.decode(self.user)?;
                    let expected_source = if let Some((_, active)) = &active {
                        active.current_manifest.clone()
                    } else {
                        self.observed.seed.current.digest()?
                    };
                    if plan.attempt != expected
                        || plan.source != *self.observed.original.reference()
                        || plan.transaction != self.observed.seed.transaction
                        || plan.current.digest()? != expected_source
                    {
                        return Err(blocked(
                            "pending mutation plan differs from admitted attempt",
                        ));
                    }
                }
                _ => return Err(blocked("pending effect is outside bundle recovery")),
            }
        }
        let abandoned_effect = pending.map(|(effect, generation)| (effect.effect_id, generation));
        Ok(BundleRecoveryRequest {
            generation: self.generation,
            current: self.observed.current.manifest.encode()?,
            destination: encoded(self.destination)?,
            previous: encoded(&(
                &self.observed.seed_reference,
                self.observed.history.snapshots(),
            ))?,
            previous_generation: self.observed.previous_generation,
            abandoned_effect,
            effects: self.observed.current.manifest.entries.len(),
        })
    }
}

fn verify_evacuated(
    quarantine: &PrivateDirectory,
    moved: &BTreeMap<String, MovedObject>,
    identities: &[(String, String)],
    original: &TreeManifest,
) -> io::Result<()> {
    let mut actual = quarantine
        .directory()
        .read_child_entries(SnapshotLimits::default().max_entries)?
        .into_iter()
        .map(|child| text(&child.name))
        .collect::<io::Result<Vec<_>>>()?;
    actual.sort();
    let expected: Vec<_> = identities.iter().map(|(name, _)| name.clone()).collect();
    if actual != expected || moved.len() != identities.len() {
        return Err(blocked("retained later namespace changed"));
    }
    for (name, id) in identities {
        let object = moved
            .get(name)
            .ok_or_else(|| blocked("retained later guard missing"))?;
        object.verify()?;
        let source = original
            .entries
            .iter()
            .find(|entry| &entry.metadata.path == name)
            .ok_or_else(|| blocked("retained later source missing"))?;
        if &source.metadata.object_identity != id {
            return Err(blocked("retained later identity mapping differs"));
        }
        let observed = match object {
            MovedObject::File(file) => {
                if Some(file.digest()?) != source.sha256 {
                    return Err(blocked("retained later bytes changed"));
                }
                entry_metadata(name, handle(&file.file))?
            }
            MovedObject::Fenced(fence) => {
                let fence = fence.lock();
                fence.verify()?;
                convert_metadata(name, fence.context_metadata()?, fence.context_descriptor()?)?
            }
            MovedObject::Directory(root, held) => {
                let tree = held
                    .as_ref()
                    .ok_or_else(|| blocked("retained later subtree missing"))?;
                tree.verify()?;
                let mut entries = tree.manifest.entries.clone();
                for entry in &mut entries {
                    entry.metadata.path = if entry.metadata.path.is_empty() {
                        name.clone()
                    } else {
                        format!("{name}/{}", entry.metadata.path)
                    };
                }
                let expected: Vec<_> = original
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.metadata.path == *name
                            || entry.metadata.path.starts_with(&format!("{name}/"))
                    })
                    .cloned()
                    .collect();
                if entries != expected {
                    return Err(blocked("retained later subtree contract changed"));
                }
                entry_metadata(name, root.raw())?
            }
        };
        if observed != source.metadata {
            return Err(blocked("retained later security or identity changed"));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BundleFault {
    BeforeCreate,
    AfterWrite,
    AfterFlush,
    AfterPermissions,
    AfterGuardRelease,
    AfterMove,
    BeforeReadmission,
    BeforeFinalReceipt,
}
#[cfg(test)]
thread_local! { static BUNDLE_FAULT: std::cell::Cell<Option<BundleFault>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(crate) struct BundleProbe(std::marker::PhantomData<std::rc::Rc<()>>);
#[cfg(test)]
impl Drop for BundleProbe {
    fn drop(&mut self) {
        BUNDLE_FAULT.set(None);
    }
}
#[cfg(test)]
pub(crate) fn probe_bundle_failure(fault: BundleFault) -> BundleProbe {
    BUNDLE_FAULT.set(Some(fault));
    BundleProbe(std::marker::PhantomData)
}
fn bundle_fault(_fault: BundleFault) -> io::Result<()> {
    #[cfg(test)]
    if BUNDLE_FAULT.get() == Some(_fault) {
        BUNDLE_FAULT.set(None);
        return Err(io::Error::other(
            "injected installation return interruption",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/version_history_bundle_restore_windows.rs"]
#[allow(non_snake_case)]
mod tests;

#[cfg(test)]
#[path = "../../tests/version_history_bundle_preparation_windows.rs"]
mod preparation_tests;

#[cfg(test)]
#[path = "../../tests/version_history_bundle_reopen_windows.rs"]
#[allow(non_snake_case)]
mod reopen_tests;
