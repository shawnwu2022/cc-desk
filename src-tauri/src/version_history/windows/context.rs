//! Complete held-object observations and private copies. These are NOT scope,
//! quiescence or SnapshotBoundary factories. No ambient Desk/UDF/CLI path is
//! discovered here, and serialized manifests never authorize a filesystem open.
use super::{
    blocked,
    fence::ImageFence,
    files::{
        ComponentName, Directory, FileAccess, FileIdentity, Metadata, PinnedFile, PrivateDirectory,
    },
    handle,
    lease::{ControlLease, ExclusiveLease, SharedLease},
    security::{capture_file_descriptor, CurrentUser},
    win_error,
};
use crate::version_history::{
    journal::{
        EffectKind, EffectSpec, JournalBinding, JournalEvent, JournalStore, Observation,
        ObservedResult, PrivateBackupOperation, RootBackupPlan, RootKind,
    },
    maintenance::SnapshotBoundary,
    snapshot::{
        ContextReader, EntryMetadata, EntryType, ManifestEntry, PermissionRecord, RootInventory,
        SnapshotLimits,
    },
    verified_package::sha256,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    io::{self, Read, Seek, SeekFrom, Write},
    mem::offset_of,
    sync::Arc,
};
use windows::Wdk::Storage::FileSystem::FILE_CREATE;
use windows::Win32::{
    Foundation::{ERROR_HANDLE_EOF, HANDLE},
    Storage::FileSystem::{
        FileStreamInfo, FlushFileBuffers, GetFileInformationByHandleEx, FILE_ATTRIBUTE_ARCHIVE,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_NOT_CONTENT_INDEXED, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_SYSTEM,
        FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_STREAM_INFO, FILE_WRITE_DATA,
        READ_CONTROL, SYNCHRONIZE,
    },
};

const MAX_MANIFEST_BYTES: usize = 32 * 1024 * 1024;

fn encoded<T: Serialize>(value: &T) -> io::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(blocked("complete manifest exceeds capacity"));
    }
    Ok(bytes)
}
fn identity(value: &impl Serialize) -> io::Result<String> {
    Ok(sha256(&encoded(value)?))
}
fn safe<T>(result: Result<T, crate::cli::types::SafeError>) -> io::Result<T> {
    result.map_err(|_| blocked("context journal requires reconciliation"))
}
fn text(name: &ComponentName) -> io::Result<String> {
    name.os_string()
        .into_string()
        .map_err(|_| blocked("unrepresentable component"))
}

/// A supplied held root or exact absence below one supplied held parent. An
/// inaccessible, linked or wrong-type root is never converted to absence.
#[derive(Clone)]
pub(crate) enum HeldRoot {
    Present(Arc<Directory>),
    Absent {
        parent: Arc<Directory>,
        name: ComponentName,
    },
}
impl HeldRoot {
    pub(crate) fn observe(parent: Arc<Directory>, name: ComponentName) -> io::Result<Self> {
        match parent.open_directory(name.clone()) {
            Ok(root) => Ok(Self::Present(root)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let absent = Self::Absent { parent, name };
                absent.verify()?;
                Ok(absent)
            }
            Err(error) => Err(error),
        }
    }
    fn verify(&self) -> io::Result<()> {
        match self {
            Self::Present(root) => root.recheck(),
            Self::Absent { parent, name } => {
                parent.recheck()?;
                // Opening a directory is only used to establish exact NotFound.
                // A file/reparse/access/type failure does not establish absence.
                match parent.open_directory(name.clone()) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                    _ => Err(blocked("absent root is no longer certainly absent")),
                }
            }
        }
    }
    pub(crate) fn location_identity(&self) -> io::Result<String> {
        self.verify()?;
        match self {
            Self::Present(root) => identity(&(
                "present",
                root.identity(),
                root.path()?
                    .into_string()
                    .map_err(|_| blocked("unrepresentable root location"))?,
            )),
            Self::Absent { parent, name } => identity(&(
                "absent",
                parent.identity(),
                parent
                    .path()?
                    .into_string()
                    .map_err(|_| blocked("unrepresentable parent location"))?,
                text(name)?,
            )),
        }
    }
    fn anchor(&self) -> &Arc<Directory> {
        match self {
            Self::Present(root) => root,
            Self::Absent { parent, .. } => parent,
        }
    }
}

/// Private observations contain source permissions. They must not be exposed
/// as UI diagnostics or assigned to backup objects.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TreeManifest {
    schema: u32,
    pub(crate) location_identity: String,
    pub(crate) entries: Vec<ManifestEntry>,
}
impl TreeManifest {
    pub(crate) fn encode(&self) -> io::Result<Vec<u8>> {
        encoded(self)
    }
    pub(crate) fn digest(&self) -> io::Result<String> {
        identity(self)
    }
}

#[derive(Clone)]
enum FileGuard {
    Ordinary(Arc<Mutex<PinnedFile>>),
    Fenced(Arc<Mutex<ImageFence>>),
}
impl FileGuard {
    fn metadata(&self, path: &str) -> io::Result<EntryMetadata> {
        match self {
            Self::Ordinary(file) => {
                let file = file.lock();
                file.verify()?;
                entry_metadata(path, handle(&file.file))
            }
            Self::Fenced(fence) => {
                let fence = fence.lock();
                let meta = fence.context_metadata()?;
                fence.context_verify_streams()?;
                convert_metadata(path, meta, fence.context_descriptor()?)
            }
        }
    }
    fn digest(&self) -> io::Result<String> {
        match self {
            Self::Ordinary(file) => file.lock().digest(),
            Self::Fenced(fence) => {
                use sha2::{Digest, Sha256};
                let fence = fence.lock();
                fence.verify()?;
                let size = fence.context_metadata()?.size;
                let mut hash = Sha256::new();
                let mut offset = 0;
                let mut bytes = [0; 65536];
                loop {
                    let count = fence.context_read(offset, &mut bytes)?;
                    if count == 0 {
                        break;
                    }
                    offset += count as u64;
                    if offset > size {
                        return Err(blocked("fenced bundle image grew"));
                    }
                    hash.update(&bytes[..count]);
                }
                if offset != size {
                    return Err(blocked("fenced bundle image shrank"));
                }
                fence.verify()?;
                Ok(format!("{:x}", hash.finalize()))
            }
        }
    }
    fn read_at(&self, offset: u64, bytes: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Ordinary(file) => {
                let file = file.lock();
                file.verify()?;
                let mut reader = &file.file;
                reader.seek(SeekFrom::Start(offset))?;
                let count = reader.read(bytes)?;
                file.verify()?;
                Ok(count)
            }
            Self::Fenced(fence) => fence.lock().context_read(offset, bytes),
        }
    }
}
enum HeldEntry {
    Directory(Arc<Directory>),
    File(FileGuard),
}
impl HeldEntry {
    fn metadata(&self, path: &str) -> io::Result<EntryMetadata> {
        match self {
            Self::Directory(root) => {
                root.recheck()?;
                entry_metadata(path, root.raw())
            }
            Self::File(file) => file.metadata(path),
        }
    }
}
struct BoundedReader {
    file: FileGuard,
    expected: EntryMetadata,
    offset: u64,
}
impl Read for BoundedReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.file.metadata(&self.expected.path)? != self.expected {
            return Err(blocked("source metadata changed during read"));
        }
        let remaining = self.expected.size.saturating_sub(self.offset);
        let limit = bytes.len().min(remaining.saturating_add(1) as usize);
        let count = self.file.read_at(self.offset, &mut bytes[..limit])?;
        self.offset += count as u64;
        if self.offset > self.expected.size || (count == 0 && self.offset != self.expected.size) {
            return Err(blocked("source length changed during read"));
        }
        Ok(count)
    }
}

struct Budget {
    limits: SnapshotLimits,
    entries: usize,
    bytes: u64,
    manifest: usize,
}
impl Budget {
    fn new(limits: SnapshotLimits) -> io::Result<Self> {
        let maximum = SnapshotLimits::default();
        if limits.max_entries == 0
            || limits.max_entries > maximum.max_entries
            || limits.max_bytes > maximum.max_bytes
            || limits.max_file_bytes > maximum.max_file_bytes
            || limits.max_depth == 0
            || limits.max_depth > maximum.max_depth
        {
            return Err(blocked("unsupported complete capture limits"));
        }
        Ok(Self {
            limits,
            entries: 0,
            bytes: 0,
            manifest: 0,
        })
    }
    fn add(&mut self, entry: &EntryMetadata) -> io::Result<()> {
        self.entries += 1;
        self.bytes = self
            .bytes
            .checked_add(entry.size)
            .ok_or_else(|| blocked("capture size overflow"))?;
        self.manifest += encoded(entry)?.len() + 96;
        if self.entries > self.limits.max_entries
            || self.bytes > self.limits.max_bytes
            || entry.size > self.limits.max_file_bytes
            || self.manifest > MAX_MANIFEST_BYTES
            || entry.path.split('/').count() > self.limits.max_depth
        {
            return Err(blocked("complete capture exceeds budget"));
        }
        Ok(())
    }
}

pub(crate) struct HeldTree {
    root: HeldRoot,
    manifest: TreeManifest,
    entries: BTreeMap<String, HeldEntry>,
    limits: SnapshotLimits,
    detached_image: Option<String>,
    fenced_location: Option<(Arc<Mutex<ImageFence>>, String)>,
}
impl HeldTree {
    /// Complete read-only re-admission of private material. In particular this
    /// cannot inherit the authority of a previously held writable copy tree.
    pub(crate) fn capture_private(
        root: Arc<Directory>,
        limits: SnapshotLimits,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        let tree = Self::admit(HeldRoot::Present(root), &mut Budget::new(limits)?, None)?;
        for entry in tree.entries.values() {
            match entry {
                HeldEntry::Directory(root) => user.verify_private_file(root.raw(), true)?,
                HeldEntry::File(FileGuard::Ordinary(file)) => {
                    user.verify_private_file(handle(&file.lock().file), false)?;
                }
                HeldEntry::File(FileGuard::Fenced(_)) => {
                    return Err(blocked(
                        "private manager tree cannot contain a source fence",
                    ));
                }
            }
        }
        tree.verify()?;
        Ok(tree)
    }
    fn admit(
        root: HeldRoot,
        budget: &mut Budget,
        fenced: Option<(ComponentName, Arc<Mutex<ImageFence>>)>,
    ) -> io::Result<Self> {
        root.verify()?;
        let mut tree = Self {
            manifest: TreeManifest {
                schema: 1,
                location_identity: root.location_identity()?,
                entries: vec![],
            },
            root,
            entries: BTreeMap::new(),
            limits: budget.limits,
            detached_image: None,
            fenced_location: None,
        };
        if let HeldRoot::Present(directory) = &tree.root {
            tree.walk(directory.clone(), "", budget, fenced.as_ref())?;
        } else if fenced.is_some() {
            return Err(blocked("bundle image requires a present bundle"));
        }
        tree.manifest
            .entries
            .sort_by(|a, b| a.metadata.path.cmp(&b.metadata.path));
        let mut identities = BTreeSet::new();
        for entry in &tree.manifest.entries {
            if !identities.insert(&entry.metadata.object_identity) {
                return Err(blocked("aliased object in complete tree"));
            }
        }
        tree.verify()?;
        tree.manifest.encode()?;
        Ok(tree)
    }
    fn insert(&mut self, path: &str, held: HeldEntry, budget: &mut Budget) -> io::Result<()> {
        let metadata = held.metadata(path)?;
        budget.add(&metadata)?;
        let digest = match &held {
            HeldEntry::Directory(_) => None,
            HeldEntry::File(file) => Some(file.digest()?),
        };
        if self.entries.insert(path.into(), held).is_some() {
            return Err(blocked("duplicate complete tree path"));
        }
        self.manifest.entries.push(ManifestEntry {
            metadata,
            sha256: digest,
        });
        Ok(())
    }
    fn walk(
        &mut self,
        directory: Arc<Directory>,
        path: &str,
        budget: &mut Budget,
        fenced: Option<&(ComponentName, Arc<Mutex<ImageFence>>)>,
    ) -> io::Result<()> {
        if let Some((name, fence)) = fenced {
            if !fence.lock().context_original_child(&directory, name)? {
                return Err(blocked("fence does not originate in this bundle"));
            }
            self.fenced_location = Some((fence.clone(), fence_location(&fence.lock())?));
        }
        self.insert(path, HeldEntry::Directory(directory.clone()), budget)?;
        let children = directory.read_child_entries(self.limits.max_entries)?;
        let mut names = BTreeSet::new();
        for child in children {
            let name = text(&child.name)?;
            if !names.insert(name.to_lowercase()) {
                return Err(blocked("ambiguous case-folded tree name"));
            }
            let next = if path.is_empty() {
                name
            } else {
                format!("{path}/{name}")
            };
            if next.len() > 32768 || next.split('/').count() > self.limits.max_depth {
                return Err(blocked("complete capture depth exceeded"));
            }
            if child.directory {
                let held = directory.open_directory(child.name)?;
                self.walk(held, &next, budget, None)?;
            } else {
                let file = match fenced.filter(|(name, _)| same_component(name, &child.name)) {
                    Some((name, fence)) => {
                        if !fence.lock().context_named_child(&directory, name)? {
                            return Err(blocked("fenced image differs from bundle entry"));
                        }
                        FileGuard::Fenced(fence.clone())
                    }
                    None => FileGuard::Ordinary(Arc::new(Mutex::new(
                        directory.open_file(child.name, FileAccess::Read)?,
                    ))),
                };
                self.insert(&next, HeldEntry::File(file), budget)?;
            }
        }
        if let Some((name, fence)) = fenced {
            if !self.entries.contains_key(&text(name)?) {
                HeldRoot::Absent {
                    parent: directory.clone(),
                    name: name.clone(),
                }
                .verify()?;
                self.detached_image = Some(text(name)?);
                self.insert(
                    &text(name)?,
                    HeldEntry::File(FileGuard::Fenced(fence.clone())),
                    budget,
                )?;
            }
        }
        Ok(())
    }
    pub(crate) fn manifest(&self) -> &TreeManifest {
        &self.manifest
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.root.verify()?;
        if let Some((fence, location)) = &self.fenced_location {
            if fence_location(&fence.lock())? != *location {
                return Err(blocked("captured fence location changed"));
            }
        }
        if self.root.location_identity()? != self.manifest.location_identity {
            return Err(blocked("tree location changed"));
        }
        let mut children: BTreeMap<&str, Vec<(String, bool)>> = BTreeMap::new();
        for child in &self.manifest.entries {
            if child.metadata.path.is_empty()
                || self.detached_image.as_ref() == Some(&child.metadata.path)
            {
                continue;
            }
            let (parent, name) = child
                .metadata
                .path
                .rsplit_once('/')
                .unwrap_or(("", &child.metadata.path));
            children
                .entry(parent)
                .or_default()
                .push((name.to_owned(), child.metadata.kind == EntryType::Directory));
        }
        for names in children.values_mut() {
            names.sort();
        }
        for expected in &self.manifest.entries {
            let path = &expected.metadata.path;
            let held = self
                .entries
                .get(path)
                .ok_or_else(|| blocked("missing retained entry"))?;
            if held.metadata(path)? != expected.metadata {
                return Err(blocked("source permissions or metadata changed"));
            }
            match held {
                HeldEntry::File(file) => {
                    if Some(file.digest()?) != expected.sha256 {
                        return Err(blocked("source bytes changed"));
                    }
                }
                HeldEntry::Directory(directory) => {
                    let mut actual = directory
                        .read_child_entries(self.limits.max_entries)?
                        .into_iter()
                        .map(|child| Ok((text(&child.name)?, child.directory)))
                        .collect::<io::Result<Vec<_>>>()?;
                    actual.sort();
                    let expected_children =
                        children.get(path.as_str()).cloned().unwrap_or_default();
                    if actual != expected_children {
                        return Err(blocked("complete child inventory changed"));
                    }
                }
            }
        }
        Ok(())
    }
    fn reader(&self, expected: &EntryMetadata) -> io::Result<BoundedReader> {
        let Some(HeldEntry::File(file)) = self.entries.get(&expected.path) else {
            return Err(blocked("file absent from held manifest"));
        };
        if file.metadata(&expected.path)? != *expected {
            return Err(blocked("file differs from held manifest"));
        }
        Ok(BoundedReader {
            file: file.clone(),
            expected: expected.clone(),
            offset: 0,
        })
    }
}

pub(crate) struct HeldContext {
    trees: BTreeMap<RootKind, HeldTree>,
}
impl HeldContext {
    /// Scope discovery supplies Desk and every controller's actual reconciled
    /// UDF. This method verifies their supplied objects, not discovery completeness.
    pub(crate) fn capture(
        desk: HeldRoot,
        webview: HeldRoot,
        limits: SnapshotLimits,
    ) -> io::Result<Self> {
        require_distinct_roots(&desk, &webview)?;
        let mut budget = Budget::new(limits)?;
        let trees = BTreeMap::from([
            (RootKind::Desk, HeldTree::admit(desk, &mut budget, None)?),
            (
                RootKind::WebView,
                HeldTree::admit(webview, &mut budget, None)?,
            ),
        ]);
        let context = Self { trees };
        context.verify()?;
        Ok(context)
    }
    pub(crate) fn tree(&self, root: RootKind) -> &HeldTree {
        &self.trees[&root]
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        for tree in self.trees.values() {
            tree.verify()?;
        }
        Ok(())
    }
    pub(crate) fn root_identities(&self) -> BTreeMap<RootKind, String> {
        self.trees
            .iter()
            .map(|(kind, tree)| (*kind, tree.manifest.location_identity.clone()))
            .collect()
    }
}
impl ContextReader for HeldContext {
    fn inventory(&mut self) -> io::Result<Vec<RootInventory>> {
        self.verify()?;
        Ok(self
            .trees
            .iter()
            .map(|(root, tree)| RootInventory {
                root: *root,
                location_identity: tree.manifest.location_identity.clone(),
                // This reader does NOT supply SnapshotBoundary's exclusion proof.
                overlaps_shared_data: false,
                entries: tree
                    .manifest
                    .entries
                    .iter()
                    .map(|entry| entry.metadata.clone())
                    .collect(),
            })
            .collect())
    }
    fn open_file(&mut self, root: RootKind, entry: &EntryMetadata) -> io::Result<Box<dyn Read>> {
        Ok(Box::new(
            self.trees
                .get(&root)
                .ok_or_else(|| blocked("foreign context root"))?
                .reader(entry)?,
        ))
    }
}

/// Bundle inventory is a separate typed role. It cannot be passed off as the
/// two-root Desk/UDF snapshot, and includes all unknown installed companions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstalledBundleManifest {
    schema: u32,
    original_image_name: String,
    fenced_image_location: String,
    pub(crate) tree: TreeManifest,
}
pub(crate) struct HeldBundle {
    tree: HeldTree,
    manifest: InstalledBundleManifest,
}
impl HeldBundle {
    /// Complete manager-bootstrap copy input while the source is still alive.
    /// The read guards and scope are rechecked, but this is not quiescence or a
    /// sealed source snapshot; the coordinator must compare it again after exit.
    pub(crate) fn capture_registered_source(
        installation: &super::scope::RegisteredInstallation,
        limits: SnapshotLimits,
    ) -> io::Result<Self> {
        installation
            .recheck()
            .map_err(|_| blocked("registered source changed"))?;
        let image = installation.image();
        let original_image_name = text(&image.name)?;
        let tree = HeldTree::admit(
            HeldRoot::Present(installation.directory().clone()),
            &mut Budget::new(limits)?,
            None,
        )?;
        let entry = tree
            .manifest
            .entries
            .iter()
            .find(|entry| entry.metadata.path == original_image_name)
            .ok_or_else(|| blocked("registered image missing from bundle"))?;
        if entry.metadata.object_identity != identity(image.identity())?
            || entry.sha256 != Some(image.digest()?)
        {
            return Err(blocked("registered image differs from complete bundle"));
        }
        let image_location = identity(&(
            image.identity(),
            image
                .path()?
                .into_string()
                .map_err(|_| blocked("invalid registered image path"))?,
        ))?;
        let manifest = InstalledBundleManifest {
            schema: 1,
            original_image_name,
            fenced_image_location: image_location,
            tree: tree.manifest.clone(),
        };
        installation
            .recheck()
            .map_err(|_| blocked("registered source changed during capture"))?;
        tree.verify()?;
        Ok(Self { tree, manifest })
    }
    pub(crate) fn capture(
        root: Arc<Directory>,
        image_name: ComponentName,
        fence: Arc<Mutex<ImageFence>>,
        limits: SnapshotLimits,
    ) -> io::Result<Self> {
        let original_image_name = text(&image_name)?;
        let fenced_image_location = fence_location(&fence.lock())?;
        let tree = HeldTree::admit(
            HeldRoot::Present(root),
            &mut Budget::new(limits)?,
            Some((image_name, fence)),
        )?;
        let manifest = InstalledBundleManifest {
            schema: 1,
            original_image_name,
            fenced_image_location,
            tree: tree.manifest.clone(),
        };
        encoded(&manifest)?;
        Ok(Self { tree, manifest })
    }
    pub(crate) fn manifest(&self) -> &InstalledBundleManifest {
        &self.manifest
    }
    pub(crate) fn tree(&self) -> &HeldTree {
        &self.tree
    }
}
impl InstalledBundleManifest {
    /// Logical original installation identity is stable across the explicitly
    /// journaled image quarantine. Actual fence location remains separate data.
    pub(crate) fn logical_digest(&self) -> io::Result<String> {
        let entries: Vec<_> = self
            .tree
            .entries
            .iter()
            .map(|entry| {
                (
                    &entry.metadata.path,
                    entry.metadata.kind,
                    entry.metadata.size,
                    entry.metadata.link_count,
                    &entry.metadata.permissions,
                    &entry.sha256,
                )
            })
            .collect();
        identity(&(self.schema, &self.original_image_name, entries))
    }
    pub(crate) fn encode(&self) -> io::Result<Vec<u8>> {
        encoded(self)
    }
}

fn same_component(left: &ComponentName, right: &ComponentName) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Globalization::{CompareStringOrdinal, CSTR_EQUAL};
    let left: Vec<_> = left.os_string().encode_wide().collect();
    let right: Vec<_> = right.os_string().encode_wide().collect();
    unsafe { CompareStringOrdinal(&left, &right, true) == CSTR_EQUAL }
}
fn fence_location(fence: &ImageFence) -> io::Result<String> {
    let (object, path) = fence.observe_location()?;
    identity(&(
        object,
        path.into_string()
            .map_err(|_| blocked("invalid fenced image path"))?,
    ))
}
fn prospective_path(root: &HeldRoot) -> io::Result<Vec<u16>> {
    root.verify()?;
    let mut path = root
        .anchor()
        .path()?
        .into_string()
        .map_err(|_| blocked("invalid held root path"))?;
    if let HeldRoot::Absent { name, .. } = root {
        path = format!("{}\\{}", path.trim_end_matches('\\'), text(name)?);
    }
    Ok(path.trim_end_matches('\\').encode_utf16().collect())
}
fn require_distinct_roots(left: &HeldRoot, right: &HeldRoot) -> io::Result<()> {
    use windows::Win32::Globalization::{CompareStringOrdinal, CSTR_EQUAL};
    let left = prospective_path(left)?;
    let right = prospective_path(right)?;
    let contains = |parent: &[u16], child: &[u16]| {
        child.len() >= parent.len()
            && unsafe { CompareStringOrdinal(parent, &child[..parent.len()], true) } == CSTR_EQUAL
            && (child.len() == parent.len() || child[parent.len()] == b'\\' as u16)
    };
    if contains(&left, &right) || contains(&right, &left) {
        return Err(blocked("context roots overlap"));
    }
    Ok(())
}

fn convert_metadata(path: &str, meta: Metadata, descriptor: Vec<u8>) -> io::Result<EntryMetadata> {
    let supported = FILE_ATTRIBUTE_ARCHIVE.0
        | FILE_ATTRIBUTE_DIRECTORY.0
        | FILE_ATTRIBUTE_HIDDEN.0
        | FILE_ATTRIBUTE_NORMAL.0
        | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED.0
        | FILE_ATTRIBUTE_READONLY.0
        | FILE_ATTRIBUTE_SYSTEM.0;
    if meta.attributes & !supported != 0 {
        return Err(blocked("unsupported source attributes"));
    }
    Ok(EntryMetadata {
        path: path.into(),
        kind: if meta.directory {
            EntryType::Directory
        } else {
            EntryType::File
        },
        size: if meta.directory { 0 } else { meta.size },
        object_identity: identity(&meta.identity)?,
        // files::metadata has already rejected any regular file with != 1 link.
        link_count: 1,
        permissions: PermissionRecord::Windows {
            descriptor,
            attributes: meta.attributes,
        },
    })
}
fn entry_metadata(path: &str, file: HANDLE) -> io::Result<EntryMetadata> {
    let meta = super::files::metadata(file)?;
    verify_streams(file, &meta)?;
    convert_metadata(path, meta, capture_file_descriptor(file)?)
}

/// Named NTFS streams are user data too. This supported format rejects them
/// rather than silently retaining only the unnamed stream. A bounded result or
/// any unsupported query blocks the complete capture.
pub(super) fn verify_streams(file: HANDLE, meta: &Metadata) -> io::Result<()> {
    let mut buffer = vec![0u64; 65536 / 8];
    if let Err(error) = unsafe {
        GetFileInformationByHandleEx(file, FileStreamInfo, buffer.as_mut_ptr().cast(), 65536)
    } {
        if meta.directory && error.code() == ERROR_HANDLE_EOF.to_hresult() {
            return Ok(());
        }
        return Err(win_error(error));
    }
    let header = offset_of!(FILE_STREAM_INFO, StreamName);
    let entry = unsafe { &*buffer.as_ptr().cast::<FILE_STREAM_INFO>() };
    let length = entry.StreamNameLength as usize;
    if meta.directory && length == 0 && entry.NextEntryOffset == 0 {
        return Ok(());
    }
    if length == 0 || !length.is_multiple_of(2) || header + length > 65536 {
        return Err(blocked("invalid stream name"));
    }
    let units = unsafe { std::slice::from_raw_parts(entry.StreamName.as_ptr(), length / 2) };
    if meta.directory
        || units != "::$DATA".encode_utf16().collect::<Vec<_>>()
        || entry.StreamSize < 0
        || entry.StreamSize as u64 != meta.size
        || entry.NextEntryOffset != 0
    {
        return Err(blocked("named or unsupported NTFS stream"));
    }
    Ok(())
}

/// Borrows the live Windows writer AND that root's exact exclusive lease. It
/// never accepts a deserialized receipt as a substitute for either guard.
pub(crate) struct ContextJournal<'a> {
    store: &'a mut JournalStore,
    root: Arc<PrivateDirectory>,
    lease: CopyLease<'a>,
    binding: JournalBinding,
    generation: u64,
}
#[derive(Clone, Copy)]
enum CopyLease<'a> {
    Exclusive(&'a ExclusiveLease),
    Precommit {
        control: &'a ControlLease,
        source: &'a SharedLease,
    },
}
impl CopyLease<'_> {
    fn verify(&self, root: &PrivateDirectory) -> io::Result<()> {
        match self {
            Self::Exclusive(lease) => lease.verify_root(root),
            Self::Precommit { control, source } => {
                control.verify_root(root)?;
                source.verify_root(root)
            }
        }
    }
}
struct PendingEffect {
    id: String,
    generation: u64,
}
impl<'a> ContextJournal<'a> {
    pub(crate) fn new(
        store: &'a mut JournalStore,
        root: Arc<PrivateDirectory>,
        lease: &'a ExclusiveLease,
        binding: JournalBinding,
        generation: u64,
    ) -> io::Result<Self> {
        let mut journal = Self {
            store,
            root,
            lease: CopyLease::Exclusive(lease),
            binding,
            generation,
        };
        journal.verify()?;
        Ok(journal)
    }
    /// Only independent private copies are permitted while the source retains
    /// its shared lifetime lease; every source-root effect requires exclusive.
    pub(crate) fn new_precommit(
        store: &'a mut JournalStore,
        root: Arc<PrivateDirectory>,
        control: &'a ControlLease,
        source: &'a SharedLease,
        binding: JournalBinding,
        generation: u64,
    ) -> io::Result<Self> {
        let mut journal = Self {
            store,
            root,
            lease: CopyLease::Precommit { control, source },
            binding,
            generation,
        };
        journal.verify()?;
        Ok(journal)
    }
    fn exclusive(&self) -> io::Result<&'a ExclusiveLease> {
        match self.lease {
            CopyLease::Exclusive(lease) => Ok(lease),
            CopyLease::Precommit { .. } => Err(blocked(
                "source rotation requires a new exclusive lifetime lease",
            )),
        }
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
    pub(crate) fn verify_transaction(&mut self, transaction: &str) -> io::Result<()> {
        self.verify()?;
        if self.binding.transaction_id != transaction {
            return Err(blocked("context journal belongs to another transaction"));
        }
        Ok(())
    }
    fn verify(&mut self) -> io::Result<()> {
        self.lease.verify(&self.root)?;
        self.root.verify(&CurrentUser::capture()?)?;
        safe(
            self.store
                .verify_windows_binding(&self.root, &self.binding, self.generation),
        )
    }
    fn retain<T: Serialize>(&mut self, value: &T) -> io::Result<String> {
        self.verify()?;
        let digest = safe(self.store.retain_manifest(&encoded(value)?))?;
        self.verify()?;
        Ok(digest)
    }
    fn begin(
        &mut self,
        kind: EffectKind,
        before: &impl Serialize,
        expected: &impl Serialize,
    ) -> io::Result<PendingEffect> {
        if !matches!(kind, EffectKind::PrivateBackupEntry { .. }) {
            self.exclusive()?.verify_root(&self.root)?;
        }
        let before = self.retain(before)?;
        let expected_postconditions = self.retain(expected)?;
        let id = uuid::Uuid::new_v4().to_string();
        self.verify()?;
        self.generation = safe(self.store.append(
            self.generation,
            JournalEvent::Intent {
                effect: EffectSpec {
                    effect_id: id.clone(),
                    kind,
                    before,
                    expected_postconditions,
                },
            },
        ))?;
        self.verify()?;
        Ok(PendingEffect {
            id,
            generation: self.generation,
        })
    }
    fn applied(&mut self, pending: PendingEffect, observed: &impl Serialize) -> io::Result<()> {
        let observed = self.retain(observed)?;
        self.applied_retained(pending, &observed)
    }
    fn applied_retained(&mut self, pending: PendingEffect, observed: &str) -> io::Result<()> {
        self.verify()?;
        let receipt = safe(self.store.retain_effect_receipt(
            &pending.id,
            Observation::Applied,
            observed,
        ))?;
        self.generation = safe(self.store.append(
            self.generation,
            JournalEvent::Observed {
                effect_id: pending.id,
                intent_generation: pending.generation,
                result: ObservedResult {
                    observation: Observation::Applied,
                    receipt: Some(receipt),
                },
            },
        ))?;
        self.verify()
    }
    fn unknown(&mut self, pending: PendingEffect) -> io::Result<()> {
        self.verify()?;
        self.generation = safe(self.store.append(
            self.generation,
            JournalEvent::Observed {
                effect_id: pending.id,
                intent_generation: pending.generation,
                result: ObservedResult {
                    observation: Observation::Unknown,
                    receipt: None,
                },
            },
        ))?;
        self.verify()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrivateCopyManifest {
    schema: u32,
    /// Original IDs, owner/DACL and attributes are kept separately and protected
    /// by the same private manifest persistence as the byte hashes.
    pub(crate) source: TreeManifest,
    /// Different IDs and private ACLs are expected. Never verify with exact
    /// source identity equality or broaden the backup ACL to match the source.
    pub(crate) copy: TreeManifest,
}

/// A one-attempt destination. Failure retains its private directory and every
/// successfully opened/created object in this owner; Drop never removes data.
pub(crate) struct PrivateTreeCopy {
    parent: Arc<PrivateDirectory>,
    name: ComponentName,
    tree: Option<HeldTree>,
    manifest: Option<PrivateCopyManifest>,
    attempted: bool,
    rotation_attempted: bool,
    rotation: Option<RotationTicket>,
    recovery_copy: Option<Box<PrivateTreeCopy>>,
    retained_recovery_attempts: BTreeMap<String, HeldTree>,
}
impl PrivateTreeCopy {
    /// Consume the unchanged-copy authority. Drop writable descendants, reopen
    /// the complete tree read-only and compare every original ID/byte/ACL; then
    /// separately journal one extra manager executable. Any release-gap change,
    /// collision or partial write preserves the actual complete/partial trees.
    pub(crate) fn into_manager_bundle(
        mut self,
        source_image: &PinnedFile,
        manager_name: ComponentName,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<(Arc<PrivateDirectory>, HeldTree)> {
        self.verify(user)?;
        journal.verify()?;
        if self.rotation_attempted
            || self.rotation.is_some()
            || self.recovery_copy.is_some()
            || !self.retained_recovery_attempts.is_empty()
            || text(&manager_name)? != "cc-desk-version-manager.exe"
        {
            return Err(blocked(
                "manager copy cannot reuse context recovery authority",
            ));
        }
        let prior = self.manifest()?.clone();
        let source_name = text(&source_image.name)?;
        source_image.verify()?;
        let original = prior
            .source
            .entries
            .iter()
            .find(|entry| entry.metadata.path == source_name)
            .ok_or_else(|| blocked("manager source image is outside the copied tree"))?;
        if original.metadata.kind != EntryType::File
            || original.metadata.object_identity != identity(source_image.identity())?
            || original.sha256.as_deref() != Some(source_image.digest()?.as_str())
        {
            return Err(blocked("manager source image changed"));
        }
        let tree = self
            .tree
            .take()
            .ok_or_else(|| blocked("manager base copy is absent"))?;
        let HeldRoot::Present(directory) = &tree.root else {
            return Err(blocked("manager requires a complete present bundle"));
        };
        let directory = directory.clone();
        let limits = tree.limits;
        if prior.copy.entries.len() >= limits.max_entries
            || prior
                .copy
                .entries
                .iter()
                .try_fold(original.metadata.size, |total, entry| {
                    total.checked_add(entry.metadata.size)
                })
                .is_none_or(|total| total > limits.max_bytes)
        {
            return Err(blocked("augmented manager bundle exceeds capture capacity"));
        }
        // No claim of continuous descendant sealing across this interval. The
        // root and parent remain retained, but all actual descendants must be
        // freshly observed before the private copy can authorize a launch.
        drop(tree);
        #[cfg(test)]
        if let Some(action) = MANAGER_COPY_HANDOFF_PROBE.with_borrow_mut(|pending| pending.take()) {
            action();
        }
        let readmitted = HeldTree::capture_private(directory.clone(), limits, user)?;
        if readmitted.manifest != prior.copy {
            return Err(blocked(
                "private manager copy changed during read-only handoff",
            ));
        }
        let root = Arc::new(PrivateDirectory::open_existing(
            self.parent.directory().clone(),
            self.name.clone(),
            user,
        )?);
        if root.directory().identity() != directory.identity() {
            return Err(blocked("private manager bundle root changed"));
        }
        let extra_path = text(&manager_name)?;
        let absent = HeldRoot::Absent {
            parent: directory.clone(),
            name: manager_name.clone(),
        };
        absent.verify()?;
        // This is an additional independent destination with a separate bounded
        // operation plan, not a mutation concealed by the old copy manifest.
        let source_record = TreeManifest {
            schema: 1,
            location_identity: identity(&(
                source_image.identity(),
                source_image
                    .path()?
                    .into_string()
                    .map_err(|_| blocked("unrepresentable manager image location"))?,
            ))?,
            entries: vec![original.clone()],
        };
        let (plan_generation, manifest) = safe(journal.store.plan_private_backup(
            journal.generation,
            1,
            &source_record.encode()?,
        ))?;
        journal.generation = plan_generation;
        journal.verify()?;
        let destination = (directory.identity().clone(), extra_path.clone());
        let pending = journal.begin(
            EffectKind::PrivateBackupEntry {
                plan_generation,
                operation: PrivateBackupOperation::CopyFile,
                manifest,
                entry_index: 0,
            },
            &("absent-manager-image", &destination),
            &("independent-manager-image", &destination, original),
        )?;
        let outcome = (|| -> io::Result<HeldTree> {
            readmitted.verify()?;
            source_image.verify()?;
            let descriptor = user.descriptor(false)?;
            let file = directory.open_relative(
                &manager_name,
                FILE_READ_DATA
                    | FILE_WRITE_DATA
                    | FILE_READ_ATTRIBUTES
                    | READ_CONTROL
                    | SYNCHRONIZE,
                FILE_SHARE_READ,
                FILE_CREATE,
                false,
                Some(&descriptor),
            )?;
            user.verify_private_file(handle(&file), false)?;
            let file = PinnedFile::from_file(directory.clone(), manager_name.clone(), file)?;
            let mut source = &source_image.file;
            source.seek(SeekFrom::Start(0))?;
            let mut writer = &file.file;
            let copied = io::copy(
                &mut source.take(original.metadata.size.saturating_add(1)),
                &mut writer,
            )?;
            if copied != original.metadata.size {
                return Err(blocked("manager image copy length differs"));
            }
            copy_fault(CopyFault::AfterWrite)?;
            unsafe { FlushFileBuffers(handle(&file.file)) }.map_err(win_error)?;
            copy_fault(CopyFault::AfterFlush)?;
            let extra = ManifestEntry {
                metadata: entry_metadata(&extra_path, handle(&file.file))?,
                sha256: Some(file.digest()?),
            };
            if extra.metadata.kind != EntryType::File
                || extra.metadata.size != original.metadata.size
                || extra.sha256 != original.sha256
                || prior
                    .source
                    .entries
                    .iter()
                    .chain(&prior.copy.entries)
                    .any(|entry| entry.metadata.object_identity == extra.metadata.object_identity)
            {
                return Err(blocked("manager image is not an independent exact copy"));
            }
            // Release only the new writer, then compare its exact object through
            // a complete read-only admission before exposing the launch root.
            drop(file);
            let complete = HeldTree::capture_private(directory.clone(), limits, user)?;
            let mut expected = prior.copy.clone();
            expected.entries.push(extra);
            expected
                .entries
                .sort_by(|a, b| a.metadata.path.cmp(&b.metadata.path));
            if complete.manifest != expected {
                return Err(blocked("complete augmented manager bundle differs"));
            }
            source_image.verify()?;
            if original.sha256.as_deref() != Some(source_image.digest()?.as_str()) {
                return Err(blocked("manager source changed during copy"));
            }
            copy_fault(CopyFault::BeforeReceipt)?;
            Ok(complete)
        })();
        match outcome {
            Ok(complete) => {
                journal.applied(pending, complete.manifest())?;
                root.verify(user)?;
                complete.verify()?;
                // Retain the complete read-only admission through transfer.
                // The consumed original PrivateTreeCopy no longer exists.
                Ok((root, complete))
            }
            Err(error) => {
                let _ = journal.unknown(pending);
                Err(error)
            }
        }
    }
    pub(crate) fn new(parent: Arc<PrivateDirectory>, name: ComponentName) -> Self {
        Self {
            parent,
            name,
            tree: None,
            manifest: None,
            attempted: false,
            rotation_attempted: false,
            rotation: None,
            recovery_copy: None,
            retained_recovery_attempts: BTreeMap::new(),
        }
    }
    pub(crate) fn manifest(&self) -> io::Result<&PrivateCopyManifest> {
        self.manifest
            .as_ref()
            .ok_or_else(|| blocked("private copy is incomplete"))
    }
    pub(crate) fn copy_from(
        &mut self,
        source: &HeldTree,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<()> {
        self.copy_from_plan(source, user, journal, None)
    }
    fn copy_from_plan(
        &mut self,
        source: &HeldTree,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
        admitted: Option<(u64, String)>,
    ) -> io::Result<()> {
        if self.attempted {
            return Err(blocked("copy retry requires reconciliation"));
        }
        self.attempted = true;
        source.verify()?;
        self.parent.verify(user)?;
        if let HeldRoot::Present(root) = &source.root {
            root.require_disjoint(&[self.parent.directory().clone()])?;
        }
        // Retain the complete source before any destination effect. This is an
        // unassigned artifact, not a premature SourceContext binding.
        let source_bytes = source.manifest.encode()?;
        // A private descriptor has at most owner/group SIDs and one simple ACE;
        // <=1024 additional serialized bytes per entry bounds source-to-private
        // descriptor growth. Reject wrapper overflow before creating anything.
        if source_bytes
            .len()
            .checked_mul(2)
            .and_then(|size| {
                size.checked_add(source.manifest.entries.len().saturating_mul(1024) + 4096)
            })
            .is_none_or(|size| size > MAX_MANIFEST_BYTES)
        {
            return Err(blocked("copy manifest wrapper exceeds capacity"));
        }
        journal.verify()?;
        let (generation, source_digest) = if let Some((generation, digest)) = admitted {
            if journal.generation != generation || digest != source.manifest.digest()? {
                return Err(blocked("recovery copy admission differs"));
            }
            (generation, digest)
        } else {
            safe(journal.store.plan_private_backup(
                journal.generation,
                source.manifest.entries.len(),
                &source_bytes,
            ))?
        };
        journal.generation = generation;
        journal.verify()?;
        let root = HeldRoot::Absent {
            parent: self.parent.directory().clone(),
            name: self.name.clone(),
        };
        root.verify()?;
        self.tree = Some(HeldTree {
            manifest: TreeManifest {
                schema: 1,
                location_identity: root.location_identity()?,
                entries: vec![],
            },
            root,
            entries: BTreeMap::new(),
            limits: source.limits,
            detached_image: None,
            fenced_location: None,
        });
        for (index, entry) in source.manifest.entries.iter().enumerate() {
            source.root.verify()?;
            self.parent.verify(user)?;
            let (parent, name) = if entry.metadata.path.is_empty() {
                (self.parent.directory().clone(), self.name.clone())
            } else {
                let (parent_path, leaf) = entry
                    .metadata
                    .path
                    .rsplit_once('/')
                    .unwrap_or(("", &entry.metadata.path));
                let Some(HeldEntry::Directory(parent)) = self
                    .tree
                    .as_ref()
                    .and_then(|tree| tree.entries.get(parent_path))
                else {
                    return Err(blocked("copy parent is not retained"));
                };
                (parent.clone(), ComponentName::new(OsStr::new(leaf))?)
            };
            let absent = HeldRoot::Absent {
                parent: parent.clone(),
                name: name.clone(),
            };
            absent.verify()?;
            let destination = (parent.identity().clone(), text(&name)?);
            let operation = if entry.metadata.kind == EntryType::Directory {
                PrivateBackupOperation::CreateDirectory
            } else {
                PrivateBackupOperation::CopyFile
            };
            let pending = journal.begin(
                EffectKind::PrivateBackupEntry {
                    plan_generation: generation,
                    operation,
                    manifest: source_digest.clone(),
                    entry_index: index as u32,
                },
                &("absent", &destination, &entry.metadata.object_identity),
                &("private-independent-copy", &destination, entry),
            )?;
            // Any error after this boundary leaves this exact intent unresolved.
            let result = copy_fault(CopyFault::BeforeCreate)
                .and_then(|()| self.copy_entry(source, entry, parent, name, user));
            match result {
                Ok(observed) => {
                    journal.applied(pending, &observed)?;
                    self.parent.verify(user)?;
                }
                Err(error) => {
                    let _ = journal.unknown(pending);
                    return Err(error);
                }
            }
        }
        let tree = self.tree.as_mut().expect("copy tree initialized");
        tree.manifest
            .entries
            .sort_by(|a, b| a.metadata.path.cmp(&b.metadata.path));
        tree.manifest.location_identity = tree.root.location_identity()?;
        tree.verify()?;
        source.verify()?;
        let manifest = PrivateCopyManifest {
            schema: 1,
            source: source.manifest.clone(),
            copy: tree.manifest.clone(),
        };
        verify_copy_mapping(&manifest)?;
        journal.retain(&manifest)?;
        self.manifest = Some(manifest);
        self.verify(user)?;
        Ok(())
    }
    fn copy_entry(
        &mut self,
        source: &HeldTree,
        entry: &ManifestEntry,
        parent: Arc<Directory>,
        name: ComponentName,
        user: &CurrentUser,
    ) -> io::Result<ManifestEntry> {
        let path = &entry.metadata.path;
        if entry.metadata.kind == EntryType::Directory {
            let private = PrivateDirectory::create_new(parent, name, user)?;
            let directory = private.directory().clone();
            let tree = self.tree.as_mut().expect("copy tree initialized");
            if path.is_empty() {
                tree.root = HeldRoot::Present(directory.clone());
            }
            tree.entries
                .insert(path.clone(), HeldEntry::Directory(directory));
        } else {
            let descriptor = user.descriptor(false)?;
            let file = parent.open_relative(
                &name,
                FILE_READ_DATA
                    | FILE_WRITE_DATA
                    | FILE_READ_ATTRIBUTES
                    | READ_CONTROL
                    | SYNCHRONIZE,
                FILE_SHARE_READ,
                FILE_CREATE,
                false,
                Some(&descriptor),
            )?;
            user.verify_private_file(handle(&file), false)?;
            let held = Arc::new(Mutex::new(PinnedFile::from_file(parent, name, file)?));
            // Keep the created object even if a later write/flush/readback fails.
            self.tree
                .as_mut()
                .expect("copy tree initialized")
                .entries
                .insert(
                    path.clone(),
                    HeldEntry::File(FileGuard::Ordinary(held.clone())),
                );
            let mut reader = source.reader(&entry.metadata)?;
            let file = held.lock();
            let mut destination = &file.file;
            let mut bytes = [0u8; 65536];
            loop {
                let count = reader.read(&mut bytes)?;
                if count == 0 {
                    break;
                }
                destination.write_all(&bytes[..count])?;
                copy_fault(CopyFault::AfterWrite)?;
            }
            unsafe {
                FlushFileBuffers(handle(&file.file)).map_err(win_error)?;
            }
            copy_fault(CopyFault::AfterFlush)?;
            user.verify_private_file(handle(&file.file), false)?;
            if Some(file.digest()?) != entry.sha256 {
                return Err(blocked("private copied bytes differ"));
            }
        }
        let tree = self.tree.as_mut().expect("copy tree initialized");
        let held = &tree.entries[path];
        let metadata = held.metadata(path)?;
        let digest = match held {
            HeldEntry::File(file) => Some(file.digest()?),
            HeldEntry::Directory(_) => None,
        };
        let observed = ManifestEntry {
            metadata,
            sha256: digest,
        };
        if observed.metadata.object_identity == entry.metadata.object_identity
            || observed.metadata.kind != entry.metadata.kind
            || observed.metadata.size != entry.metadata.size
            || observed.sha256 != entry.sha256
        {
            return Err(blocked("private copy is not independent and complete"));
        }
        tree.manifest.entries.push(observed.clone());
        copy_fault(CopyFault::BeforeReceipt)?;
        Ok(observed)
    }
    pub(crate) fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        let expected = self.manifest()?;
        self.parent.verify(user)?;
        let tree = self
            .tree
            .as_ref()
            .ok_or_else(|| blocked("missing private copy guards"))?;
        tree.verify()?;
        if tree.manifest != expected.copy {
            return Err(blocked("private copy manifest changed"));
        }
        for entry in tree.entries.values() {
            match entry {
                HeldEntry::Directory(root) => user.verify_private_file(root.raw(), true)?,
                HeldEntry::File(FileGuard::Ordinary(file)) => {
                    user.verify_private_file(handle(&file.lock().file), false)?
                }
                HeldEntry::File(FileGuard::Fenced(_)) => {
                    return Err(blocked("source image cannot be a backup object"))
                }
            }
        }
        verify_copy_mapping(expected)
    }
}
fn verify_copy_mapping(manifest: &PrivateCopyManifest) -> io::Result<()> {
    if manifest.schema != 1 || manifest.source.entries.len() != manifest.copy.entries.len() {
        return Err(blocked("private copy entry count differs"));
    }
    let sources: BTreeSet<_> = manifest
        .source
        .entries
        .iter()
        .map(|entry| &entry.metadata.object_identity)
        .collect();
    let mut copies = BTreeSet::new();
    for (source, copy) in manifest.source.entries.iter().zip(&manifest.copy.entries) {
        if source.metadata.path != copy.metadata.path
            || source.metadata.kind != copy.metadata.kind
            || source.metadata.size != copy.metadata.size
            || source.sha256 != copy.sha256
            || sources.contains(&copy.metadata.object_identity)
            || !copies.insert(&copy.metadata.object_identity)
        {
            return Err(blocked("private source-to-copy mapping differs"));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyFault {
    BeforeCreate,
    AfterWrite,
    AfterFlush,
    BeforeReceipt,
    AfterReverseMove,
}

#[cfg(test)]
thread_local! {
    static MANAGER_COPY_HANDOFF_PROBE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(crate) struct ManagerCopyHandoffProbe(std::marker::PhantomData<std::rc::Rc<()>>);
#[cfg(test)]
impl Drop for ManagerCopyHandoffProbe {
    fn drop(&mut self) {
        MANAGER_COPY_HANDOFF_PROBE.with_borrow_mut(|pending| {
            pending.take();
        });
    }
}
#[cfg(test)]
pub(crate) fn probe_manager_copy_handoff(
    action: impl FnOnce() + 'static,
) -> ManagerCopyHandoffProbe {
    MANAGER_COPY_HANDOFF_PROBE.with_borrow_mut(|pending| {
        assert!(pending.is_none());
        *pending = Some(Box::new(action));
    });
    ManagerCopyHandoffProbe(std::marker::PhantomData)
}
#[cfg(test)]
thread_local! { static COPY_FAULT: std::cell::Cell<Option<CopyFault>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(crate) struct CopyProbeGuard(std::marker::PhantomData<std::rc::Rc<()>>);
#[cfg(test)]
impl Drop for CopyProbeGuard {
    fn drop(&mut self) {
        COPY_FAULT.set(None);
    }
}
#[cfg(test)]
pub(crate) fn probe_copy_failure(fault: CopyFault) -> CopyProbeGuard {
    COPY_FAULT.with(|pending| {
        assert!(pending.get().is_none());
        pending.set(Some(fault));
    });
    CopyProbeGuard(std::marker::PhantomData)
}
fn copy_fault(fault: CopyFault) -> io::Result<()> {
    #[cfg(test)]
    if COPY_FAULT.with(|pending| {
        if pending.get() == Some(fault) {
            pending.set(None);
            true
        } else {
            false
        }
    }) {
        return Err(io::Error::other("injected copy boundary failure"));
    }
    let _ = fault;
    Ok(())
}

/// Same-object, equal-state re-admission after the explicitly recorded gap in
/// descendant guards. This is NOT a complete SnapshotBoundary or SourceContext
/// binding: the coordinator must combine both roots and its other live proofs.
pub(crate) struct ReadmittedRoot {
    root: RootKind,
    before: TreeManifest,
    after: TreeManifest,
    copy: PrivateCopyManifest,
    receipt_manifest: String,
    live_location: HeldRoot,
}
impl ReadmittedRoot {
    pub(crate) fn root(&self) -> RootKind {
        self.root
    }
    pub(crate) fn receipt_manifest(&self) -> &str {
        &self.receipt_manifest
    }
    pub(crate) fn verify(
        &self,
        context: &HeldContext,
        copy: &PrivateTreeCopy,
        user: &CurrentUser,
    ) -> io::Result<()> {
        context.tree(self.root).verify()?;
        self.live_location.verify()?;
        copy.verify(user)?;
        if context.tree(self.root).manifest != self.after
            || copy.manifest()? != &self.copy
            || self.before.entries != self.after.entries
        {
            return Err(blocked("root re-admission no longer matches"));
        }
        Ok(())
    }
}
impl PrivateTreeCopy {
    /// The coordinator supplies the original live parent/name and a separate
    /// retained private quarantine parent. No fresh root is created here.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rotate_context_root(
        &mut self,
        context: &mut HeldContext,
        kind: RootKind,
        original_parent: Arc<Directory>,
        original_name: ComponentName,
        quarantine: Arc<PrivateDirectory>,
        quarantine_name: ComponentName,
        boundary: &SnapshotBoundary,
        fence: &ImageFence,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<ReadmittedRoot> {
        journal.exclusive()?.verify_root(&journal.root)?;
        if self.rotation_attempted {
            return Err(blocked("rotation retry requires reconciliation"));
        }
        context.verify()?;
        self.verify(user)?;
        fence.verify()?;
        journal.verify()?;
        quarantine.verify(user)?;
        let source = context
            .trees
            .get_mut(&kind)
            .ok_or_else(|| blocked("foreign rotation root"))?;
        let before = source.manifest.clone();
        if boundary.binding() != &journal.binding
            || boundary.root_identity(kind) != Some(before.location_identity.as_str())
            || self.manifest()?.source != before
        {
            return Err(blocked("rotation source boundary differs"));
        }
        let HeldRoot::Present(root) = &source.root else {
            return Err(blocked("absent roots require no rotation"));
        };
        let root = root.clone();
        root.require_renameable()?;
        original_parent.recheck()?;
        let actual = root
            .path()?
            .into_string()
            .map_err(|_| blocked("invalid original root name"))?;
        let parent = original_parent
            .path()?
            .into_string()
            .map_err(|_| blocked("invalid original parent name"))?;
        if actual
            != format!(
                "{}\\{}",
                parent.trim_end_matches('\\'),
                text(&original_name)?
            )
        {
            return Err(blocked("original root parent/name differs"));
        }
        root.require_disjoint(&[
            quarantine.directory().clone(),
            self.parent.directory().clone(),
        ])?;
        let destination = HeldRoot::Absent {
            parent: quarantine.directory().clone(),
            name: quarantine_name.clone(),
        };
        destination.verify()?;
        // Whole-tree retention needs confidentiality in addition to a private
        // copy. A parent ACL alone does not constrain bypass-traverse access.
        verify_confidential_tree(source, user)?;
        let ready = journal.retain(self.manifest()?)?;
        let before_digest = journal.retain(&before)?;
        let plan = RotationPlan {
            schema: 1,
            root: kind,
            source_manifest: before_digest.clone(),
            copy_manifest: ready.clone(),
            original: RootSlotRecord {
                parent: original_parent.identity().clone(),
                name: text(&original_name)?,
            },
            quarantine: RootSlotRecord {
                parent: quarantine.directory().identity().clone(),
                name: text(&quarantine_name)?,
            },
            copy: CopyReference {
                parent: self.parent.directory().identity().clone(),
                name: text(&self.name)?,
                manifest: ready.clone(),
            },
        };
        let expected = (
            "release-descendant-guards-then-rotate-and-readmit",
            &ready,
            root.identity(),
            quarantine.directory().identity(),
            text(&quarantine_name)?,
            "all-relative-names-identities-bytes-permissions-must-match",
        );
        let pending = journal.begin(
            EffectKind::RotateSourceRoot {
                root: kind,
                manifest: ready.clone(),
            },
            &plan,
            &expected,
        )?;
        self.rotation_attempted = true;
        self.rotation = Some(RotationTicket {
            effect_id: pending.id.clone(),
            intent_generation: pending.generation,
            root: kind,
            original_parent: original_parent.clone(),
            original_name: original_name.clone(),
            quarantine: quarantine.clone(),
            quarantine_name: quarantine_name.clone(),
            reverse: None,
            admitted_manifest: None,
        });
        // Recheck everything after intent persistence and immediately before
        // invalidating only descendant guards. Keep root DELETE guard, ancestors,
        // lease, image fence, M0, C0 and every immutable journal dependency.
        source.verify()?;
        self.verify(user)?;
        fence.verify()?;
        journal.verify()?;
        source.entries.clear();
        #[cfg(test)]
        if let Some(action) = ROTATION_PROBE.with_borrow_mut(|pending| pending.take()) {
            action();
        }
        let result = (|| {
            fence.verify()?;
            journal.verify()?;
            root.rename_to(quarantine.directory().clone(), quarantine_name)?;
            #[cfg(test)]
            if let Some(action) = ROTATION_AFTER_MOVE.with_borrow_mut(|pending| pending.take()) {
                action();
            }
            let admitted = HeldTree::admit(
                HeldRoot::Present(root.clone()),
                &mut Budget::new(source.limits)?,
                None,
            )?;
            let after = admitted.manifest.clone();
            // Keep the actual rotated tree even on content/permission drift.
            *source = admitted;
            if before.entries != after.entries {
                return Err(blocked(
                    "source changed while descendant guards were released",
                ));
            }
            let fresh_location = HeldRoot::Absent {
                parent: original_parent,
                name: original_name,
            };
            fresh_location.verify()?;
            self.verify(user)?;
            fence.verify()?;
            journal.verify()?;
            let after_digest = journal.retain(&after)?;
            let observation = (
                &ready,
                &before_digest,
                &after_digest,
                "same-object-complete-equal-state-readmission",
            );
            let receipt_manifest = journal.retain(&observation)?;
            let proof = ReadmittedRoot {
                root: kind,
                before,
                after,
                copy: self.manifest()?.clone(),
                receipt_manifest,
                live_location: fresh_location,
            };
            Ok(proof)
        })();
        match result {
            Ok(proof) => {
                journal.applied_retained(pending, &proof.receipt_manifest)?;
                self.verify(user)?;
                fence.verify()?;
                Ok(proof)
            }
            Err(error) => {
                // No replay, no path-based repair and no deletion. Even an
                // uncertain rename keeps the same root object and complete C0.
                let _ = journal.unknown(pending);
                Err(error)
            }
        }
    }
}
fn verify_confidential_tree(tree: &HeldTree, user: &CurrentUser) -> io::Result<()> {
    for entry in tree.entries.values() {
        match entry {
            HeldEntry::Directory(directory) => {
                user.verify_confidential_source(directory.raw(), true)?
            }
            HeldEntry::File(FileGuard::Ordinary(file)) => {
                user.verify_confidential_source(handle(&file.lock().file), false)?
            }
            HeldEntry::File(FileGuard::Fenced(_)) => {
                return Err(blocked("context cannot contain fenced application image"))
            }
        }
    }
    Ok(())
}
#[cfg(test)]
thread_local! {
    static ROTATION_PROBE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static ROTATION_AFTER_MOVE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(crate) struct RotationProbeGuard(std::marker::PhantomData<std::rc::Rc<()>>);
#[cfg(test)]
impl Drop for RotationProbeGuard {
    fn drop(&mut self) {
        ROTATION_PROBE.with_borrow_mut(|pending| {
            pending.take();
        });
        ROTATION_AFTER_MOVE.with_borrow_mut(|pending| {
            pending.take();
        });
    }
}
#[cfg(test)]
pub(crate) fn probe_after_root_rotation(action: impl FnOnce() + 'static) -> RotationProbeGuard {
    ROTATION_AFTER_MOVE.with_borrow_mut(|pending| {
        assert!(pending.is_none());
        *pending = Some(Box::new(action));
    });
    RotationProbeGuard(std::marker::PhantomData)
}
#[cfg(test)]
pub(crate) fn probe_after_guard_release(action: impl FnOnce() + 'static) -> RotationProbeGuard {
    ROTATION_PROBE.with_borrow_mut(|pending| {
        assert!(pending.is_none());
        *pending = Some(Box::new(action));
    });
    RotationProbeGuard(std::marker::PhantomData)
}

struct RotationTicket {
    effect_id: String,
    intent_generation: u64,
    root: RootKind,
    original_parent: Arc<Directory>,
    original_name: ComponentName,
    quarantine: Arc<PrivateDirectory>,
    quarantine_name: ComponentName,
    reverse: Option<PendingEffect>,
    admitted_manifest: Option<String>,
}

/// Only the live context executor constructs this borrowed capability. The
/// journal rechecks it before and after retaining its anchored receipt.
pub(crate) struct RootRecoveryEvidence<'a> {
    copy: &'a PrivateTreeCopy,
    context: &'a HeldContext,
    boundary: &'a SnapshotBoundary,
    fence: &'a ImageFence,
    user: &'a CurrentUser,
    records: &'a PrivateDirectory,
    lease: &'a ExclusiveLease,
    binding: &'a JournalBinding,
    generation: u64,
    returned: bool,
}
pub(crate) struct RootRecoveryRequest {
    pub(crate) effect_id: String,
    pub(crate) intent_generation: u64,
    pub(crate) generation: u64,
    pub(crate) current: Vec<u8>,
    pub(crate) returned: bool,
}
impl RootRecoveryEvidence<'_> {
    pub(crate) fn verify(&self, store: &mut JournalStore) -> io::Result<RootRecoveryRequest> {
        self.lease.verify_root(self.records)?;
        self.fence.verify()?;
        self.copy.verify(self.user)?;
        if self.returned {
            self.copy
                .recovery_copy
                .as_ref()
                .ok_or_else(|| blocked("missing retained pre-reverse data"))?
                .verify(self.user)?;
        }
        safe(store.verify_windows_binding(self.records, self.binding, self.generation))?;
        let ticket = self
            .copy
            .rotation
            .as_ref()
            .ok_or_else(|| blocked("missing live rotation ticket"))?;
        let source = self.context.tree(ticket.root);
        source.verify()?;
        verify_confidential_tree(source, self.user)?;
        let original = &self.copy.manifest()?.source;
        if self.boundary.binding() != self.binding
            || self.boundary.root_identity(ticket.root) != Some(original.location_identity.as_str())
            || original
                .entries
                .first()
                .map(|entry| &entry.metadata.object_identity)
                != source
                    .manifest
                    .entries
                    .first()
                    .map(|entry| &entry.metadata.object_identity)
        {
            return Err(blocked("root recovery identity or source boundary differs"));
        }
        let (spec, intent_generation) = safe(store.context_rotation(&ticket.effect_id))?;
        if spec.kind
            != (EffectKind::RotateSourceRoot {
                root: ticket.root,
                manifest: identity(self.copy.manifest()?)?,
            })
            || intent_generation != ticket.intent_generation
        {
            return Err(blocked("root recovery journal effect differs"));
        }
        let HeldRoot::Present(root) = &source.root else {
            return Err(blocked("recovery root is absent"));
        };
        let at_original = occupies(root, &ticket.original_parent, &ticket.original_name)?;
        let at_quarantine = occupies(root, ticket.quarantine.directory(), &ticket.quarantine_name)?;
        if (!at_original && !at_quarantine) || (self.returned && !at_original) {
            return Err(blocked(
                "owned recovery root occupies an unsupported location",
            ));
        }
        let (effect_id, intent_generation) = if self.returned {
            let reverse = ticket
                .reverse
                .as_ref()
                .ok_or_else(|| blocked("missing inverse intent"))?;
            let (effect, generation) = safe(store.context_rotation(&reverse.id))?;
            if generation != reverse.generation
                || !matches!(&effect.kind,
                EffectKind::ReverseSourceRoot { original_effect_id, original_intent_generation, current_manifest }
                if original_effect_id == &ticket.effect_id && *original_intent_generation == ticket.intent_generation
                    && ticket.admitted_manifest.as_ref() == Some(current_manifest))
            {
                return Err(blocked("inverse intent differs"));
            }
            (reverse.id.clone(), reverse.generation)
        } else {
            (ticket.effect_id.clone(), ticket.intent_generation)
        };
        Ok(RootRecoveryRequest {
            effect_id,
            intent_generation,
            generation: self.generation,
            current: source.manifest.encode()?,
            returned: self.returned,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryDestination {
    parent: FileIdentity,
    name: String,
}
/// Re-observation of a previous C1 attempt plus an absent, independent next
/// destination. A decoded plan alone cannot produce this live capability.
pub(crate) struct RootBackupEvidence<'a> {
    recovery: RootRecoveryEvidence<'a>,
    destination: &'a HeldRoot,
    previous: Option<&'a HeldTree>,
}
pub(crate) struct RootBackupRequest {
    pub(crate) effect_id: String,
    pub(crate) intent_generation: u64,
    pub(crate) generation: u64,
    pub(crate) current: Vec<u8>,
    pub(crate) destination: Vec<u8>,
    pub(crate) preserved: Option<Vec<u8>>,
    pub(crate) prior_plan_generation: Option<u64>,
    pub(crate) abandoned_effect: Option<(String, u64)>,
    pub(crate) reservation_source: String,
    pub(crate) effects: usize,
}
impl RootBackupEvidence<'_> {
    pub(crate) fn verify(&self, store: &mut JournalStore) -> io::Result<RootBackupRequest> {
        let current = self.recovery.verify(store)?;
        let ticket = self
            .recovery
            .copy
            .rotation
            .as_ref()
            .expect("recovery verified ticket");
        if self.recovery.returned || ticket.reverse.is_some() {
            return Err(blocked("C1 cannot change after inverse intent"));
        }
        let source = self.recovery.context.tree(ticket.root);
        if current
            .current
            .len()
            .checked_mul(2)
            .and_then(|size| {
                size.checked_add(source.manifest.entries.len().saturating_mul(1024) + 4096)
            })
            .is_none_or(|size| size > MAX_MANIFEST_BYTES)
        {
            return Err(blocked("copy manifest wrapper exceeds capacity"));
        }
        self.destination.verify()?;
        require_distinct_roots(&source.root, self.destination)?;
        let HeldRoot::Absent { parent, name } = self.destination else {
            return Err(blocked("new C1 destination is not absent"));
        };
        if parent.identity() != self.recovery.copy.parent.directory().identity() {
            return Err(blocked("new C1 parent differs"));
        }
        self.recovery.copy.parent.verify(self.recovery.user)?;
        let prior = safe(store.context_root_backup(&ticket.effect_id))?;
        let preserved = match (&prior, self.previous) {
            (None, None) => None,
            (Some((_, plan)), Some(previous)) => {
                previous.verify()?;
                verify_private_tree(previous, self.recovery.user)?;
                let recorded: RecoveryDestination =
                    serde_json::from_slice(&safe(store.read_manifest(&plan.destination))?)?;
                let old_name = ComponentName::new(OsStr::new(&recorded.name))?;
                if recorded.parent != *parent.identity() || same_component(name, &old_name) {
                    return Err(blocked("C1 destination aliases prior attempt"));
                }
                let matches = match &previous.root {
                    HeldRoot::Present(root) => occupies(root, parent, &old_name)?,
                    HeldRoot::Absent {
                        parent: old_parent,
                        name: absent_name,
                    } => {
                        old_parent.identity() == parent.identity()
                            && same_component(absent_name, &old_name)
                    }
                };
                if !matches {
                    return Err(blocked(
                        "prior C1 observation differs from recorded destination",
                    ));
                }
                Some(previous.manifest.encode()?)
            }
            _ => return Err(blocked("missing previous C1 observation")),
        };
        let abandoned_effect = if let Some((effect, generation)) = safe(store.context_pending())? {
            if !matches!(&effect.kind, EffectKind::PrivateBackupEntry { plan_generation, manifest, .. }
                if prior.as_ref().is_some_and(|(prior_generation, plan)| prior_generation == plan_generation && &plan.current_manifest == manifest))
            {
                return Err(blocked("pending effect is not this root's C1 attempt"));
            }
            Some((effect.effect_id, generation))
        } else {
            None
        };
        Ok(RootBackupRequest {
            effect_id: current.effect_id,
            intent_generation: current.intent_generation,
            generation: current.generation,
            current: current.current,
            destination: encoded(&RecoveryDestination {
                parent: parent.identity().clone(),
                name: text(name)?,
            })?,
            preserved,
            prior_plan_generation: prior.map(|(generation, _)| generation),
            abandoned_effect,
            reservation_source: self.recovery.copy.manifest()?.source.digest()?,
            effects: source.manifest.entries.len(),
        })
    }
}
fn verify_private_tree(tree: &HeldTree, user: &CurrentUser) -> io::Result<()> {
    tree.verify()?;
    for entry in tree.entries.values() {
        match entry {
            HeldEntry::Directory(root) => user.verify_private_file(root.raw(), true)?,
            HeldEntry::File(FileGuard::Ordinary(file)) => {
                user.verify_private_file(handle(&file.lock().file), false)?
            }
            HeldEntry::File(FileGuard::Fenced(_)) => {
                return Err(blocked("fenced image is not a private C1 object"))
            }
        }
    }
    Ok(())
}
fn occupies(root: &Directory, parent: &Directory, name: &ComponentName) -> io::Result<bool> {
    parent.recheck()?;
    let (_, actual) = root.observe_location()?;
    let expected = format!(
        "{}\\{}",
        parent
            .path()?
            .into_string()
            .map_err(|_| blocked("invalid root parent"))?
            .trim_end_matches('\\'),
        text(name)?
    );
    Ok(actual.to_str() == Some(expected.as_str()))
}

/// Returned source root observation, with original and intervening bytes kept.
/// It still cannot terminate a transaction without the complete abort factory's
/// bundle, registration, both-root and quiescence proofs.
pub(crate) struct ReturnedRoot {
    root: RootKind,
    observed: TreeManifest,
}
impl ReturnedRoot {
    pub(crate) fn verify(&self, context: &HeldContext) -> io::Result<()> {
        context.tree(self.root).verify()?;
        if context.tree(self.root).manifest != self.observed {
            return Err(blocked("returned root changed"));
        }
        Ok(())
    }
}
impl PrivateTreeCopy {
    /// Reopen only an expected private copy below backend-held authority. A
    /// manifest does not supply an absolute path or broaden that authority.
    pub(crate) fn reopen(
        parent: Arc<PrivateDirectory>,
        name: ComponentName,
        expected: PrivateCopyManifest,
        user: &CurrentUser,
        limits: SnapshotLimits,
    ) -> io::Result<Self> {
        parent.verify(user)?;
        let root = if expected.copy.entries.is_empty() {
            HeldRoot::Absent {
                parent: parent.directory().clone(),
                name: name.clone(),
            }
        } else {
            HeldRoot::Present(
                PrivateDirectory::open_existing(parent.directory().clone(), name.clone(), user)?
                    .directory()
                    .clone(),
            )
        };
        let tree = HeldTree::admit(root, &mut Budget::new(limits)?, None)?;
        let copy = Self {
            parent,
            name,
            tree: Some(tree),
            manifest: Some(expected),
            attempted: true,
            rotation_attempted: false,
            rotation: None,
            recovery_copy: None,
            retained_recovery_attempts: BTreeMap::new(),
        };
        copy.verify(user)?;
        Ok(copy)
    }
    fn observe_previous_recovery(
        &mut self,
        plan: &RootBackupPlan,
        user: &CurrentUser,
        limits: SnapshotLimits,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<HeldTree> {
        let recorded: RecoveryDestination =
            serde_json::from_slice(&safe(journal.store.read_manifest(&plan.destination))?)?;
        if recorded.parent != *self.parent.directory().identity() {
            return Err(blocked("prior C1 parent differs"));
        }
        let name = ComponentName::new(OsStr::new(&recorded.name))?;
        let held_root = if let Some(mut old) = self.recovery_copy.take() {
            if !same_component(&old.name, &name)
                || old.parent.directory().identity() != self.parent.directory().identity()
            {
                self.recovery_copy = Some(old);
                return Err(blocked("held C1 destination differs"));
            }
            let root = old.tree.as_ref().map(|tree| tree.root.clone());
            if let Some(tree) = old.tree.as_mut() {
                tree.entries.clear();
            }
            drop(old);
            root
        } else {
            None
        };
        let root = match held_root {
            Some(HeldRoot::Present(root)) => HeldRoot::Present(root),
            _ => HeldRoot::observe(self.parent.directory().clone(), name)?,
        };
        let observed = HeldTree::admit(root, &mut Budget::new(limits)?, None)?;
        verify_private_tree(&observed, user)?;
        journal.verify()?;
        Ok(observed)
    }
    /// After a failed rotation re-observe the same owned root, retain a complete
    /// independent copy of its ACTUAL current contents, then reverse at most once.
    /// No data is restored from C0 and an occupied original slot always blocks.
    pub(crate) fn reverse_context_root(
        &mut self,
        context: &mut HeldContext,
        boundary: &SnapshotBoundary,
        fence: &ImageFence,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<ReturnedRoot> {
        journal.exclusive()?.verify_root(&journal.root)?;
        self.verify(user)?;
        fence.verify()?;
        journal.verify()?;
        let ticket = self
            .rotation
            .as_ref()
            .ok_or_else(|| blocked("missing rotation ownership"))?;
        let kind = ticket.root;
        let original_parent = ticket.original_parent.clone();
        let original_name = ticket.original_name.clone();
        let quarantine = ticket.quarantine.clone();
        let quarantine_name = ticket.quarantine_name.clone();
        let source = context
            .trees
            .get_mut(&kind)
            .ok_or_else(|| blocked("missing owned source root"))?;
        let HeldRoot::Present(root) = &source.root else {
            return Err(blocked("owned source root is absent"));
        };
        let root = root.clone();
        let at_original = occupies(&root, &original_parent, &original_name)?;
        if at_original {
            root.reconcile_location(original_parent.clone(), original_name.clone())?;
        } else if occupies(&root, quarantine.directory(), &quarantine_name)? {
            root.reconcile_location(quarantine.directory().clone(), quarantine_name)?;
        } else {
            return Err(blocked("uncertain root is outside recorded locations"));
        }
        *source = HeldTree::admit(
            HeldRoot::Present(root.clone()),
            &mut Budget::new(source.limits)?,
            None,
        )?;
        let evidence = RootRecoveryEvidence {
            copy: self,
            context,
            boundary,
            fence,
            user,
            records: &journal.root,
            lease: journal.exclusive()?,
            binding: &journal.binding,
            generation: journal.generation,
            returned: ticket.reverse.is_some(),
        };
        if evidence.returned {
            // An inverse may already have moved the object before its receipt
            // failed. Only positive original-slot re-observation can finish it.
            if !at_original {
                return Err(blocked("inverse outcome remains unknown; never replay"));
            }
            journal.generation = safe(journal.store.admit_root_reverse(&evidence))?;
            journal.verify()?;
            return Ok(ReturnedRoot {
                root: kind,
                observed: context.tree(kind).manifest.clone(),
            });
        }
        if ticket.admitted_manifest.is_none() {
            journal.generation = safe(journal.store.admit_root_reverse(&evidence))?;
            let current = context.tree(kind).manifest.digest()?;
            self.rotation
                .as_mut()
                .expect("ticket retained")
                .admitted_manifest = Some(current);
        }
        journal.verify()?;
        let reusable = self.recovery_copy.as_ref().is_some_and(|copy| {
            copy.manifest
                .as_ref()
                .is_some_and(|manifest| manifest.source == context.tree(kind).manifest)
        });
        if !reusable {
            // A failed or stale C1 is observed and kept, never resumed. Dropping
            // only its writer guards permits read-only observation; the private
            // root guard is retained across that gap when this process owns it.
            let ticket = self.rotation.as_ref().expect("ticket retained");
            let prior = safe(journal.store.context_root_backup(&ticket.effect_id))?;
            let previous = if let Some((_, plan)) = &prior {
                Some(self.observe_previous_recovery(
                    plan,
                    user,
                    context.tree(kind).limits,
                    journal,
                )?)
            } else {
                None
            };
            if let (Some(previous), Some((_, plan))) = (previous, &prior) {
                self.retained_recovery_attempts
                    .insert(plan.destination.clone(), previous);
            }
            let next_name = ComponentName::new(OsStr::new(&format!(
                "before-reverse-{}",
                uuid::Uuid::new_v4()
            )))?;
            let destination = HeldRoot::Absent {
                parent: self.parent.directory().clone(),
                name: next_name.clone(),
            };
            let evidence = RootBackupEvidence {
                recovery: RootRecoveryEvidence {
                    copy: self,
                    context,
                    boundary,
                    fence,
                    user,
                    records: &journal.root,
                    lease: journal.exclusive()?,
                    binding: &journal.binding,
                    generation: journal.generation,
                    returned: false,
                },
                destination: &destination,
                previous: prior
                    .as_ref()
                    .and_then(|(_, plan)| self.retained_recovery_attempts.get(&plan.destination)),
            };
            let (generation, digest) = safe(journal.store.prepare_root_backup(&evidence))?;
            journal.generation = generation;
            journal.verify()?;
            self.rotation
                .as_mut()
                .expect("ticket retained")
                .admitted_manifest = Some(digest.clone());
            self.recovery_copy = Some(Box::new(PrivateTreeCopy::new(
                self.parent.clone(),
                next_name,
            )));
            self.recovery_copy
                .as_mut()
                .expect("new C1 retained")
                .copy_from_plan(
                    context.tree(kind),
                    user,
                    journal,
                    Some((generation, digest)),
                )?;
        }
        let retained = self.recovery_copy.as_mut().expect("recovery copy retained");
        retained.verify(user)?;
        if retained.manifest()?.source != context.tree(kind).manifest {
            return Err(blocked(
                "current source differs from retained pre-reverse copy",
            ));
        }
        let saved_current = journal.retain(retained.manifest()?)?;
        let before = context.tree(kind).manifest.clone();
        let before_digest = journal.retain(&before)?;
        let saved = CopyReference {
            parent: retained.parent.directory().identity().clone(),
            name: text(&retained.name)?,
            manifest: saved_current.clone(),
        };
        let ticket = self.rotation.as_ref().expect("ticket retained");
        let reverse_plan = ReversePlan {
            schema: 1,
            original_effect_id: ticket.effect_id.clone(),
            original_intent_generation: ticket.intent_generation,
            current_manifest: ticket
                .admitted_manifest
                .clone()
                .expect("root recovery admitted"),
            saved,
        };
        let inverse = EffectKind::ReverseSourceRoot {
            original_effect_id: ticket.effect_id.clone(),
            original_intent_generation: ticket.intent_generation,
            current_manifest: ticket
                .admitted_manifest
                .clone()
                .expect("root recovery admitted"),
        };
        root.require_renameable()?;
        if !at_original {
            HeldRoot::Absent {
                parent: original_parent.clone(),
                name: original_name.clone(),
            }
            .verify()?;
        }
        let pending = journal.begin(
            inverse,
            &reverse_plan,
            &(
                "same-root-return-with-descendant-guard-gap",
                root.identity(),
                original_parent.identity(),
                text(&original_name)?,
            ),
        )?;
        self.rotation.as_mut().expect("ticket retained").reverse = Some(PendingEffect {
            id: pending.id.clone(),
            generation: pending.generation,
        });
        let result = (|| {
            context.tree(kind).verify()?;
            self.verify(user)?;
            fence.verify()?;
            journal.verify()?;
            if !at_original {
                context
                    .trees
                    .get_mut(&kind)
                    .expect("source retained")
                    .entries
                    .clear();
                root.rename_to(original_parent, original_name)?;
                #[cfg(test)]
                copy_fault(CopyFault::AfterReverseMove)?;
            }
            let admitted = HeldTree::admit(
                HeldRoot::Present(root),
                &mut Budget::new(context.tree(kind).limits)?,
                None,
            )?;
            // Preserve actual data even if a writer changed it during this second
            // guard gap. A later positive returned-slot observation may finish;
            // this attempt cannot claim equality or overwrite it from a backup.
            let unchanged = before.entries == admitted.manifest.entries;
            context.trees.insert(kind, admitted);
            if !unchanged {
                return Err(blocked("current root changed during reverse guard gap"));
            }
            fence.verify()?;
            let after = journal.retain(&context.tree(kind).manifest)?;
            journal.applied(pending, &(&before_digest, &saved_current, &after))?;
            Ok(ReturnedRoot {
                root: kind,
                observed: context.tree(kind).manifest.clone(),
            })
        })();
        // Any inverse failure retains its exact pending/Unknown effect. The next
        // call only re-observes an already returned object and never replays.
        result
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootSlotRecord {
    parent: FileIdentity,
    name: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyReference {
    parent: FileIdentity,
    name: String,
    manifest: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RotationPlan {
    schema: u32,
    root: RootKind,
    source_manifest: String,
    copy_manifest: String,
    original: RootSlotRecord,
    quarantine: RootSlotRecord,
    copy: CopyReference,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReversePlan {
    schema: u32,
    original_effect_id: String,
    original_intent_generation: u64,
    current_manifest: String,
    saved: CopyReference,
}
impl PrivateTreeCopy {
    /// Validated journal selectors are interpreted only inside these separately
    /// admitted held parents. This restores observation ownership, not permission
    /// to repeat either namespace operation.
    pub(crate) fn reopen_rotation(
        parent: Arc<PrivateDirectory>,
        original_parent: Arc<Directory>,
        quarantine: Arc<PrivateDirectory>,
        effect_id: &str,
        user: &CurrentUser,
        limits: SnapshotLimits,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        journal.verify()?;
        parent.verify(user)?;
        original_parent.recheck()?;
        quarantine.verify(user)?;
        let (effect, generation) = safe(journal.store.context_rotation(effect_id))?;
        let EffectKind::RotateSourceRoot { root, manifest } = &effect.kind else {
            return Err(blocked("not a source root rotation"));
        };
        let plan: RotationPlan =
            serde_json::from_slice(&safe(journal.store.read_manifest(&effect.before))?)?;
        if plan.schema != 1
            || plan.root != *root
            || &plan.copy_manifest != manifest
            || plan.copy.manifest != *manifest
            || plan.original.parent != *original_parent.identity()
            || plan.quarantine.parent != *quarantine.directory().identity()
            || plan.copy.parent != *parent.directory().identity()
        {
            return Err(blocked("rotation selectors differ from held parents"));
        }
        let expected: PrivateCopyManifest =
            serde_json::from_slice(&safe(journal.store.read_manifest(manifest))?)?;
        if expected.source.digest()? != plan.source_manifest {
            return Err(blocked("rotation source mapping differs"));
        }
        let mut copy = Self::reopen(
            parent.clone(),
            ComponentName::new(OsStr::new(&plan.copy.name))?,
            expected,
            user,
            limits,
        )?;
        let (admitted_manifest, inverse) = safe(journal.store.context_inverse(effect_id))?;
        let reverse = if let Some((effect, inverse_generation)) = inverse {
            let inverse_plan: ReversePlan =
                serde_json::from_slice(&safe(journal.store.read_manifest(&effect.before))?)?;
            if inverse_plan.schema != 1
                || inverse_plan.original_effect_id != effect_id
                || inverse_plan.original_intent_generation != generation
                || admitted_manifest.as_ref() != Some(&inverse_plan.current_manifest)
                || inverse_plan.saved.parent != *parent.directory().identity()
            {
                return Err(blocked("inverse selectors differ"));
            }
            let retained: PrivateCopyManifest = serde_json::from_slice(&safe(
                journal.store.read_manifest(&inverse_plan.saved.manifest),
            )?)?;
            copy.recovery_copy = Some(Box::new(Self::reopen(
                parent,
                ComponentName::new(OsStr::new(&inverse_plan.saved.name))?,
                retained,
                user,
                limits,
            )?));
            Some(PendingEffect {
                id: effect.effect_id,
                generation: inverse_generation,
            })
        } else {
            None
        };
        copy.rotation_attempted = true;
        copy.rotation = Some(RotationTicket {
            effect_id: effect_id.into(),
            intent_generation: generation,
            root: *root,
            original_parent,
            original_name: ComponentName::new(OsStr::new(&plan.original.name))?,
            quarantine,
            quarantine_name: ComponentName::new(OsStr::new(&plan.quarantine.name))?,
            reverse,
            admitted_manifest,
        });
        journal.verify()?;
        Ok(copy)
    }
}
