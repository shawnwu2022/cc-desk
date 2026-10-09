//! Operation-specific local NTFS receipts. Deliberately does not implement
//! DirectoryDurability: directory FlushFileBuffers is not a portable fsync.
use super::{
    blocked,
    files::{ComponentName, FileAccess, FileIdentity, PinnedFile, PrivateDirectory},
    handle,
    lease::ControlLease,
    security::CurrentUser,
    win_error,
};
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    sync::Arc,
};
use windows::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_OPEN, FILE_OPEN_IF, NTCREATEFILE_CREATE_DISPOSITION,
};
use windows::Win32::Storage::FileSystem::{
    FlushFileBuffers, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_WRITE_DATA, READ_CONTROL, SYNCHRONIZE,
};

const MAX_RECEIPT_BYTES: usize = 1024 * 1024;
/// The same write-through, no-delete/no-external-write handle survives flush,
/// readback and subsequent use. This is one immutable record, not a transaction
/// commit or evidence that unrelated metadata was durably persisted.
pub(crate) struct DurableRecord {
    file: PinnedFile,
    bytes: Vec<u8>,
    digest: String,
    _root: Arc<PrivateDirectory>,
}

const MAX_ARTIFACT_BYTES: usize = 32 * 1024 * 1024;
const MAX_LOG_BYTES: usize = 64 * 1024 * 1024;
const MAX_MARKER_FRAME_BYTES: usize = 16 * 1024;
const MAX_MARKER_RECORDS: u64 = 4096;

/// A secured mutable log is held with no external write/delete sharing. Only
/// append is exposed; failed writes leave their exact tail for inspection.
struct HeldLog {
    root: Arc<PrivateDirectory>,
    user: CurrentUser,
    file: PinnedFile,
}
impl HeldLog {
    fn open(
        root: Arc<PrivateDirectory>,
        name: &str,
        disposition: NTCREATEFILE_CREATE_DISPOSITION,
    ) -> io::Result<Self> {
        let user = CurrentUser::capture()?;
        root.verify(&user)?;
        let name = ComponentName::new(OsStr::new(name))?;
        let descriptor = user.descriptor(false)?;
        let file = root.directory().open_relative(
            &name,
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            FILE_SHARE_READ,
            disposition,
            false,
            Some(&descriptor),
        )?;
        user.verify_private_file(handle(&file), false)?;
        let file = PinnedFile::from_file(root.directory().clone(), name, file)?;
        Ok(Self { root, user, file })
    }
    fn verify(&self) -> io::Result<()> {
        self.root.verify(&self.user)?;
        self.file.verify()?;
        self.user
            .verify_private_file(handle(&self.file.file), false)
    }
    fn read(&self, maximum: usize) -> io::Result<Vec<u8>> {
        self.verify()?;
        let length = self.file.file.metadata()?.len();
        if length > maximum as u64 {
            return Err(blocked("persisted log exceeds capacity"));
        }
        let mut file = &self.file.file;
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != length || self.file.file.metadata()?.len() != length {
            return Err(blocked("persisted log changed during read"));
        }
        self.verify()?;
        Ok(bytes)
    }
    fn append(&self, length: u64, bytes: &[u8], operation: PersistenceOperation) -> io::Result<()> {
        self.verify()?;
        persist_bytes(&self.file, length, bytes, MAX_LOG_BYTES, operation)?;
        self.verify()
    }
}

/// JournalStore owns chain, capacity and effect validation. This object supplies
/// only the actual secured NTFS persistence boundary, with no ambient reopen or
/// claim that flushing this file commits unrelated filesystem metadata.
pub(crate) struct WindowsJournalStorage {
    log: HeldLog,
    transaction_id: Option<String>,
}
impl WindowsJournalStorage {
    pub(crate) fn open(root: Arc<PrivateDirectory>) -> io::Result<Self> {
        Ok(Self {
            log: HeldLog::open(root, "journal.log", FILE_OPEN_IF)?,
            transaction_id: None,
        })
    }
    pub(crate) fn transaction(
        root: Arc<PrivateDirectory>,
        transaction_id: &str,
        create: bool,
    ) -> io::Result<Self> {
        crate::version_history::journal::validate_id(transaction_id)
            .map_err(|_| blocked("invalid transaction journal name"))?;
        if create {
            let names = root.directory().read_children(100_000)?;
            let count = names
                .iter()
                .filter(|name| {
                    name.os_string().to_str().is_some_and(|name| {
                        name.strip_prefix("journal-")
                            .and_then(|name| name.strip_suffix(".log"))
                            .is_some_and(|id| {
                                crate::version_history::journal::validate_id(id).is_ok()
                            })
                    })
                })
                .count();
            if count >= 64 {
                return Err(blocked("retained transaction capacity reached"));
            }
        }
        let name = format!("journal-{transaction_id}.log");
        let log = HeldLog::open(root, &name, if create { FILE_CREATE } else { FILE_OPEN })?;
        Ok(Self {
            log,
            transaction_id: Some(transaction_id.into()),
        })
    }
    pub(crate) fn matches_transaction(&self, transaction_id: &str) -> bool {
        self.transaction_id
            .as_deref()
            .is_none_or(|expected| expected == transaction_id)
    }
    pub(crate) fn log_file(&self) -> io::Result<File> {
        self.log.verify()?;
        self.log.file.file.try_clone()
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.log.verify()
    }
    pub(crate) fn matches_root(&self, root: &PrivateDirectory) -> bool {
        self.log.root.directory().identity() == root.directory().identity()
    }
    pub(crate) fn append(&self, length: u64, bytes: &[u8]) -> io::Result<()> {
        self.log
            .append(length, bytes, PersistenceOperation::JournalFrame)
    }
    pub(crate) fn open_artifact(&self, digest: &str) -> io::Result<DurableArtifact> {
        DurableArtifact::open(self.log.root.clone(), digest, &self.log.user)
    }
    pub(crate) fn create_artifact(
        &self,
        digest: &str,
        bytes: &[u8],
    ) -> io::Result<DurableArtifact> {
        DurableArtifact::create(self.log.root.clone(), digest, bytes, &self.log.user)
    }
    pub(crate) fn namespace_valid(&self) -> io::Result<bool> {
        self.log.verify()?;
        let mut transactions = 0usize;
        for name in self.log.root.directory().read_children(100_000)? {
            let name_os = name.os_string();
            let Some(text) = name_os.to_str() else {
                return Ok(false);
            };
            let reserved = matches!(
                text,
                "journal.log"
                    | "journal.lock"
                    | "control.lock"
                    | "lifetime.lock"
                    | "active-context.log"
            );
            let manifest = text
                .strip_prefix("manifest-")
                .and_then(|text| text.strip_suffix(".json"));
            let transaction = text
                .strip_prefix("journal-")
                .and_then(|text| text.strip_suffix(".log"));
            let named_journal = transaction
                .is_some_and(|id| crate::version_history::journal::validate_id(id).is_ok());
            if named_journal {
                transactions += 1;
                if transactions > 64 {
                    return Ok(false);
                }
            }
            if !reserved
                && !named_journal
                && !manifest.is_some_and(|digest| {
                    crate::version_history::journal::validate_digest(digest).is_ok()
                })
            {
                return Ok(false);
            }
            // Attribute/security-only observation admits our own writer but
            // never replaces its original guard or supplies write authority.
            let file = self.log.root.directory().open_relative(
                &name,
                FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                FILE_OPEN,
                false,
                None,
            )?;
            self.log.user.verify_private_file(handle(&file), false)?;
            PinnedFile::from_file(self.log.root.directory().clone(), name, file)?.verify()?;
        }
        Ok(true)
    }
}

/// Larger immutable journal dependencies have a separate 32 MiB bound. Package
/// bytes and arbitrary snapshot file contents must not use this manifest API.
pub(crate) struct DurableArtifact {
    root: Arc<PrivateDirectory>,
    file: PinnedFile,
    length: u64,
    digest: String,
}
impl DurableArtifact {
    fn name(digest: &str) -> io::Result<ComponentName> {
        crate::version_history::journal::validate_digest(digest)
            .map_err(|_| blocked("invalid artifact digest"))?;
        ComponentName::new(OsStr::new(&format!("manifest-{digest}.json")))
    }
    fn create(
        root: Arc<PrivateDirectory>,
        digest: &str,
        bytes: &[u8],
        user: &CurrentUser,
    ) -> io::Result<Self> {
        if bytes.is_empty()
            || bytes.len() > MAX_ARTIFACT_BYTES
            || crate::version_history::verified_package::sha256(bytes) != digest
        {
            return Err(blocked("invalid artifact content"));
        }
        root.verify(user)?;
        let name = Self::name(digest)?;
        let descriptor = user.descriptor(false)?;
        let file = root.directory().open_relative(
            &name,
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            FILE_SHARE_READ,
            FILE_CREATE,
            false,
            Some(&descriptor),
        )?;
        user.verify_private_file(handle(&file), false)?;
        let file = PinnedFile::from_file(root.directory().clone(), name, file)?;
        persist_bytes(
            &file,
            0,
            bytes,
            MAX_ARTIFACT_BYTES,
            PersistenceOperation::Artifact,
        )?;
        let artifact = Self {
            root,
            file,
            length: bytes.len() as u64,
            digest: digest.into(),
        };
        artifact.verify()?;
        Ok(artifact)
    }
    fn open(root: Arc<PrivateDirectory>, digest: &str, user: &CurrentUser) -> io::Result<Self> {
        root.verify(user)?;
        let file = root
            .directory()
            .open_file(Self::name(digest)?, FileAccess::Read)?;
        user.verify_private_file(handle(&file.file), false)?;
        let length = file.file.metadata()?.len();
        if length == 0 || length > MAX_ARTIFACT_BYTES as u64 {
            return Err(blocked("invalid persisted artifact size"));
        }
        let artifact = Self {
            root,
            file,
            length,
            digest: digest.into(),
        };
        artifact.verify()?;
        if artifact.file.digest()? != digest {
            return Err(blocked("persisted artifact differs"));
        }
        Ok(artifact)
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        let user = CurrentUser::capture()?;
        self.root.verify(&user)?;
        self.file.verify()?;
        user.verify_private_file(handle(&self.file.file), false)?;
        if self.file.file.metadata()?.len() != self.length {
            return Err(blocked("persisted artifact length changed"));
        }
        Ok(())
    }
    pub(crate) fn read(&self) -> io::Result<Vec<u8>> {
        self.verify()?;
        let mut file = &self.file.file;
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(MAX_ARTIFACT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 != self.length
            || crate::version_history::verified_package::sha256(&bytes) != self.digest
        {
            return Err(blocked("persisted artifact content changed"));
        }
        self.verify()?;
        Ok(bytes)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PersistenceOperation {
    JournalFrame,
    Artifact,
    MarkerFrame,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PersistenceBoundary {
    BeforeWrite,
    PartialWrite,
    AfterWrite,
    FlushCall,
    AfterFlush,
    Readback,
    ChangedReadback,
}

fn persist_bytes(
    file: &PinnedFile,
    length: u64,
    bytes: &[u8],
    maximum: usize,
    operation: PersistenceOperation,
) -> io::Result<()> {
    file.verify()?;
    let end = length
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| blocked("persistence capacity exceeded"))?;
    if bytes.is_empty() || end > maximum as u64 || file.file.metadata()?.len() != length {
        return Err(blocked("persistence length changed"));
    }
    let mut handle_file = &file.file;
    handle_file.seek(SeekFrom::Start(length))?;
    fail_at(operation, PersistenceBoundary::BeforeWrite)?;
    if fault_at(operation, PersistenceBoundary::PartialWrite) {
        handle_file.write_all(&bytes[..(bytes.len() / 2).max(1)])?;
        return Err(io::Error::other("injected partial persistence write"));
    }
    handle_file.write_all(bytes)?;
    fail_at(operation, PersistenceBoundary::AfterWrite)?;
    flush_persistence(file, operation)?;
    fail_at(operation, PersistenceBoundary::AfterFlush)?;
    if fault_at(operation, PersistenceBoundary::ChangedReadback) {
        handle_file.seek(SeekFrom::Start(length))?;
        handle_file.write_all(&[bytes[0] ^ 0xff])?;
    }
    handle_file.seek(SeekFrom::Start(length))?;
    let mut actual = vec![0; bytes.len()];
    if fault_at(operation, PersistenceBoundary::Readback) {
        let half = (actual.len() / 2).max(1);
        handle_file.read_exact(&mut actual[..half])?;
        return Err(io::Error::other("injected incomplete persistence readback"));
    }
    handle_file.read_exact(&mut actual)?;
    if actual != bytes || file.file.metadata()?.len() != end {
        return Err(blocked("persistence readback differs"));
    }
    file.verify()
}

fn flush_persistence(file: &PinnedFile, operation: PersistenceOperation) -> io::Result<()> {
    #[cfg(test)]
    if fault_at(operation, PersistenceBoundary::FlushCall) {
        // Exercise a real FlushFileBuffers error on the exact same object using
        // a read-only observation handle. The original write-through guard stays
        // retained, and its complete bytes may still reconcile after reopening.
        let read_only = file.parent.open_relative(
            &file.name,
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_OPEN,
            false,
            None,
        )?;
        if super::files::metadata(handle(&read_only))?.identity != *file.identity() {
            return Err(blocked("flush fault target differs"));
        }
        let result = unsafe { FlushFileBuffers(handle(&read_only)).map_err(win_error) };
        PERSISTENCE_FLUSH_ERROR.set(result.as_ref().err().and_then(io::Error::raw_os_error));
        return result;
    }
    let _ = operation;
    unsafe { FlushFileBuffers(handle(&file.file)).map_err(win_error) }
}

#[cfg(test)]
thread_local! {
    static PERSISTENCE_FAULT: std::cell::Cell<Option<(PersistenceOperation, PersistenceBoundary)>> = const { std::cell::Cell::new(None) };
    static PERSISTENCE_FLUSH_ERROR: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
pub(crate) struct PersistenceProbeGuard(std::marker::PhantomData<std::rc::Rc<()>>);
#[cfg(test)]
impl PersistenceProbeGuard {
    pub(crate) fn flush_error(&self) -> Option<i32> {
        PERSISTENCE_FLUSH_ERROR.get()
    }
}
#[cfg(test)]
impl Drop for PersistenceProbeGuard {
    fn drop(&mut self) {
        PERSISTENCE_FAULT.set(None);
    }
}
#[cfg(test)]
pub(crate) fn probe_persistence_fault(
    operation: PersistenceOperation,
    boundary: PersistenceBoundary,
) -> PersistenceProbeGuard {
    PERSISTENCE_FLUSH_ERROR.set(None);
    PERSISTENCE_FAULT.with(|fault| {
        assert!(fault.get().is_none());
        fault.set(Some((operation, boundary)));
    });
    PersistenceProbeGuard(std::marker::PhantomData)
}
fn fault_at(operation: PersistenceOperation, boundary: PersistenceBoundary) -> bool {
    #[cfg(test)]
    {
        PERSISTENCE_FAULT.with(|fault| {
            if fault.get() == Some((operation, boundary)) {
                fault.set(None);
                true
            } else {
                false
            }
        })
    }
    #[cfg(not(test))]
    {
        let _ = (operation, boundary);
        false
    }
}
fn fail_at(operation: PersistenceOperation, boundary: PersistenceBoundary) -> io::Result<()> {
    if fault_at(operation, boundary) {
        Err(io::Error::other("injected persistence boundary failure"))
    } else {
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MarkerFrame {
    schema: u32,
    sequence: u64,
    previous: Option<String>,
    marker: crate::version_history::maintenance::ActiveContextMarker,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MarkerEnvelope {
    frame: MarkerFrame,
    digest: String,
}
impl MarkerEnvelope {
    fn new(frame: MarkerFrame) -> io::Result<Self> {
        let digest = crate::version_history::verified_package::sha256(&serde_json::to_vec(&frame)?);
        Ok(Self { frame, digest })
    }
    fn encode(&self) -> io::Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(self)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_MARKER_FRAME_BYTES {
            return Err(blocked("marker frame exceeds capacity"));
        }
        Ok(bytes)
    }
}

/// Append-only markers for one bound switch. Borrowing the exact root's control
/// lease prevents an unlocked writer. It retains every earlier marker and never
/// reuses a valid prefix after a torn tail. No startup factory is supplied here.
pub(crate) struct MarkerStore<'control> {
    log: HeldLog,
    control: &'control ControlLease,
    last: MarkerEnvelope,
    current: Vec<u8>,
    length: u64,
    poisoned: bool,
    transactions: std::collections::BTreeSet<String>,
}
impl<'control> MarkerStore<'control> {
    pub(crate) fn create(
        root: Arc<PrivateDirectory>,
        control: &'control ControlLease,
        marker: &crate::version_history::maintenance::ActiveContextMarker,
        journal: &mut crate::version_history::journal::JournalStore,
    ) -> io::Result<Self> {
        control.verify_root(&root)?;
        journal
            .validate_marker_publication(&root, marker)
            .map_err(|_| blocked("marker journal checkpoint differs"))?;
        let current = marker.encode().map_err(|_| blocked("invalid marker"))?;
        let last = MarkerEnvelope::new(MarkerFrame {
            schema: 1,
            sequence: 0,
            previous: None,
            marker: serde_json::from_slice(&current)?,
        })?;
        let bytes = last.encode()?;
        let log = HeldLog::open(root, "active-context.log", FILE_CREATE)?;
        log.append(0, &bytes, PersistenceOperation::MarkerFrame)?;
        journal
            .validate_marker_publication(&log.root, marker)
            .map_err(|_| blocked("marker journal checkpoint changed during publication"))?;
        control.verify_root(&log.root)?;
        Ok(Self {
            log,
            control,
            last,
            current,
            length: bytes.len() as u64,
            poisoned: false,
            transactions: std::collections::BTreeSet::from([marker
                .binding()
                .transaction_id
                .clone()]),
        })
    }
    pub(crate) fn open_existing(
        root: Arc<PrivateDirectory>,
        control: &'control ControlLease,
    ) -> io::Result<Option<Self>> {
        control.verify_root(&root)?;
        let log = match HeldLog::open(root, "active-context.log", FILE_OPEN) {
            Ok(log) => log,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let bytes = log.read(MAX_LOG_BYTES)?;
        let mut last: Option<MarkerEnvelope> = None;
        let mut transactions = std::collections::BTreeSet::new();
        for (sequence, frame) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
            if sequence as u64 >= MAX_MARKER_RECORDS
                || frame.len() > MAX_MARKER_FRAME_BYTES
                || !frame.ends_with(b"\n")
            {
                return Err(blocked("marker log has an uncertain tail"));
            }
            let envelope: MarkerEnvelope = serde_json::from_slice(frame)?;
            envelope
                .frame
                .marker
                .validate_structure()
                .map_err(|_| blocked("invalid marker"))?;
            if envelope.frame.schema != 1
                || envelope.frame.sequence != sequence as u64
                || envelope.frame.previous.as_deref()
                    != last.as_ref().map(|last| last.digest.as_str())
                || envelope.digest
                    != crate::version_history::verified_package::sha256(&serde_json::to_vec(
                        &envelope.frame,
                    )?)
                || last.as_ref().is_some_and(|last| {
                    !envelope.frame.marker.follows(&last.frame.marker)
                        && !envelope.frame.marker.succeeds_terminal(&last.frame.marker)
                })
            {
                return Err(blocked("marker chain differs"));
            }
            let transaction = &envelope.frame.marker.binding().transaction_id;
            if last
                .as_ref()
                .is_none_or(|last| last.frame.marker.binding().transaction_id != *transaction)
                && (!transactions.insert(transaction.clone()) || transactions.len() > 64)
            {
                return Err(blocked("marker reused an earlier transaction"));
            }
            last = Some(envelope);
        }
        let last = last.ok_or_else(|| blocked("empty marker log is not authoritative absence"))?;
        let current = last
            .frame
            .marker
            .encode()
            .map_err(|_| blocked("invalid marker"))?;
        Ok(Some(Self {
            log,
            control,
            last,
            current,
            length: bytes.len() as u64,
            poisoned: false,
            transactions,
        }))
    }
    pub(crate) fn current(&self) -> io::Result<&[u8]> {
        if self.poisoned {
            return Err(blocked("marker reconciliation required"));
        }
        self.control.verify_root(&self.log.root)?;
        self.log.verify()?;
        if self.log.file.file.metadata()?.len() != self.length {
            return Err(blocked("marker log length changed"));
        }
        Ok(&self.current)
    }
    pub(crate) fn append(
        &mut self,
        marker: &crate::version_history::maintenance::ActiveContextMarker,
        journal: &mut crate::version_history::journal::JournalStore,
    ) -> io::Result<()> {
        self.append_checked(marker, journal, false)
    }
    /// Terminal rollover never resets the marker or reuses a completed journal.
    /// Both original live stores must match this control root and their exact
    /// checkpoints. A detached inspection or an unrelated completed transaction
    /// cannot release the current source into a fresh switch.
    pub(crate) fn append_successor(
        &mut self,
        marker: &crate::version_history::maintenance::ActiveContextMarker,
        prior: &mut crate::version_history::journal::JournalStore,
        successor: &mut crate::version_history::journal::JournalStore,
    ) -> io::Result<()> {
        self.current()?;
        if !marker.succeeds_terminal(&self.last.frame.marker) {
            return Err(blocked("marker does not follow this restored source"));
        }
        if self.transactions.contains(&marker.binding().transaction_id)
            || self.transactions.len() >= 64
        {
            return Err(blocked(
                "marker transaction was already used or exceeds capacity",
            ));
        }
        prior
            .validate_marker_publication(&self.log.root, &self.last.frame.marker)
            .map_err(|_| blocked("previous terminal journal checkpoint differs"))?;
        successor
            .validate_marker_successor(&self.log.root, marker)
            .map_err(|_| blocked("successor journal is not reviewed"))?;
        self.append_checked(marker, successor, true)?;
        // The old writer remained exclusively borrowed through the append.
        // Its exact held log/artifact objects remain retained by the caller.
        Ok(())
    }
    fn append_checked(
        &mut self,
        marker: &crate::version_history::maintenance::ActiveContextMarker,
        journal: &mut crate::version_history::journal::JournalStore,
        successor: bool,
    ) -> io::Result<()> {
        if self.poisoned {
            return Err(blocked("marker reconciliation required"));
        }
        self.control.verify_root(&self.log.root)?;
        journal
            .validate_marker_publication(&self.log.root, marker)
            .map_err(|_| blocked("marker journal checkpoint differs"))?;
        if !(if successor {
            marker.succeeds_terminal(&self.last.frame.marker)
        } else {
            marker.follows(&self.last.frame.marker)
        }) || self.last.frame.sequence + 1 >= MAX_MARKER_RECORDS
        {
            return Err(blocked("marker generation or capacity differs"));
        }
        let current = marker.encode().map_err(|_| blocked("invalid marker"))?;
        let next = MarkerEnvelope::new(MarkerFrame {
            schema: 1,
            sequence: self.last.frame.sequence + 1,
            previous: Some(self.last.digest.clone()),
            marker: serde_json::from_slice(&current)?,
        })?;
        let bytes = next.encode()?;
        self.poisoned = true;
        self.log
            .append(self.length, &bytes, PersistenceOperation::MarkerFrame)?;
        journal
            .validate_marker_publication(&self.log.root, marker)
            .map_err(|_| blocked("marker journal checkpoint changed during publication"))?;
        self.control.verify_root(&self.log.root)?;
        self.length += bytes.len() as u64;
        self.last = next;
        self.current = current;
        self.transactions
            .insert(marker.binding().transaction_id.clone());
        self.poisoned = false;
        Ok(())
    }
}
impl DurableRecord {
    pub(crate) fn create(
        root: Arc<PrivateDirectory>,
        name: ComponentName,
        bytes: &[u8],
        user: &CurrentUser,
    ) -> io::Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_RECEIPT_BYTES {
            return Err(blocked("unsupported receipt size"));
        }
        root.verify(user)?;
        let security = user.descriptor(false)?;
        let mut file = root.directory().open_relative(
            &name,
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            FILE_SHARE_READ,
            FILE_CREATE,
            false,
            Some(&security),
        )?;
        user.verify_private_file(handle(&file), false)?;
        // On any failure the partial file remains for reconciliation. Never
        // truncate, delete, overwrite or silently retry a failed record write.
        file.write_all(bytes)?;
        unsafe {
            FlushFileBuffers(handle(&file)).map_err(win_error)?;
        }
        let file = PinnedFile::from_file(root.directory().clone(), name, file)?;
        let result = Self {
            file,
            bytes: bytes.to_vec(),
            digest: crate::version_history::verified_package::sha256(bytes),
            _root: root,
        };
        result.verify()?;
        Ok(result)
    }
    /// Reopen only through the secured retained root and the expected digest
    /// from the validated journal. Existence alone never supplies authority.
    pub(crate) fn open(
        root: Arc<PrivateDirectory>,
        name: ComponentName,
        expected_digest: &str,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        root.verify(user)?;
        let file = root.directory().open_file(name, FileAccess::Read)?;
        user.verify_private_file(handle(&file.file), false)?;
        let mut source = &file.file;
        source.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        source
            .take((MAX_RECEIPT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.is_empty()
            || bytes.len() > MAX_RECEIPT_BYTES
            || crate::version_history::verified_package::sha256(&bytes) != expected_digest
        {
            return Err(blocked("persisted receipt differs from the journal"));
        }
        let result = Self {
            file,
            bytes,
            digest: expected_digest.into(),
            _root: root,
        };
        result.verify()?;
        Ok(result)
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.file.verify()?;
        let mut file = &self.file.file;
        file.seek(SeekFrom::Start(0))?;
        let mut actual = Vec::new();
        file.take((MAX_RECEIPT_BYTES + 1) as u64)
            .read_to_end(&mut actual)?;
        if actual != self.bytes || self.file.digest()? != self.digest {
            return Err(blocked("durable receipt changed"));
        }
        Ok(())
    }
    pub(crate) fn verify_after_parent_rename(&self) -> io::Result<()> {
        self.verify()
    }
    pub(super) fn root_identity(&self) -> &FileIdentity {
        self._root.directory().identity()
    }
    pub(super) fn file_identity(&self) -> &FileIdentity {
        self.file.identity()
    }
    pub(super) fn name(&self) -> &ComponentName {
        &self.file.name
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }
}

/// Authoritative write completion must be proved separately, before this
/// post-exit flush. A successful flush cannot repair an interrupted truncation.
pub(crate) fn flush_held_file(file: &PinnedFile, expected_digest: &str) -> io::Result<()> {
    file.verify()?;
    unsafe {
        FlushFileBuffers(handle(&file.file)).map_err(win_error)?;
    }
    if file.digest()? != expected_digest {
        return Err(blocked("flushed file content differs"));
    }
    Ok(())
}
