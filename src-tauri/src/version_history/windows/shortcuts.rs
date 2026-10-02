//! The two per-user product shortcuts are bounded opaque files, never launch
//! commands. Source guards are consumed only after immutable journal retention.
//! Restore selects an exact held known-folder parent and the fixed product leaf;
//! serialized paths, digests and booleans cannot supply destination authority.
use super::{
    blocked,
    context::verify_streams,
    files::{
        metadata, ComponentName, Directory, FileAccess, FileIdentity, PinnedFile, PrivateDirectory,
    },
    handle,
    lease::ExclusiveLease,
    security::{capture_file_descriptor, CurrentUser},
    win_error,
};
use crate::cli::types::SafeError;
use crate::version_history::journal::{
    EffectKind, EffectSpec, JournalBinding, JournalEvent, JournalPhase, JournalStore, ManifestRole,
    Observation, ObservedResult, ShortcutOperation, ShortcutSlot,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    mem::size_of,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::Path,
    sync::Arc,
};
use windows::{
    Wdk::Storage::FileSystem::{FILE_CREATE, FILE_OPEN},
    Win32::{
        Security::{
            Authorization::{SetSecurityInfo, SE_FILE_OBJECT},
            GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorGroup,
            GetSecurityDescriptorOwner, IsValidAcl, IsValidSecurityDescriptor, IsValidSid, ACL,
            DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
            PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
            SE_SELF_RELATIVE, UNPROTECTED_DACL_SECURITY_INFORMATION,
        },
        Storage::FileSystem::{
            FileBasicInfo, FileDispositionInfo, SetFileInformationByHandle, FILE_ALL_ACCESS,
            FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_NORMAL, FILE_BASIC_INFO, FILE_DISPOSITION_INFO,
            FILE_SHARE_MODE,
        },
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
    },
};

pub(crate) const MAX_SHORTCUT_BYTES: usize = 1024 * 1024;
const MAX_ARTIFACT_BYTES: usize = 12 * 1024 * 1024;
const PRODUCT_LINK: &str = "CC Desk.lnk";
const SLOTS: [ShortcutSlot; 2] = [ShortcutSlot::Desktop, ShortcutSlot::StartMenu];
fn safe<T>(result: Result<T, SafeError>) -> io::Result<T> {
    result.map_err(|_| blocked("shortcut journal evidence is unavailable"))
}
fn encode(value: &impl Serialize) -> io::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(blocked("shortcut artifact exceeds budget"));
    }
    Ok(bytes)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn index(slot: ShortcutSlot) -> usize {
    match slot {
        ShortcutSlot::Desktop => 0,
        ShortcutSlot::StartMenu => 1,
    }
}
fn leaf() -> io::Result<ComponentName> {
    ComponentName::new(OsStr::new(PRODUCT_LINK))
}
fn path_units(parent: &Directory) -> io::Result<Vec<u16>> {
    Ok(parent.path()?.encode_wide().collect())
}

/// Permissions are the exact self-relative owner/group/DACL descriptor. The
/// artifact itself always retains the recovery store's private ACL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum ShortcutState {
    Absent,
    Present {
        identity: FileIdentity,
        attributes: u32,
        bytes: Vec<u8>,
        sha256: String,
        descriptor: Vec<u8>,
    },
}
impl ShortcutState {
    fn validate(&self) -> io::Result<()> {
        if let Self::Present {
            bytes,
            sha256,
            descriptor,
            attributes,
            ..
        } = self
        {
            if bytes.len() > MAX_SHORTCUT_BYTES
                || digest(bytes) != *sha256
                || ![FILE_ATTRIBUTE_NORMAL.0, FILE_ATTRIBUTE_ARCHIVE.0].contains(attributes)
            {
                return Err(blocked("invalid shortcut content"));
            }
            descriptor_control(descriptor)?;
        }
        Ok(())
    }
    pub(crate) fn same_content_and_permissions(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Absent, Self::Absent) => true,
            (
                Self::Present {
                    bytes: a,
                    sha256: ah,
                    descriptor: ad,
                    attributes: aa,
                    ..
                },
                Self::Present {
                    bytes: b,
                    sha256: bh,
                    descriptor: bd,
                    attributes: ba,
                    ..
                },
            ) => a == b && ah == bh && ad == bd && aa == ba,
            _ => false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShortcutEntry {
    slot: ShortcutSlot,
    parent: FileIdentity,
    parent_path: Vec<u16>,
    leaf: String,
    state: ShortcutState,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShortcutManifest {
    format: u32,
    binding: JournalBinding,
    entries: [ShortcutEntry; 2],
}
impl ShortcutManifest {
    fn validate(&self, binding: &JournalBinding) -> io::Result<()> {
        if self.format != 2
            || &self.binding != binding
            || self.entries[0].parent == self.entries[1].parent
        {
            return Err(blocked("shortcut binding changed"));
        }
        for (entry, slot) in self.entries.iter().zip(SLOTS) {
            if entry.slot != slot
                || entry.leaf != PRODUCT_LINK
                || entry.parent_path.is_empty()
                || entry.parent_path.len() > 32767
            {
                return Err(blocked("invalid product shortcut manifest"));
            }
            entry.state.validate()?;
        }
        Ok(())
    }
}
struct Destination {
    parent: Arc<Directory>,
    identity: FileIdentity,
    path: Vec<u16>,
}
impl Destination {
    fn capture(parent: Arc<Directory>) -> io::Result<Self> {
        let identity = parent.identity().clone();
        let path = path_units(&parent)?;
        Ok(Self {
            parent,
            identity,
            path,
        })
    }
    fn verify(&self) -> io::Result<()> {
        self.parent.recheck()?;
        if self.parent.identity() != &self.identity || path_units(&self.parent)? != self.path {
            return Err(blocked("shortcut destination changed"));
        }
        Ok(())
    }
    fn matches(&self, entry: &ShortcutEntry) -> io::Result<()> {
        self.verify()?;
        if self.identity != entry.parent
            || self.path != entry.parent_path
            || entry.leaf != PRODUCT_LINK
        {
            return Err(blocked(
                "shortcut destination does not match retained source",
            ));
        }
        Ok(())
    }
    fn open(&self, writable: bool) -> io::Result<Option<PinnedFile>> {
        self.verify()?;
        let opened = if writable {
            self.parent
                .open_relative(
                    &leaf()?,
                    FILE_ALL_ACCESS,
                    FILE_SHARE_MODE(0),
                    FILE_OPEN,
                    false,
                    None,
                )
                .and_then(|file| PinnedFile::from_file(self.parent.clone(), leaf()?, file))
        } else {
            self.parent.open_file(leaf()?, FileAccess::Read)
        };
        match opened {
            Ok(file) => {
                if file.name != leaf()? {
                    return Err(blocked("shortcut leaf has unsupported canonical spelling"));
                }
                Ok(Some(file))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.verify()?;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
    fn entry(&self, slot: ShortcutSlot, held: &Option<PinnedFile>) -> io::Result<ShortcutEntry> {
        self.verify()?;
        let state = match held {
            Some(file) => read_state(file)?,
            None => {
                if self.open(false)?.is_some() {
                    return Err(blocked("absent shortcut changed"));
                }
                ShortcutState::Absent
            }
        };
        Ok(ShortcutEntry {
            slot,
            parent: self.identity.clone(),
            parent_path: self.path.clone(),
            leaf: PRODUCT_LINK.into(),
            state,
        })
    }
}
fn read_state(file: &PinnedFile) -> io::Result<ShortcutState> {
    file.verify()?;
    let before = metadata(handle(&file.file))?;
    if before.size > MAX_SHORTCUT_BYTES as u64
        || before.attributes & !(FILE_ATTRIBUTE_NORMAL.0 | FILE_ATTRIBUTE_ARCHIVE.0) != 0
    {
        return Err(blocked("unsupported shortcut size or attributes"));
    }
    verify_streams(handle(&file.file), &before)?;
    let descriptor = capture_file_descriptor(handle(&file.file))?;
    let mut reader = &file.file;
    reader.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    reader
        .take(MAX_SHORTCUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    file.verify()?;
    if bytes.len() as u64 != before.size
        || metadata(handle(&file.file))? != before
        || capture_file_descriptor(handle(&file.file))? != descriptor
    {
        return Err(blocked("shortcut changed during capture"));
    }
    let state = ShortcutState::Present {
        identity: before.identity,
        attributes: before.attributes,
        sha256: digest(&bytes),
        bytes,
        descriptor,
    };
    state.validate()?;
    Ok(state)
}

fn current_destinations() -> io::Result<[Arc<Directory>; 2]> {
    #[cfg(test)]
    if let Some(parents) =
        RESOLVER.with(|state| state.borrow().as_ref().map(|state| state.current.clone()))
    {
        return Ok(parents);
    }
    CurrentUser::capture()?.require_unelevated()?;
    fn known_folder(id: &windows_core::GUID) -> io::Result<Arc<Directory>> {
        // The API uses the current token and actual redirected known-folder
        // location. No environment substitution, path guess or folder creation.
        let value =
            unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.map_err(win_error)?;
        struct Allocation(windows_core::PWSTR);
        impl Drop for Allocation {
            fn drop(&mut self) {
                unsafe {
                    CoTaskMemFree(Some(self.0 .0.cast()));
                }
            }
        }
        let allocation = Allocation(value);
        if allocation.0 .0.is_null() {
            return Err(blocked("known folder has no location"));
        }
        let mut length = 0;
        while length < 32768 && unsafe { *allocation.0 .0.add(length) } != 0 {
            length += 1;
        }
        if length == 0 || length == 32768 {
            return Err(blocked("known folder path exceeds budget"));
        }
        let path =
            OsString::from_wide(unsafe { std::slice::from_raw_parts(allocation.0 .0, length) });
        Directory::open_absolute(Path::new(&path))
    }
    Ok([
        known_folder(&FOLDERID_Desktop)?,
        known_folder(&FOLDERID_Programs)?,
    ])
}

#[derive(Clone, Copy)]
enum DestinationSource {
    KnownFolders,
    #[cfg(test)]
    Fixture,
}
impl DestinationSource {
    fn verify_entry(self, entry: &ShortcutEntry) -> io::Result<()> {
        match self {
            Self::KnownFolders => {
                let current = current_destinations()?;
                let parent = &current[index(entry.slot)];
                if parent.identity() != &entry.parent || path_units(parent)? != entry.parent_path {
                    return Err(blocked("current known-folder destination changed"));
                }
            }
            #[cfg(test)]
            Self::Fixture => (),
        }
        Ok(())
    }
    fn verify(self, destinations: &[Destination; 2]) -> io::Result<()> {
        match self {
            Self::KnownFolders => {
                let current = current_destinations()?;
                for i in 0..2 {
                    if current[i].identity() != &destinations[i].identity
                        || path_units(&current[i])? != destinations[i].path
                    {
                        return Err(blocked("current known-folder destination changed"));
                    }
                }
            }
            #[cfg(test)]
            Self::Fixture => (),
        }
        Ok(())
    }
}

pub(crate) struct HeldProductShortcuts {
    source: DestinationSource,
    destinations: [Destination; 2],
    held: [Option<PinnedFile>; 2],
    entries: [ShortcutEntry; 2],
}
impl HeldProductShortcuts {
    pub(crate) fn capture_current_user() -> io::Result<Self> {
        Self::capture(current_destinations()?, DestinationSource::KnownFolders)
    }
    fn capture(parents: [Arc<Directory>; 2], source: DestinationSource) -> io::Result<Self> {
        let [desktop, programs] = parents;
        if desktop.identity() == programs.identity() {
            return Err(blocked("product shortcut destinations overlap"));
        }
        let destinations = [
            Destination::capture(desktop)?,
            Destination::capture(programs)?,
        ];
        let held = [destinations[0].open(false)?, destinations[1].open(false)?];
        let entries = [
            destinations[0].entry(SLOTS[0], &held[0])?,
            destinations[1].entry(SLOTS[1], &held[1])?,
        ];
        let capture = Self {
            source,
            destinations,
            held,
            entries,
        };
        capture.verify()?;
        Ok(capture)
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.source.verify(&self.destinations)?;
        for (i, slot) in SLOTS.iter().enumerate() {
            if self.destinations[i].entry(*slot, &self.held[i])? != self.entries[i] {
                return Err(blocked("captured shortcut changed"));
            }
        }
        Ok(())
    }
    pub(crate) fn state(&self, slot: ShortcutSlot) -> &ShortcutState {
        &self.entries[index(slot)].state
    }
    /// Complete current shortcut observation for a private-only abort. This
    /// creates no retained Registration/Shortcuts role or restoration authority.
    pub(crate) fn encode(&self) -> io::Result<Vec<u8>> {
        self.verify()?;
        let bytes = encode(&self.entries)?;
        self.verify()?;
        Ok(bytes)
    }
    pub(crate) fn retain(
        self,
        journal: &mut ShortcutJournal<'_>,
    ) -> io::Result<RetainedProductShortcuts> {
        self.verify()?;
        journal.require_phase(JournalPhase::Reviewed)?;
        let manifest = ShortcutManifest {
            format: 2,
            binding: journal.binding.clone(),
            entries: self.entries.clone(),
        };
        manifest.validate(&journal.binding)?;
        let digest = journal.retain(&manifest)?;
        self.verify()?;
        journal.append(JournalEvent::Manifest {
            role: ManifestRole::Shortcuts,
            digest: digest.clone(),
        })?;
        self.verify()?;
        let Self {
            source,
            destinations,
            held,
            ..
        } = self;
        drop(held); // Only the durable private artifact now owns source evidence.
        Ok(RetainedProductShortcuts {
            source,
            manifest,
            digest,
            destinations,
        })
    }
    #[cfg(test)]
    pub(crate) fn capture_at(parents: [Arc<Directory>; 2]) -> io::Result<Self> {
        Self::capture(parents, DestinationSource::Fixture)
    }
}

pub(crate) struct RetainedProductShortcuts {
    source: DestinationSource,
    manifest: ShortcutManifest,
    digest: String,
    destinations: [Destination; 2],
}
impl RetainedProductShortcuts {
    /// Re-observe both original product slots immediately before installation.
    /// Only this retained capability selects the resolver and held parents;
    /// callers cannot substitute states, paths or digests. Temporary file read
    /// guards are released on both success and failure before returning, so a
    /// successful check cannot itself prevent the installer replacing a link.
    pub(crate) fn verify_original(&self) -> io::Result<()> {
        self.manifest.validate(&self.manifest.binding)?;
        self.source.verify(&self.destinations)?;
        for i in 0..2 {
            self.destinations[i].matches(&self.manifest.entries[i])?;
        }
        let observed = HeldProductShortcuts::capture(
            [
                self.destinations[0].parent.clone(),
                self.destinations[1].parent.clone(),
            ],
            self.source,
        )?;
        if observed.entries != self.manifest.entries {
            return Err(blocked("original product shortcut state changed"));
        }
        // Recheck exact state and the live resolver while both temporary source
        // readers are held. Entry equality includes identity, bytes/hash,
        // attributes, owner/group/DACL and actual absence in the original parent.
        observed.verify()?;
        drop(observed);
        Ok(())
    }
    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }
    pub(crate) fn state(&self, slot: ShortcutSlot) -> &ShortcutState {
        &self.manifest.entries[index(slot)].state
    }
    pub(crate) fn reopen_current_user(
        store: &JournalStore,
        binding: &JournalBinding,
        digest: &str,
    ) -> io::Result<Self> {
        Self::reopen(
            store,
            binding,
            digest,
            current_destinations()?,
            DestinationSource::KnownFolders,
        )
    }
    fn reopen(
        store: &JournalStore,
        binding: &JournalBinding,
        digest: &str,
        parents: [Arc<Directory>; 2],
        source: DestinationSource,
    ) -> io::Result<Self> {
        verify_registered_manifest(store, binding, digest)?;
        let bytes = safe(store.read_manifest(digest))?;
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(blocked("shortcut artifact exceeds budget"));
        }
        let manifest: ShortcutManifest =
            serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        manifest.validate(binding)?;
        let [desktop, programs] = parents;
        let destinations = [
            Destination::capture(desktop)?,
            Destination::capture(programs)?,
        ];
        for (destination, entry) in destinations.iter().zip(&manifest.entries) {
            destination.matches(entry)?;
        }
        source.verify(&destinations)?;
        Ok(Self {
            source,
            manifest,
            digest: digest.into(),
            destinations,
        })
    }
    #[cfg(test)]
    pub(crate) fn reopen_at(
        store: &JournalStore,
        binding: &JournalBinding,
        digest: &str,
        parents: [Arc<Directory>; 2],
    ) -> io::Result<Self> {
        Self::reopen(store, binding, digest, parents, DestinationSource::Fixture)
    }

    /// Every invocation begins at ordinal zero, including after process restart.
    /// The durable first intent ID is stable for this transaction and slot, so
    /// a changed plan/current file cannot replay or resume an attempted restore.
    pub(crate) fn restore(
        &self,
        slot: ShortcutSlot,
        journal: &mut ShortcutJournal<'_>,
    ) -> io::Result<ShortcutRestoreReceipt> {
        journal.require_phase(JournalPhase::Restoring)?;
        self.source.verify(&self.destinations)?;
        self.manifest.validate(&journal.binding)?;
        verify_registered_manifest(journal.store, &journal.binding, &self.digest)?;
        if safe(journal.store.read_manifest(&self.digest))? != encode(&self.manifest)? {
            return Err(blocked("retained shortcut source changed"));
        }
        let destination = &self.destinations[index(slot)];
        let source = &self.manifest.entries[index(slot)];
        destination.matches(source)?;
        let mut held = destination.open(true)?;
        let current = destination.entry(slot, &held)?;
        // Retain every current state, even a matching/no-op state; no live file
        // is changed before this immutable exact bytes/security evidence exists.
        let preserved_current = journal.retain(&current)?;
        if destination.entry(slot, &held)? != current {
            return Err(blocked("shortcut conflict changed before preservation"));
        }
        // Include every separate create/write/attribute/permission/verification
        // effect before changing the first live byte. Current conflict evidence
        // is already retained even if the actual remaining reserve is too small.
        let changed = !source.state.same_content_and_permissions(&current.state);
        let effects = if !changed {
            1
        } else {
            match &source.state {
                ShortcutState::Absent => 2,
                ShortcutState::Present { .. } => {
                    if held.is_none() {
                        5
                    } else {
                        4
                    }
                }
            }
        };
        journal.verify()?;
        safe(journal.store.admit_restore_capacity(
            journal.generation,
            effects,
            effects as usize * 4,
        ))?;
        journal.verify()?;
        let mut ordinal = 0;
        if changed {
            match &source.state {
                ShortcutState::Absent => {
                    let pending = journal.begin(
                        slot,
                        &self.digest,
                        &mut ordinal,
                        Some(ShortcutOperation::RemoveFile),
                        &current,
                        &source.state,
                    )?;
                    let result = (|| {
                        if destination.entry(slot, &held)? != current {
                            return Err(blocked("shortcut conflict changed before removal"));
                        }
                        let file = held
                            .as_ref()
                            .ok_or_else(|| blocked("shortcut removal lost its held file"))?;
                        let delete = FILE_DISPOSITION_INFO { DeleteFile: true };
                        unsafe {
                            SetFileInformationByHandle(
                                handle(&file.file),
                                FileDispositionInfo,
                                (&delete as *const FILE_DISPOSITION_INFO).cast(),
                                size_of::<FILE_DISPOSITION_INFO>() as u32,
                            )
                        }
                        .map_err(win_error)?;
                        file.file.sync_all()?;
                        drop(held.take());
                        fault(ShortcutFault::AfterRemove)?;
                        destination.entry(slot, &held)
                    })();
                    journal.finish(pending, result)?;
                }
                ShortcutState::Present {
                    bytes,
                    descriptor,
                    attributes,
                    ..
                } => {
                    if held.is_none() {
                        let pending = journal.begin(
                            slot,
                            &self.digest,
                            &mut ordinal,
                            Some(ShortcutOperation::CreateFile),
                            &current,
                            &"new private empty product link",
                        )?;
                        let result = (|| {
                            if destination.entry(slot, &held)? != current {
                                return Err(blocked("shortcut appeared before create"));
                            }
                            let user = CurrentUser::capture()?;
                            let security = user.descriptor(false)?;
                            let file = destination.parent.open_relative(
                                &leaf()?,
                                FILE_ALL_ACCESS,
                                FILE_SHARE_MODE(0),
                                FILE_CREATE,
                                false,
                                Some(&security),
                            )?;
                            file.sync_all()?;
                            held = Some(PinnedFile::from_file(
                                destination.parent.clone(),
                                leaf()?,
                                file,
                            )?);
                            destination.entry(slot, &held)
                        })();
                        journal.finish(pending, result)?;
                    }
                    let before = destination.entry(slot, &held)?;
                    let pending = journal.begin(
                        slot,
                        &self.digest,
                        &mut ordinal,
                        Some(ShortcutOperation::WriteBytes),
                        &before,
                        &BytesExpected {
                            sha256: digest(bytes),
                            length: bytes.len(),
                        },
                    )?;
                    let result = (|| {
                        if destination.entry(slot, &held)? != before {
                            return Err(blocked("shortcut changed before content write"));
                        }
                        let file = held
                            .as_ref()
                            .ok_or_else(|| blocked("shortcut write lost its held file"))?;
                        let mut writer = &file.file;
                        writer.seek(SeekFrom::Start(0))?;
                        if fault_active(ShortcutFault::PartialWrite) {
                            writer.write_all(&bytes[..bytes.len().div_ceil(2)])?;
                            file.file.sync_all()?;
                            return Err(blocked("injected partial shortcut write"));
                        }
                        writer.write_all(bytes)?;
                        file.file.set_len(bytes.len() as u64)?;
                        file.file.sync_all()?;
                        fault(ShortcutFault::AfterWrite)?;
                        let observed = destination.entry(slot, &held)?;
                        match &observed.state {
                            ShortcutState::Present { bytes: actual, .. } if actual == bytes => {
                                Ok(observed)
                            }
                            _ => Err(blocked("shortcut content readback changed")),
                        }
                    })();
                    journal.finish(pending, result)?;
                    let before = destination.entry(slot, &held)?;
                    let mut expected = before.clone();
                    let ShortcutState::Present {
                        attributes: expected_attributes,
                        ..
                    } = &mut expected.state
                    else {
                        return Err(blocked("shortcut attributes lost their held file"));
                    };
                    *expected_attributes = *attributes;
                    let pending = journal.begin(
                        slot,
                        &self.digest,
                        &mut ordinal,
                        Some(ShortcutOperation::SetAttributes),
                        &before,
                        &expected,
                    )?;
                    let result = (|| {
                        if destination.entry(slot, &held)? != before {
                            return Err(blocked("shortcut changed before attribute write"));
                        }
                        let file = held
                            .as_ref()
                            .ok_or_else(|| blocked("shortcut attributes lost their held file"))?;
                        let basic = FILE_BASIC_INFO {
                            FileAttributes: *attributes,
                            ..Default::default()
                        };
                        // Zero timestamps leave those fields unchanged. Only the
                        // exact admitted NORMAL/ARCHIVE flags are written here.
                        unsafe {
                            SetFileInformationByHandle(
                                handle(&file.file),
                                FileBasicInfo,
                                (&basic as *const FILE_BASIC_INFO).cast(),
                                size_of::<FILE_BASIC_INFO>() as u32,
                            )
                        }
                        .map_err(win_error)?;
                        file.file.sync_all()?;
                        fault(ShortcutFault::AfterAttributes)?;
                        let observed = destination.entry(slot, &held)?;
                        if observed != expected {
                            return Err(blocked("shortcut attribute readback changed"));
                        }
                        Ok(observed)
                    })();
                    journal.finish_checked(pending, result, || {
                        self.source.verify_entry(&expected)?;
                        if destination.entry(slot, &held)? != expected {
                            return Err(blocked("shortcut attributes changed before Applied"));
                        }
                        self.source.verify_entry(&expected)
                    })?;
                    let before = destination.entry(slot, &held)?;
                    let pending = journal.begin(
                        slot,
                        &self.digest,
                        &mut ordinal,
                        Some(ShortcutOperation::SetPermissions),
                        &before,
                        &source.state,
                    )?;
                    let result = (|| {
                        if destination.entry(slot, &held)? != before {
                            return Err(blocked("shortcut changed before permissions write"));
                        }
                        let file = held
                            .as_ref()
                            .ok_or_else(|| blocked("shortcut permissions lost their held file"))?;
                        apply_descriptor(&file.file, descriptor)?;
                        file.file.sync_all()?;
                        fault(ShortcutFault::AfterPermissions)?;
                        let observed = destination.entry(slot, &held)?;
                        if !observed.state.same_content_and_permissions(&source.state) {
                            return Err(blocked("shortcut permission readback changed"));
                        }
                        Ok(observed)
                    })();
                    journal.finish(pending, result)?;
                }
            }
        }
        // Reacquire a read guard for downstream final verification; the actual
        // object identity and state must survive this guarded access transition.
        let before = destination.entry(slot, &held)?;
        drop(held.take());
        held = destination.open(false)?;
        let observed = destination.entry(slot, &held)?;
        if observed != before || !observed.state.same_content_and_permissions(&source.state) {
            return Err(blocked("shortcut changed at final verification"));
        }
        let pending = journal.begin(
            slot,
            &self.digest,
            &mut ordinal,
            None,
            &observed,
            &source.state,
        )?;
        let verified = destination.entry(slot, &held)?;
        if verified != observed {
            let _ = journal.unknown(pending);
            return Err(blocked("shortcut final observation changed"));
        }
        journal.finish_checked(pending, Ok(&verified), || {
            #[cfg(test)]
            apply_resolver_final_change();
            self.source.verify_entry(&verified)?;
            if destination.entry(slot, &held)? != verified {
                return Err(blocked("shortcut changed before final Applied"));
            }
            self.source.verify_entry(&verified)
        })?;
        Ok(ShortcutRestoreReceipt {
            source: self.source,
            preserved_current,
            entry: verified,
            parent: destination.parent.clone(),
            held,
        })
    }
}
#[derive(Serialize)]
struct BytesExpected {
    sha256: String,
    length: usize,
}

pub(crate) struct ShortcutRestoreReceipt {
    source: DestinationSource,
    preserved_current: String,
    entry: ShortcutEntry,
    parent: Arc<Directory>,
    held: Option<PinnedFile>,
}
impl ShortcutRestoreReceipt {
    pub(crate) fn preserved_current(&self) -> &str {
        &self.preserved_current
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.source.verify_entry(&self.entry)?;
        let destination = Destination::capture(self.parent.clone())?;
        destination.matches(&self.entry)?;
        if destination.entry(self.entry.slot, &self.held)? != self.entry {
            return Err(blocked("restored shortcut changed"));
        }
        self.source.verify_entry(&self.entry)
    }
}
fn verify_registered_manifest(
    store: &JournalStore,
    binding: &JournalBinding,
    digest: &str,
) -> io::Result<()> {
    let inspection = safe(store.inspect(binding))?;
    let journal = inspection
        .last_valid
        .as_ref()
        .ok_or_else(|| blocked("shortcut journal is missing"))?;
    if inspection.blocked || journal.manifest(ManifestRole::Shortcuts) != Some(digest) {
        return Err(blocked("shortcut artifact is not journal-bound"));
    }
    Ok(())
}

pub(crate) struct ShortcutJournal<'a> {
    store: &'a mut JournalStore,
    root: Arc<PrivateDirectory>,
    lease: &'a ExclusiveLease,
    binding: JournalBinding,
    generation: u64,
}
struct Pending {
    id: String,
    generation: u64,
}
impl<'a> ShortcutJournal<'a> {
    pub(crate) fn new(
        store: &'a mut JournalStore,
        root: Arc<PrivateDirectory>,
        lease: &'a ExclusiveLease,
        binding: JournalBinding,
        generation: u64,
    ) -> io::Result<Self> {
        let mut result = Self {
            store,
            root,
            lease,
            binding,
            generation,
        };
        result.verify()?;
        Ok(result)
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
    fn verify(&mut self) -> io::Result<()> {
        self.lease.verify_root(&self.root)?;
        self.root.verify(&CurrentUser::capture()?)?;
        safe(
            self.store
                .verify_windows_binding(&self.root, &self.binding, self.generation),
        )
    }
    fn require_phase(&mut self, phase: JournalPhase) -> io::Result<()> {
        self.verify()?;
        let inspection = safe(self.store.inspect(&self.binding))?;
        let state = inspection
            .last_valid
            .ok_or_else(|| blocked("shortcut journal is missing"))?;
        if inspection.blocked || state.phase() != phase || state.requires_reconciliation() {
            return Err(blocked("shortcut journal does not admit this phase"));
        }
        Ok(())
    }
    fn retain(&mut self, value: &impl Serialize) -> io::Result<String> {
        self.verify()?;
        let bytes = encode(value)?;
        let digest = safe(self.store.retain_manifest(&bytes))?;
        if safe(self.store.read_manifest(&digest))? != bytes {
            return Err(blocked("shortcut artifact readback changed"));
        }
        self.verify()?;
        Ok(digest)
    }
    fn append(&mut self, event: JournalEvent) -> io::Result<()> {
        self.verify()?;
        self.generation = safe(self.store.append(self.generation, event))?;
        self.verify()
    }
    fn begin(
        &mut self,
        slot: ShortcutSlot,
        manifest: &str,
        ordinal: &mut u32,
        operation: Option<ShortcutOperation>,
        before: &impl Serialize,
        expected: &impl Serialize,
    ) -> io::Result<Pending> {
        self.require_phase(JournalPhase::Restoring)?;
        let before = self.retain(before)?;
        let expected_postconditions = self.retain(expected)?;
        let id = effect_id(&self.binding, slot, *ordinal);
        let kind = match operation {
            Some(operation) => EffectKind::RecoveryShortcutEntry {
                slot,
                operation,
                manifest: manifest.into(),
                entry_index: index(slot) as u32,
            },
            None => EffectKind::RestoreShortcut { slot },
        };
        self.append(JournalEvent::Intent {
            effect: EffectSpec {
                effect_id: id.clone(),
                kind,
                before,
                expected_postconditions,
            },
        })?;
        *ordinal += 1;
        Ok(Pending {
            id,
            generation: self.generation,
        })
    }
    fn unknown(&mut self, pending: Pending) -> io::Result<()> {
        self.append(JournalEvent::Observed {
            effect_id: pending.id,
            intent_generation: pending.generation,
            result: ObservedResult {
                observation: Observation::Unknown,
                receipt: None,
            },
        })
    }
    fn finish<T: Serialize>(&mut self, pending: Pending, result: io::Result<T>) -> io::Result<()> {
        self.finish_checked(pending, result, || Ok(()))
    }
    fn finish_checked<T: Serialize>(
        &mut self,
        pending: Pending,
        result: io::Result<T>,
        verify: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        let observed = match result {
            Ok(observed) => observed,
            Err(error) => {
                let _ = self.unknown(pending);
                return Err(error);
            }
        };
        // Any persistence failure leaves the existing intent pending. No second
        // attempt is made even when the OS effect or first flush succeeded.
        let observed = self.retain(&observed)?;
        let receipt = safe(self.store.retain_effect_receipt(
            &pending.id,
            Observation::Applied,
            &observed,
        ))?;
        // The final product-link observation also revalidates the live resolver
        // immediately before Applied; retained old-folder handles are not proof
        // that the current known-folder mapping still selects those objects.
        if let Err(error) = verify() {
            let _ = self.unknown(pending);
            return Err(error);
        }
        self.append(JournalEvent::Observed {
            effect_id: pending.id,
            intent_generation: pending.generation,
            result: ObservedResult {
                observation: Observation::Applied,
                receipt: Some(receipt),
            },
        })
    }
}
fn effect_id(binding: &JournalBinding, slot: ShortcutSlot, ordinal: u32) -> String {
    let mut hash = Sha256::new();
    hash.update(b"cc-desk-product-shortcut-restore-v1\0");
    hash.update(binding.transaction_id.as_bytes());
    hash.update([index(slot) as u8]);
    hash.update(ordinal.to_le_bytes());
    let hash = hash.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}
fn descriptor_control(bytes: &[u8]) -> io::Result<u16> {
    if !(20..=65536).contains(&bytes.len()) {
        return Err(blocked("unsupported shortcut descriptor"));
    }
    // Align the untrusted retained byte representation before Win32 inspects it.
    let mut words = vec![0u32; bytes.len().div_ceil(4)];
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), words.as_mut_ptr().cast::<u8>(), bytes.len());
    }
    let descriptor = PSECURITY_DESCRIPTOR(words.as_mut_ptr().cast());
    // GetSecurityDescriptorLength does not bound offset pointers against our
    // allocation. Validate self-relative offsets/ACL lengths before API calls.
    validate_descriptor_layout(bytes)?;
    if !unsafe { IsValidSecurityDescriptor(descriptor) }.as_bool() {
        return Err(blocked("invalid shortcut descriptor"));
    }
    let mut control = 0;
    let mut revision = 0;
    unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) }
        .map_err(win_error)?;
    if control & SE_SELF_RELATIVE.0 == 0 {
        return Err(blocked("shortcut descriptor is not self-relative"));
    }
    Ok(control)
}
fn validate_descriptor_layout(bytes: &[u8]) -> io::Result<()> {
    let invalid = || blocked("invalid shortcut descriptor offsets");
    if bytes.len() < 20
        || bytes[0] != 1
        || u16::from_le_bytes([bytes[2], bytes[3]]) & SE_SELF_RELATIVE.0 == 0
    {
        return Err(invalid());
    }
    for (field, acl) in [(4, false), (8, false), (12, true), (16, true)] {
        let offset =
            u32::from_le_bytes(bytes[field..field + 4].try_into().map_err(|_| invalid())?) as usize;
        if offset == 0 {
            if field == 4 || field == 8 || field == 16 {
                return Err(invalid());
            }
            continue;
        }
        if offset < 20
            || !offset.is_multiple_of(4)
            || offset.checked_add(8).is_none_or(|end| end > bytes.len())
        {
            return Err(invalid());
        }
        let length = if acl {
            u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize
        } else {
            8 + bytes[offset + 1] as usize * 4
        };
        if length < 8
            || offset
                .checked_add(length)
                .is_none_or(|end| end > bytes.len())
        {
            return Err(invalid());
        }
    }
    Ok(())
}
fn apply_descriptor(file: &File, bytes: &[u8]) -> io::Result<()> {
    let control = descriptor_control(bytes)?;
    let mut words = vec![0u32; bytes.len().div_ceil(4)];
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), words.as_mut_ptr().cast::<u8>(), bytes.len());
    }
    let descriptor = PSECURITY_DESCRIPTOR(words.as_mut_ptr().cast());
    let mut owner = PSID::default();
    let mut group = PSID::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut defaulted = windows_core::BOOL::default();
    let mut present = windows_core::BOOL::default();
    unsafe {
        GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted).map_err(win_error)?;
        GetSecurityDescriptorGroup(descriptor, &mut group, &mut defaulted).map_err(win_error)?;
        GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)
            .map_err(win_error)?;
    }
    // Layout validation above bounds each self-relative SID/ACL before Win32
    // sees the descriptor. Require the accessor pointers to designate those
    // exact bounded fields; reject missing or NULL DACLs instead of granting
    // broader access. The aligned owned storage lives through SetSecurityInfo.
    let base = words.as_mut_ptr().cast::<u8>();
    let field = |offset: usize| -> *mut core::ffi::c_void {
        let relative = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
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
        return Err(blocked("unsupported shortcut owner/group/DACL"));
    }
    let protected = if control & SE_DACL_PROTECTED.0 != 0 {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    unsafe {
        SetSecurityInfo(
            handle(file),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | GROUP_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | protected,
            Some(owner),
            Some(group),
            Some(dacl.cast_const()),
            None,
        )
        .ok()
    }
    .map_err(win_error)?;
    if capture_file_descriptor(handle(file))? != bytes {
        return Err(blocked("shortcut owner/group/DACL readback changed"));
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortcutFault {
    PartialWrite,
    AfterWrite,
    AfterAttributes,
    AfterPermissions,
    AfterRemove,
}
#[cfg(test)]
thread_local! { static FAULT: std::cell::Cell<Option<ShortcutFault>> = const { std::cell::Cell::new(None) }; }
fn fault_active(at: ShortcutFault) -> bool {
    #[cfg(test)]
    {
        FAULT.with(|fault| fault.get() == Some(at))
    }
    #[cfg(not(test))]
    {
        let _ = at;
        false
    }
}
fn fault(at: ShortcutFault) -> io::Result<()> {
    if fault_active(at) {
        Err(blocked("injected shortcut observation loss"))
    } else {
        Ok(())
    }
}
#[cfg(test)]
pub(crate) struct ShortcutProbe(std::marker::PhantomData<std::rc::Rc<()>>);
#[cfg(test)]
impl Drop for ShortcutProbe {
    fn drop(&mut self) {
        FAULT.with(|fault| fault.set(None));
    }
}
#[cfg(test)]
pub(crate) fn probe_shortcut_fault(fault: ShortcutFault) -> ShortcutProbe {
    FAULT.with(|current| {
        assert!(current.get().is_none());
        current.set(Some(fault));
    });
    ShortcutProbe(std::marker::PhantomData)
}

// Disposable resolver injection exercises the production KnownFolders binding
// without changing the current user's Desktop/Programs configuration.
#[cfg(test)]
struct ResolverState {
    current: [Arc<Directory>; 2],
    before_final: Option<(ShortcutSlot, Arc<Directory>)>,
}
#[cfg(test)]
thread_local! { static RESOLVER: std::cell::RefCell<Option<ResolverState>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(crate) struct ShortcutResolverProbe(std::marker::PhantomData<std::rc::Rc<()>>);
#[cfg(test)]
impl ShortcutResolverProbe {
    pub(crate) fn replace(&self, slot: ShortcutSlot, parent: Arc<Directory>) {
        RESOLVER.with(|state| state.borrow_mut().as_mut().unwrap().current[index(slot)] = parent);
    }
    pub(crate) fn replace_before_final(&self, slot: ShortcutSlot, parent: Arc<Directory>) {
        RESOLVER
            .with(|state| state.borrow_mut().as_mut().unwrap().before_final = Some((slot, parent)));
    }
}
#[cfg(test)]
impl Drop for ShortcutResolverProbe {
    fn drop(&mut self) {
        RESOLVER.with(|state| *state.borrow_mut() = None);
    }
}
#[cfg(test)]
pub(crate) fn probe_shortcut_resolver(parents: [Arc<Directory>; 2]) -> ShortcutResolverProbe {
    RESOLVER.with(|state| {
        let mut state = state.borrow_mut();
        assert!(state.is_none());
        *state = Some(ResolverState {
            current: parents,
            before_final: None,
        });
    });
    ShortcutResolverProbe(std::marker::PhantomData)
}
#[cfg(test)]
fn apply_resolver_final_change() {
    RESOLVER.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(state) = state.as_mut() {
            if let Some((slot, parent)) = state.before_final.take() {
                state.current[index(slot)] = parent;
            }
        }
    });
}
