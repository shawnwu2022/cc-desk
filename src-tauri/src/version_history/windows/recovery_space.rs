//! A bounded minimum-abort allocation in the actual private transaction root.
//! Free-space observations do not construct this capability. This is neither an
//! NTFS transaction nor a guarantee against other users consuming free space.
use super::{
    blocked,
    durability::MarkerStore,
    files::{final_path, metadata, ComponentName, FileIdentity, PinnedFile},
    handle,
    lease::ControlLease,
    own,
    security::CurrentUser,
    startup::{InstallationControl, TransactionDataRoot},
    win_error,
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        journal::{JournalBinding, JournalStore},
        maintenance::ActiveContextMarker,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    mem::size_of,
    sync::Arc,
};
use windows::{
    core::PWSTR,
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            NtCreateFile, FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
            FILE_SYNCHRONOUS_IO_NONALERT, FILE_WRITE_THROUGH,
        },
    },
    Win32::{
        Foundation::{HANDLE, OBJ_DONT_REPARSE, UNICODE_STRING},
        Storage::FileSystem::{
            FileAllocationInfo, FileEndOfFileInfo, FileStandardInfo, FlushFileBuffers,
            GetFileInformationByHandleEx, SetFileInformationByHandle, FILE_ALLOCATION_INFO,
            FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_SPARSE_FILE,
            FILE_END_OF_FILE_INFO, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ,
            FILE_SHARE_WRITE, FILE_STANDARD_INFO, FILE_WRITE_DATA, READ_CONTROL, SYNCHRONIZE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

pub(crate) const ABORT_RESERVE_FILENAME: &str = "abort-reserve.bin";
/// Matches space.rs's control/abort headroom. The independently measured small
/// record allocation remains after consumption, so all 128 MiB can be released.
pub(crate) const MINIMUM_ABORT_BYTES: u64 = 128 * 1024 * 1024;
const HEADER_BYTES: usize = 4096;
const MAX_RECORD_ALLOCATION: u64 = 2 * 1024 * 1024;
const PREPARING: u8 = b'P';
const HELD: u8 = b'H';
const ATTEMPTED: u8 = b'U';

fn unavailable(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_ABORT_RESERVE_UNAVAILABLE")
}

/// Persisted lookup evidence only. There is deliberately no deserialization
/// path from this record to an AbortReserve or a writable restart handle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AbortReserveRecord {
    schema: u32,
    transaction_id: String,
    data_root: FileIdentity,
    control_root: FileIdentity,
    file: FileIdentity,
    record_allocation: u64,
    minimum_bytes: u64,
}
impl AbortReserveRecord {
    fn header(&self, state: u8) -> io::Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self)?;
        if bytes.is_empty() || bytes.len() + 1 >= HEADER_BYTES {
            return Err(blocked("reserve record exceeds capacity"));
        }
        let mut header = vec![0; HEADER_BYTES];
        header[..bytes.len()].copy_from_slice(&bytes);
        header[HEADER_BYTES - 1] = state;
        Ok(header)
    }
    fn validate(&self, roots: &ReserveRoots, identity: &FileIdentity) -> io::Result<()> {
        if self.schema != 1
            || self.transaction_id != roots.data.transaction_id()
            || &self.data_root != roots.data.root().directory().identity()
            || &self.control_root != roots.installation.root().directory().identity()
            || &self.file != identity
            || self.minimum_bytes != MINIMUM_ABORT_BYTES
            || !(HEADER_BYTES as u64..=MAX_RECORD_ALLOCATION).contains(&self.record_allocation)
        {
            return Err(blocked("reserve record does not describe the held objects"));
        }
        Ok(())
    }
    fn target(&self) -> u64 {
        self.record_allocation + MINIMUM_ABORT_BYTES
    }
}

struct ReserveRoots {
    data: Arc<TransactionDataRoot>,
    installation: Arc<InstallationControl>,
    user: CurrentUser,
}
impl ReserveRoots {
    fn new(
        data: Arc<TransactionDataRoot>,
        installation: Arc<InstallationControl>,
        control: &ControlLease,
    ) -> Result<Self, SafeError> {
        let roots = Self {
            data,
            installation,
            user: CurrentUser::capture().map_err(unavailable)?,
        };
        roots.verify().map_err(unavailable)?;
        control
            .verify_root(roots.installation.root())
            .map_err(unavailable)?;
        Ok(roots)
    }
    fn verify(&self) -> io::Result<()> {
        self.data
            .verify_installation(&self.installation)
            .map_err(|_| blocked("transaction data root changed"))?;
        self.data.root().verify(&self.user)?;
        self.installation.root().verify(&self.user)?;
        self.data
            .root()
            .directory()
            .require_same_volume(self.installation.root().directory())?;
        if self.data.root().directory().identity()
            == self.installation.root().directory().identity()
        {
            return Err(blocked("reserve and control need distinct held roots"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReserveObservationState {
    Preparing,
    Held,
    ReleaseAttempted,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct ReserveObservation {
    pub(crate) logical_bytes: u64,
    pub(crate) allocated_bytes: u64,
    pub(crate) state: ReserveObservationState,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReleaseState {
    Available,
    Uncertain,
    Released,
}

/// A positively created object is retained even if metadata, record write,
/// allocation, flush or readback subsequently fails. Drop never deletes it.
struct OwnedAllocation {
    roots: ReserveRoots,
    file: File,
    identity: Option<FileIdentity>,
    record: Option<AbortReserveRecord>,
    state: ReleaseState,
}
pub(crate) struct AbortReserve {
    owned: OwnedAllocation,
    binding: JournalBinding,
}
pub(crate) struct PartialAbortReserve {
    owned: OwnedAllocation,
}
pub(crate) struct ReserveCreateFailure {
    error: SafeError,
    partial: Option<Box<PartialAbortReserve>>,
}
impl std::fmt::Debug for ReserveCreateFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReserveCreateFailure")
            .field("error", &self.error)
            .field("retains_created_object", &self.partial.is_some())
            .finish()
    }
}
impl ReserveCreateFailure {
    fn before_create(error: SafeError) -> Self {
        Self {
            error,
            partial: None,
        }
    }
    fn after_create(error: SafeError, owned: OwnedAllocation) -> Self {
        Self {
            error,
            partial: Some(Box::new(PartialAbortReserve { owned })),
        }
    }
    pub(crate) fn error(&self) -> &SafeError {
        &self.error
    }
    pub(crate) fn into_partial(self) -> Option<Box<PartialAbortReserve>> {
        self.partial
    }
}

impl AbortReserve {
    pub(crate) fn create(
        data: Arc<TransactionDataRoot>,
        installation: Arc<InstallationControl>,
        control: &ControlLease,
        store: &mut JournalStore,
        binding: &JournalBinding,
        generation: u64,
    ) -> Result<Self, ReserveCreateFailure> {
        let roots = ReserveRoots::new(data, installation, control)
            .map_err(ReserveCreateFailure::before_create)?;
        if roots.data.transaction_id() != binding.transaction_id {
            return Err(ReserveCreateFailure::before_create(unavailable(
                "transaction mismatch",
            )));
        }
        store
            .verify_windows_binding(roots.installation.root(), binding, generation)
            .map_err(ReserveCreateFailure::before_create)?;
        // The marker and journal are opened through this same held control root;
        // neither a caller's claim about the volume nor a digest substitutes.
        let marker = MarkerStore::open_existing(roots.installation.root().clone(), control)
            .map_err(|failure| ReserveCreateFailure::before_create(unavailable(failure)))?
            .ok_or_else(|| {
                ReserveCreateFailure::before_create(unavailable("missing active marker"))
            })?;
        let active = ActiveContextMarker::decode(
            marker
                .current()
                .map_err(|failure| ReserveCreateFailure::before_create(unavailable(failure)))?,
        )
        .map_err(ReserveCreateFailure::before_create)?;
        if active.is_terminal() || active.binding() != binding {
            return Err(ReserveCreateFailure::before_create(unavailable(
                "inactive transaction marker",
            )));
        }
        drop(marker);
        let mut owned = create_owned(roots)?;
        if let Err(failure) = owned.initialize() {
            return Err(ReserveCreateFailure::after_create(
                unavailable(failure),
                owned,
            ));
        }
        if let Err(failure) =
            store.verify_windows_binding(owned.roots.installation.root(), binding, generation)
        {
            return Err(ReserveCreateFailure::after_create(failure, owned));
        }
        if let Err(failure) = control.verify_root(owned.roots.installation.root()) {
            return Err(ReserveCreateFailure::after_create(
                unavailable(failure),
                owned,
            ));
        }
        Ok(Self {
            owned,
            binding: binding.clone(),
        })
    }
    pub(crate) fn record(&self) -> &AbortReserveRecord {
        // Only successful initialization constructs this public capability.
        self.owned
            .record
            .as_ref()
            .expect("admitted reserve has its owned record")
    }
    pub(crate) fn verify_for(
        &self,
        data: &TransactionDataRoot,
        installation: &InstallationControl,
        binding: &JournalBinding,
    ) -> Result<(), SafeError> {
        data.verify_installation(installation)?;
        if &self.binding != binding
            || data.transaction_id() != binding.transaction_id
            || data.root().directory().identity()
                != self.owned.roots.data.root().directory().identity()
            || installation.root().directory().identity()
                != self.owned.roots.installation.root().directory().identity()
            || self.owned.state != ReleaseState::Available
        {
            return Err(unavailable("reserve owner changed or release attempted"));
        }
        let observed = self.owned.inspect().map_err(unavailable)?;
        let record = self.record();
        if observed.state != ReserveObservationState::Held
            || observed.logical_bytes != record.target()
            || observed.allocated_bytes < record.target()
            || observed.allocated_bytes > MINIMUM_ABORT_BYTES + MAX_RECORD_ALLOCATION
        {
            return Err(unavailable("minimum abort allocation is not held"));
        }
        Ok(())
    }
    /// Only consumes the allocation of this exact create-new file. It grants no
    /// source, installer, context, marker-clear or terminal authority.
    pub(crate) fn release_once(
        &mut self,
        control: &ControlLease,
    ) -> Result<ReserveRelease, SafeError> {
        self.owned.release_once(control).map_err(unavailable)
    }
}
impl PartialAbortReserve {
    pub(crate) fn inspect(&self) -> Result<ReserveObservation, SafeError> {
        self.owned.inspect().map_err(unavailable)
    }
    /// A failed construction is never a reserve. Once its own immutable header
    /// was established, its private tail can still be consumed exactly once.
    /// Earlier record/identity failures stay retained for read-only recovery.
    pub(crate) fn release_owned_partial_once(
        &mut self,
        control: &ControlLease,
    ) -> Result<ReserveRelease, SafeError> {
        self.owned.release_once(control).map_err(unavailable)
    }
}

pub(crate) struct ReserveRelease {
    released_bytes: u64,
    remaining_allocation: u64,
}
impl ReserveRelease {
    pub(crate) fn released_bytes(&self) -> u64 {
        self.released_bytes
    }
    pub(crate) fn remaining_allocation(&self) -> u64 {
        self.remaining_allocation
    }
}
impl OwnedAllocation {
    fn verify_file(&self) -> io::Result<FileIdentity> {
        self.roots.verify()?;
        self.roots
            .user
            .verify_private_file(handle(&self.file), false)?;
        let actual = metadata(handle(&self.file))?;
        if actual.directory
            || actual.attributes & (FILE_ATTRIBUTE_SPARSE_FILE.0 | FILE_ATTRIBUTE_COMPRESSED.0) != 0
            || self
                .identity
                .as_ref()
                .is_some_and(|identity| identity != &actual.identity)
        {
            return Err(blocked(
                "reserve identity, type or allocation format changed",
            ));
        }
        let parent = self.roots.data.root().directory();
        let name = ComponentName::new(OsStr::new(ABORT_RESERVE_FILENAME))?;
        let named = parent.open_relative(
            &name,
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_OPEN,
            false,
            None,
        )?;
        if metadata(handle(&named))?.identity != actual.identity {
            return Err(blocked("reserve slot identifies another file"));
        }
        let mut expected = final_path(parent.raw())?;
        if expected.last() != Some(&(b'\\' as u16)) {
            expected.push(b'\\' as u16);
        }
        expected.extend(ABORT_RESERVE_FILENAME.encode_utf16());
        if final_path(handle(&self.file))? != expected {
            return Err(blocked("reserve moved outside its fixed slot"));
        }
        Ok(actual.identity)
    }
    fn initialize(&mut self) -> io::Result<()> {
        self.identity = Some(self.verify_file()?);
        // Establish only the bounded record first. No large allocation precedes
        // its immutable ownership record and measured preservation floor.
        let mut file = &self.file;
        file.write_all(&[0; HEADER_BYTES])?;
        flush(&self.file)?;
        let (logical, allocated) = sizes(&self.file)?;
        if logical != HEADER_BYTES as u64
            || !(HEADER_BYTES as u64..=MAX_RECORD_ALLOCATION).contains(&allocated)
        {
            return Err(blocked("unsupported NTFS record allocation"));
        }
        let record = AbortReserveRecord {
            schema: 1,
            transaction_id: self.roots.data.transaction_id().into(),
            data_root: self.roots.data.root().directory().identity().clone(),
            control_root: self
                .roots
                .installation
                .root()
                .directory()
                .identity()
                .clone(),
            file: self
                .identity
                .clone()
                .ok_or_else(|| blocked("missing owned identity"))?,
            record_allocation: allocated,
            minimum_bytes: MINIMUM_ABORT_BYTES,
        };
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&record.header(PREPARING)?)?;
        flush(&self.file)?;
        read_header(&self.file, &record)?;
        self.record = Some(record.clone());
        #[cfg(test)]
        if take_fault(ReserveFault::PartialAllocation) {
            set_allocation(&self.file, allocated + MINIMUM_ABORT_BYTES / 2)?;
            set_eof(&self.file, allocated + MINIMUM_ABORT_BYTES / 2)?;
            flush(&self.file)?;
            return Err(io::Error::from_raw_os_error(112));
        }
        set_allocation(&self.file, record.target())?;
        // Keep the positively allocated tail across final close/restart. EOF is
        // never used as proof of allocation: FileStandardInfo is checked below.
        set_eof(&self.file, record.target())?;
        flush(&self.file)?;
        let observed = self.inspect()?;
        if observed.logical_bytes != record.target()
            || observed.allocated_bytes < record.target()
            || observed.allocated_bytes > MINIMUM_ABORT_BYTES + MAX_RECORD_ALLOCATION
        {
            return Err(blocked("NTFS did not retain the minimum abort allocation"));
        }
        write_state(&self.file, HELD)?;
        let admitted = self.inspect()?;
        if admitted.state != ReserveObservationState::Held
            || admitted.logical_bytes != record.target()
            || admitted.allocated_bytes < record.target()
            || admitted.allocated_bytes > MINIMUM_ABORT_BYTES + MAX_RECORD_ALLOCATION
        {
            return Err(blocked("reserve completion readback differs"));
        }
        Ok(())
    }
    fn inspect(&self) -> io::Result<ReserveObservation> {
        let identity = self.verify_file()?;
        let record = self
            .record
            .as_ref()
            .ok_or_else(|| blocked("partial reserve record not established"))?;
        record.validate(&self.roots, &identity)?;
        let state = read_header(&self.file, record)?;
        let (logical_bytes, allocated_bytes) = sizes(&self.file)?;
        self.verify_file()?;
        observation(record, logical_bytes, allocated_bytes, state)
    }
    fn release_once(&mut self, control: &ControlLease) -> io::Result<ReserveRelease> {
        if self.state != ReleaseState::Available {
            return Err(blocked("reserve release was already attempted"));
        }
        // The API itself is one attempt, including failed preflight. Callers
        // cannot turn any error into an implicit retry of a capacity effect.
        self.state = ReleaseState::Uncertain;
        control.verify_root(self.roots.installation.root())?;
        let before = self.inspect()?;
        let record = self
            .record
            .as_ref()
            .ok_or_else(|| blocked("partial reserve has no ownership record"))?;
        if before.state == ReserveObservationState::ReleaseAttempted
            || before.logical_bytes < HEADER_BYTES as u64
            || before.allocated_bytes < record.record_allocation
            || before.allocated_bytes > MINIMUM_ABORT_BYTES + MAX_RECORD_ALLOCATION
        {
            return Err(blocked("reserve release needs reconciliation"));
        }
        // Persist uncertainty before deallocation. A failed marker write,
        // syscall, flush or readback never permits an automatic retry.
        write_state(&self.file, ATTEMPTED)?;
        if read_header(&self.file, record)? != ReserveObservationState::ReleaseAttempted {
            return Err(blocked("reserve release intent was not read back"));
        }
        self.verify_file()?;
        control.verify_root(self.roots.installation.root())?;
        // FileAllocationInfo also lowers EOF when needed. Keep the original
        // record allocation, then set the original 4 KiB logical record length.
        set_allocation(&self.file, record.record_allocation)?;
        #[cfg(test)]
        if take_fault(ReserveFault::AfterReleaseAllocation) {
            return Err(io::Error::other("injected post-release failure"));
        }
        set_eof(&self.file, HEADER_BYTES as u64)?;
        flush(&self.file)?;
        let after = self.inspect()?;
        if after.state != ReserveObservationState::ReleaseAttempted
            || after.logical_bytes != HEADER_BYTES as u64
            || after.allocated_bytes != record.record_allocation
        {
            return Err(blocked("reserve release allocation readback differs"));
        }
        control.verify_root(self.roots.installation.root())?;
        self.state = ReleaseState::Released;
        Ok(ReserveRelease {
            released_bytes: before.allocated_bytes - after.allocated_bytes,
            remaining_allocation: after.allocated_bytes,
        })
    }
}

/// Restart inspection is deliberately read-only. Even valid serialized lookup
/// evidence and actual file identity cannot mint the create-new release handle.
/// A future writable restart route needs a separate typed recovery admission.
pub(crate) struct ObservedAbortReserve {
    roots: ReserveRoots,
    file: PinnedFile,
    record: AbortReserveRecord,
}
impl ObservedAbortReserve {
    pub(crate) fn open(
        data: Arc<TransactionDataRoot>,
        installation: Arc<InstallationControl>,
        control: &ControlLease,
        reference: &AbortReserveRecord,
    ) -> Result<Self, SafeError> {
        let roots = ReserveRoots::new(data, installation, control)?;
        let name = ComponentName::new(OsStr::new(ABORT_RESERVE_FILENAME)).map_err(unavailable)?;
        let file = roots
            .data
            .root()
            .directory()
            .open_relative(
                &name,
                FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                FILE_OPEN,
                false,
                None,
            )
            .map_err(unavailable)?;
        let file = PinnedFile::from_file(roots.data.root().directory().clone(), name, file)
            .map_err(unavailable)?;
        let observed = Self {
            roots,
            file,
            record: reference.clone(),
        };
        observed.inspect()?;
        Ok(observed)
    }
    pub(crate) fn inspect(&self) -> Result<ReserveObservation, SafeError> {
        self.roots.verify().map_err(unavailable)?;
        self.file.verify().map_err(unavailable)?;
        self.roots
            .user
            .verify_private_file(handle(&self.file.file), false)
            .map_err(unavailable)?;
        self.record
            .validate(&self.roots, self.file.identity())
            .map_err(unavailable)?;
        let actual = metadata(handle(&self.file.file)).map_err(unavailable)?;
        if actual.attributes & (FILE_ATTRIBUTE_SPARSE_FILE.0 | FILE_ATTRIBUTE_COMPRESSED.0) != 0 {
            return Err(unavailable("unsupported reserve allocation"));
        }
        let state = read_header(&self.file.file, &self.record).map_err(unavailable)?;
        let (logical_bytes, allocated_bytes) = sizes(&self.file.file).map_err(unavailable)?;
        self.file.verify().map_err(unavailable)?;
        observation(&self.record, logical_bytes, allocated_bytes, state).map_err(unavailable)
    }
}

fn observation(
    record: &AbortReserveRecord,
    logical_bytes: u64,
    allocated_bytes: u64,
    state: ReserveObservationState,
) -> io::Result<ReserveObservation> {
    if !(HEADER_BYTES as u64..=record.target()).contains(&logical_bytes)
        || !(record.record_allocation..=MINIMUM_ABORT_BYTES + MAX_RECORD_ALLOCATION)
            .contains(&allocated_bytes)
        || (state == ReserveObservationState::Held
            && (logical_bytes != record.target() || allocated_bytes < record.target()))
    {
        return Err(blocked("reserve size does not match its allocation state"));
    }
    Ok(ReserveObservation {
        logical_bytes,
        allocated_bytes,
        state,
    })
}

fn create_owned(roots: ReserveRoots) -> Result<OwnedAllocation, ReserveCreateFailure> {
    let descriptor = roots
        .user
        .descriptor(false)
        .map_err(|failure| ReserveCreateFailure::before_create(unavailable(failure)))?;
    let mut units: Vec<u16> = ABORT_RESERVE_FILENAME.encode_utf16().collect();
    let name = UNICODE_STRING {
        Length: (units.len() * 2) as u16,
        MaximumLength: (units.len() * 2) as u16,
        Buffer: PWSTR(units.as_mut_ptr()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: roots.data.root().directory().raw(),
        ObjectName: &name,
        Attributes: OBJ_DONT_REPARSE,
        SecurityDescriptor: descriptor.pointer(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut status = IO_STATUS_BLOCK::default();
    let mut raw = HANDLE::default();
    let result = unsafe {
        NtCreateFile(
            &mut raw,
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            &attributes,
            &mut status,
            None,
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ,
            FILE_CREATE,
            FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_WRITE_THROUGH,
            None,
            0,
        )
    };
    if result.0 != 0 {
        return Err(ReserveCreateFailure::before_create(unavailable(
            "create-new reserve refused",
        )));
    }
    // Retain immediately after positive creation, BEFORE any fallible object or
    // ACL checks. The generic relative-open helper validates before returning
    // and consequently cannot retain this particular partial-failure evidence.
    let owned = OwnedAllocation {
        roots,
        file: File::from(unsafe { own(raw) }),
        identity: None,
        record: None,
        state: ReleaseState::Available,
    };
    if unsafe { status.Anonymous.Status.0 } != 0 || status.Information != 2 {
        return Err(ReserveCreateFailure::after_create(
            unavailable("reserve creation completion uncertain"),
            owned,
        ));
    }
    Ok(owned)
}
fn flush(file: &File) -> io::Result<()> {
    unsafe { FlushFileBuffers(handle(file)) }.map_err(win_error)
}
fn sizes(file: &File) -> io::Result<(u64, u64)> {
    let mut info = FILE_STANDARD_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle(file),
            FileStandardInfo,
            (&mut info as *mut FILE_STANDARD_INFO).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
    }
    .map_err(win_error)?;
    if info.Directory
        || info.DeletePending
        || info.NumberOfLinks != 1
        || info.EndOfFile < 0
        || info.AllocationSize < 0
    {
        return Err(blocked("unsupported reserve size or identity"));
    }
    Ok((info.EndOfFile as u64, info.AllocationSize as u64))
}
fn set_allocation(file: &File, bytes: u64) -> io::Result<()> {
    let info = FILE_ALLOCATION_INFO {
        AllocationSize: bytes as i64,
    };
    unsafe {
        SetFileInformationByHandle(
            handle(file),
            FileAllocationInfo,
            (&info as *const FILE_ALLOCATION_INFO).cast(),
            size_of::<FILE_ALLOCATION_INFO>() as u32,
        )
    }
    .map_err(win_error)
}
fn set_eof(file: &File, bytes: u64) -> io::Result<()> {
    let info = FILE_END_OF_FILE_INFO {
        EndOfFile: bytes as i64,
    };
    unsafe {
        SetFileInformationByHandle(
            handle(file),
            FileEndOfFileInfo,
            (&info as *const FILE_END_OF_FILE_INFO).cast(),
            size_of::<FILE_END_OF_FILE_INFO>() as u32,
        )
    }
    .map_err(win_error)
}
fn read_header(file: &File, record: &AbortReserveRecord) -> io::Result<ReserveObservationState> {
    let mut file = file;
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = vec![0; HEADER_BYTES];
    file.read_exact(&mut bytes)?;
    let flag = bytes[HEADER_BYTES - 1];
    if bytes != record.header(flag)? {
        return Err(blocked("reserve ownership header changed"));
    }
    match flag {
        PREPARING => Ok(ReserveObservationState::Preparing),
        HELD => Ok(ReserveObservationState::Held),
        ATTEMPTED => Ok(ReserveObservationState::ReleaseAttempted),
        _ => Err(blocked("reserve release state is uncertain")),
    }
}
fn write_state(file: &File, state: u8) -> io::Result<()> {
    let mut file = file;
    file.seek(SeekFrom::Start((HEADER_BYTES - 1) as u64))?;
    file.write_all(&[state])?;
    flush(file)
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReserveFault {
    PartialAllocation,
    AfterReleaseAllocation,
}
#[cfg(test)]
thread_local! { static RESERVE_FAULT: std::cell::Cell<Option<ReserveFault>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(crate) struct ReserveFaultGuard(std::marker::PhantomData<std::rc::Rc<()>>);
#[cfg(test)]
impl Drop for ReserveFaultGuard {
    fn drop(&mut self) {
        RESERVE_FAULT.set(None);
    }
}
#[cfg(test)]
pub(crate) fn probe_reserve_fault(fault: ReserveFault) -> ReserveFaultGuard {
    RESERVE_FAULT.with(|pending| {
        assert!(pending.get().is_none());
        pending.set(Some(fault));
    });
    ReserveFaultGuard(std::marker::PhantomData)
}
#[cfg(test)]
fn take_fault(expected: ReserveFault) -> bool {
    RESERVE_FAULT.with(|pending| {
        if pending.get() == Some(expected) {
            pending.set(None);
            true
        } else {
            false
        }
    })
}
