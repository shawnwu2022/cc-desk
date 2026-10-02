//! Parent-handle-relative NTFS traversal. No ambient path is used below a held
//! volume root. Every component is opened with OBJ_DONT_REPARSE, then inspected.
use super::{blocked, handle, own, security::CurrentUser, win_error};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    mem::{offset_of, size_of},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path},
    sync::Arc,
};
use windows::Wdk::{
    Foundation::OBJECT_ATTRIBUTES,
    Storage::FileSystem::{
        FileRenameInformation, NtCreateFile, NtSetInformationFile, FILE_CREATE,
        FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_RENAME_INFORMATION,
        FILE_SYNCHRONOUS_IO_NONALERT, FILE_WRITE_THROUGH, NTCREATEFILE_CREATE_DISPOSITION,
    },
};
use windows::Win32::{
    Foundation::{ERROR_NO_MORE_FILES, HANDLE, OBJ_DONT_REPARSE, UNICODE_STRING},
    Storage::FileSystem::{
        CreateFileW, FileAttributeTagInfo, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
        FileIdInfo, FileStandardInfo, GetDriveTypeW, GetFileInformationByHandleEx, GetFileType,
        GetFinalPathNameByHandleW, GetVolumeInformationByHandleW, DELETE, FILE_ACCESS_RIGHTS,
        FILE_ALL_ACCESS, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_BOTH_DIR_INFO, FILE_ID_INFO, FILE_LIST_DIRECTORY,
        FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FILE_STANDARD_INFO, FILE_TRAVERSE, FILE_TYPE_DISK, FILE_WRITE_DATA, OPEN_EXISTING,
        READ_CONTROL, SYNCHRONIZE, VOLUME_NAME_GUID,
    },
    System::IO::IO_STATUS_BLOCK,
};
use windows_core::{PCWSTR, PWSTR};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComponentName(Vec<u16>);
impl ComponentName {
    pub(crate) fn new(name: &OsStr) -> io::Result<Self> {
        let units: Vec<_> = name.encode_wide().collect();
        let text = String::from_utf16(&units).map_err(|_| blocked("unrepresentable file name"))?;
        let base = text
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let device = matches!(
            base.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            base.strip_prefix(*prefix).is_some_and(|n| {
                matches!(
                    n,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
        if units.is_empty()
            || units.len() > 255
            || text == "."
            || text == ".."
            || text.ends_with(['.', ' '])
            || text
                .chars()
                .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
            || device
        {
            return Err(blocked("unsupported file component"));
        }
        Ok(Self(units))
    }
    pub(crate) fn os_string(&self) -> OsString {
        OsString::from_wide(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileIdentity {
    volume: u64,
    id: [u8; 16],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Metadata {
    pub(crate) identity: FileIdentity,
    pub(crate) size: u64,
    pub(crate) directory: bool,
    pub(crate) attributes: u32,
}
pub(super) fn metadata(file: HANDLE) -> io::Result<Metadata> {
    unsafe {
        if GetFileType(file) != FILE_TYPE_DISK {
            return Err(blocked("recovery requires disk objects"));
        }
        let mut id = FILE_ID_INFO::default();
        let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
        let mut standard = FILE_STANDARD_INFO::default();
        GetFileInformationByHandleEx(
            file,
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
        .map_err(win_error)?;
        GetFileInformationByHandleEx(
            file,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
        .map_err(win_error)?;
        GetFileInformationByHandleEx(
            file,
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
        .map_err(win_error)?;
        let directory = standard.Directory;
        if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || tag.ReparseTag != 0
            || standard.DeletePending
            || standard.EndOfFile < 0
            || (!directory && standard.NumberOfLinks != 1)
            || directory != (tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0)
        {
            return Err(blocked(
                "linked, pending-delete or unsupported recovery object",
            ));
        }
        Ok(Metadata {
            identity: FileIdentity {
                volume: id.VolumeSerialNumber,
                id: id.FileId.Identifier,
            },
            size: standard.EndOfFile as u64,
            directory,
            attributes: tag.FileAttributes,
        })
    }
}
pub(super) fn final_path(file: HANDLE) -> io::Result<Vec<u16>> {
    let mut buffer = vec![0u16; 32768];
    let length = unsafe { GetFinalPathNameByHandleW(file, &mut buffer, VOLUME_NAME_GUID) } as usize;
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length >= buffer.len() {
        return Err(blocked("unsupported recovery path length"));
    }
    buffer.truncate(length);
    String::from_utf16(&buffer).map_err(|_| blocked("unrepresentable recovery path"))?;
    if !buffer.starts_with(&"\\\\?\\Volume{".encode_utf16().collect::<Vec<_>>()) {
        return Err(blocked("recovery requires a local volume identity"));
    }
    Ok(buffer)
}
fn verify_ntfs(file: HANDLE) -> io::Result<()> {
    let mut filesystem = [0u16; 32];
    unsafe {
        GetVolumeInformationByHandleW(file, None, None, None, None, Some(&mut filesystem))
            .map_err(win_error)?;
    }
    let length = filesystem
        .iter()
        .position(|c| *c == 0)
        .ok_or_else(|| blocked("invalid filesystem identity"))?;
    if &filesystem[..length] != "NTFS".encode_utf16().collect::<Vec<_>>().as_slice() {
        return Err(blocked("only local NTFS is supported"));
    }
    final_path(file)?;
    Ok(())
}

#[derive(Clone)]
struct Location {
    parent: Arc<Directory>,
    name: ComponentName,
}
pub(crate) struct Directory {
    file: File,
    identity: FileIdentity,
    location: Mutex<Option<Location>>,
    // Only the volume root is initially opened by absolute path.
    volume_path: Vec<u16>,
    rename_access: bool,
}
impl Directory {
    pub(crate) fn open_absolute(path: &Path) -> io::Result<Arc<Self>> {
        let units: Vec<_> = path.as_os_str().encode_wide().collect();
        validate_absolute(&units)?;
        let root = [units[0], b':' as u16, b'\\' as u16, 0];
        if unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } != 3 {
            return Err(blocked("recovery requires a fixed local drive"));
        }
        let raw = unsafe {
            CreateFileW(
                PCWSTR(root.as_ptr()),
                (FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                None,
            )
            .map_err(win_error)?
        };
        let file = File::from(unsafe { own(raw) });
        verify_ntfs(handle(&file))?;
        let meta = metadata(handle(&file))?;
        if !meta.directory {
            return Err(blocked("volume root is not a directory"));
        }
        let volume_path = final_path(handle(&file))?;
        let mut current = Arc::new(Self {
            file,
            identity: meta.identity,
            location: Mutex::new(None),
            volume_path,
            rename_access: false,
        });
        for component in path.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => (),
                Component::Normal(name) => {
                    current = current.open_directory(ComponentName::new(name)?)?;
                }
                _ => return Err(blocked("unsupported absolute recovery path")),
            }
        }
        Ok(current)
    }
    pub(crate) fn open_directory(self: &Arc<Self>, name: ComponentName) -> io::Result<Arc<Self>> {
        let access =
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE;
        let file = self.open_relative(&name, access, FILE_SHARE_READ, FILE_OPEN, true, None)?;
        Self::from_child(self.clone(), name, file, false)
    }
    pub(crate) fn open_for_rename(
        self: &Arc<Self>,
        name: ComponentName,
        expected: &FileIdentity,
    ) -> io::Result<Arc<Self>> {
        let access = FILE_LIST_DIRECTORY
            | FILE_TRAVERSE
            | FILE_READ_ATTRIBUTES
            | READ_CONTROL
            | DELETE
            | SYNCHRONIZE;
        let file = self.open_relative(&name, access, FILE_SHARE_READ, FILE_OPEN, true, None)?;
        let directory = Self::from_child(self.clone(), name, file, true)?;
        if directory.identity() != expected {
            return Err(blocked("rotating root identity changed"));
        }
        Ok(directory)
    }
    /// Enumerates the held directory, including all unknown names. Every child
    /// must still be opened relative to this handle and inspected before use.
    /// This is an inventory observation, not proof that a live tree is immutable.
    pub(crate) fn read_children(&self, maximum: usize) -> io::Result<Vec<ComponentName>> {
        if maximum == 0 || maximum > 100_000 {
            return Err(blocked("unsupported inventory budget"));
        }
        self.recheck()?;
        let mut names = vec![];
        let mut buffer = vec![0usize; 65536 / size_of::<usize>()];
        let mut class = FileIdBothDirectoryRestartInfo;
        loop {
            buffer.fill(0);
            let result = unsafe {
                GetFileInformationByHandleEx(self.raw(), class, buffer.as_mut_ptr().cast(), 65536)
            };
            if let Err(error) = result {
                if error.code() == ERROR_NO_MORE_FILES.to_hresult() {
                    break;
                }
                return Err(win_error(error));
            }
            class = FileIdBothDirectoryInfo;
            let mut offset = 0usize;
            loop {
                let header = offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
                if !offset.is_multiple_of(std::mem::align_of::<FILE_ID_BOTH_DIR_INFO>())
                    || offset + size_of::<FILE_ID_BOTH_DIR_INFO>() > 65536
                {
                    return Err(blocked("malformed NTFS directory entry"));
                }
                let entry = unsafe {
                    &*buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset)
                        .cast::<FILE_ID_BOTH_DIR_INFO>()
                };
                let length = entry.FileNameLength as usize;
                if length == 0
                    || !length.is_multiple_of(2)
                    || length > 510
                    || offset + header + length > 65536
                {
                    return Err(blocked("unsupported NTFS directory name"));
                }
                let units =
                    unsafe { std::slice::from_raw_parts(entry.FileName.as_ptr(), length / 2) };
                if units != [b'.' as u16] && units != [b'.' as u16, b'.' as u16] {
                    names.push(ComponentName::new(&OsString::from_wide(units))?);
                    if names.len() > maximum {
                        return Err(blocked("directory inventory exceeds budget"));
                    }
                }
                if entry.NextEntryOffset == 0 {
                    break;
                }
                let next = entry.NextEntryOffset as usize;
                if next < header + length || next > 65536 - offset {
                    return Err(blocked("malformed NTFS directory chain"));
                }
                offset += next;
            }
        }
        self.recheck()?;
        Ok(names)
    }
    fn from_child(
        parent: Arc<Self>,
        _name: ComponentName,
        file: File,
        rename_access: bool,
    ) -> io::Result<Arc<Self>> {
        let meta = metadata(handle(&file))?;
        if !meta.directory || meta.identity.volume != parent.identity.volume {
            return Err(blocked("directory changed volume or type"));
        }
        let name = canonical_child(parent.raw(), handle(&file))?;
        let result = Arc::new(Self {
            file,
            identity: meta.identity,
            location: Mutex::new(Some(Location { parent, name })),
            volume_path: vec![],
            rename_access,
        });
        result.recheck()?;
        Ok(result)
    }
    pub(crate) fn identity(&self) -> &FileIdentity {
        &self.identity
    }
    pub(super) fn raw(&self) -> HANDLE {
        handle(&self.file)
    }
    pub(crate) fn recheck(&self) -> io::Result<()> {
        if metadata(self.raw())?.identity != self.identity {
            return Err(blocked("directory identity changed"));
        }
        let actual = final_path(self.raw())?;
        let location = self.location.lock().clone();
        let expected = match location {
            Some(location) => {
                location.parent.recheck()?;
                child_path(location.parent.raw(), &location.name)?
            }
            None => self.volume_path.clone(),
        };
        if actual != expected {
            return Err(blocked("directory location changed"));
        }
        Ok(())
    }
    /// Read-only reconciliation after an uncertain metadata effect; no new
    /// authority is minted from the returned path.
    pub(crate) fn observe_location(&self) -> io::Result<(Metadata, OsString)> {
        Ok((
            metadata(self.raw())?,
            OsString::from_wide(&final_path(self.raw())?),
        ))
    }
    pub(crate) fn path(&self) -> io::Result<OsString> {
        self.recheck()?;
        Ok(OsString::from_wide(&final_path(self.raw())?))
    }
    pub(crate) fn contains(&self, other: &Self) -> io::Result<bool> {
        self.recheck()?;
        other.recheck()?;
        let mut path = final_path(self.raw())?;
        if path.last() != Some(&(b'\\' as u16)) {
            path.push(b'\\' as u16);
        }
        Ok(self.identity == other.identity || final_path(other.raw())?.starts_with(&path))
    }
    /// Call with every resolved installation, Desk, UDF, CLI and project root.
    /// This primitive does not certify that discovery supplied the complete set.
    pub(crate) fn require_disjoint(&self, others: &[Arc<Self>]) -> io::Result<()> {
        for other in others {
            if self.contains(other)? || other.contains(self)? {
                return Err(blocked("recovery root overlaps an excluded root"));
            }
        }
        Ok(())
    }
    pub(crate) fn open_file(
        self: &Arc<Self>,
        name: ComponentName,
        access: FileAccess,
    ) -> io::Result<PinnedFile> {
        let (rights, sharing) = match access {
            FileAccess::Read => (
                FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
                FILE_SHARE_READ,
            ),
            FileAccess::ExclusiveRename => (
                FILE_READ_DATA
                    | FILE_WRITE_DATA
                    | FILE_READ_ATTRIBUTES
                    | READ_CONTROL
                    | DELETE
                    | SYNCHRONIZE,
                FILE_SHARE_MODE(0),
            ),
        };
        let file = self.open_relative(&name, rights, sharing, FILE_OPEN, false, None)?;
        PinnedFile::from_file(self.clone(), name, file)
    }
    pub(super) fn open_relative(
        &self,
        name: &ComponentName,
        access: FILE_ACCESS_RIGHTS,
        sharing: FILE_SHARE_MODE,
        disposition: NTCREATEFILE_CREATE_DISPOSITION,
        directory: bool,
        security: Option<&super::security::PrivateDescriptor>,
    ) -> io::Result<File> {
        self.recheck()?;
        let mut units = name.0.clone();
        let string = UNICODE_STRING {
            Length: (units.len() * 2) as u16,
            MaximumLength: (units.len() * 2) as u16,
            Buffer: PWSTR(units.as_mut_ptr()),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: self.raw(),
            ObjectName: &string,
            Attributes: OBJ_DONT_REPARSE,
            SecurityDescriptor: security.map_or(std::ptr::null(), |s| s.pointer()),
            SecurityQualityOfService: std::ptr::null(),
        };
        let mut status = IO_STATUS_BLOCK::default();
        let mut raw = HANDLE::default();
        let options = FILE_SYNCHRONOUS_IO_NONALERT
            | FILE_WRITE_THROUGH
            | if directory {
                FILE_DIRECTORY_FILE
            } else {
                FILE_NON_DIRECTORY_FILE
            };
        let result = unsafe {
            NtCreateFile(
                &mut raw,
                access,
                &attributes,
                &mut status,
                None,
                FILE_ATTRIBUTE_NORMAL,
                sharing,
                disposition,
                options,
                None,
                0,
            )
        };
        // Synchronous handles must never return STATUS_PENDING as a successful open.
        if result.0 as u32 == 0xc0000034 {
            // STATUS_OBJECT_NAME_NOT_FOUND for one relative component in this
            // verified held parent. Other failures never mean authoritative absence.
            return Err(io::Error::from_raw_os_error(2));
        }
        if result.0 != 0 {
            return Err(io::Error::other(format!(
                "relative NTFS open failed: 0x{:08x}",
                result.0 as u32
            )));
        }
        let file = File::from(unsafe { own(raw) });
        if unsafe { status.Anonymous.Status.0 } != 0 {
            return Err(blocked("relative NTFS open was not completed"));
        }
        let meta = metadata(handle(&file))?;
        if meta.directory != directory || meta.identity.volume != self.identity.volume {
            return Err(blocked("relative object changed volume or type"));
        }
        canonical_child(self.raw(), handle(&file))?;
        Ok(file)
    }
    pub(crate) fn rename_to(
        &self,
        parent: Arc<Self>,
        name: ComponentName,
    ) -> io::Result<RenameReceipt> {
        if !self.rename_access || self.contains(&parent)? {
            return Err(blocked("directory rename is not supported by this guard"));
        }
        self.recheck()?;
        let receipt = rename_handle(self.raw(), &self.identity, &parent, &name)?;
        *self.location.lock() = Some(Location { parent, name });
        self.recheck()?;
        Ok(receipt)
    }
}
pub(super) fn validate_absolute(units: &[u16]) -> io::Result<()> {
    if units.len() < 3
        || units.len() >= 32760
        || !(units[0] as u8).is_ascii_alphabetic()
        || units[0] > 127
        || units[1] != b':' as u16
        || units[2] != b'\\' as u16
        || units.contains(&0)
    {
        return Err(blocked("only exact drive-absolute paths are supported"));
    }
    let text = String::from_utf16(units).map_err(|_| blocked("unrepresentable absolute path"))?;
    if text[3..].split('\\').count() > 128
        || text.contains('/')
        || text[3..]
            .split('\\')
            .any(|part| !part.is_empty() && ComponentName::new(OsStr::new(part)).is_err())
        || text[3..].contains("\\\\")
    {
        return Err(blocked("unsupported absolute path"));
    }
    Ok(())
}
fn child_path(parent: HANDLE, name: &ComponentName) -> io::Result<Vec<u16>> {
    let mut path = final_path(parent)?;
    if path.last() != Some(&(b'\\' as u16)) {
        path.push(b'\\' as u16);
    }
    path.extend(&name.0);
    Ok(path)
}

fn canonical_child(parent: HANDLE, child: HANDLE) -> io::Result<ComponentName> {
    let mut prefix = final_path(parent)?;
    if prefix.last() != Some(&(b'\\' as u16)) {
        prefix.push(b'\\' as u16);
    }
    let actual = final_path(child)?;
    let suffix = actual
        .strip_prefix(prefix.as_slice())
        .ok_or_else(|| blocked("relative object is outside its held parent"))?;
    // Preserve the exact canonical spelling returned for the held object. This
    // supports Windows' ordinary case and 8.3 aliases without a path reopen.
    ComponentName::new(&OsString::from_wide(suffix))
}

pub(crate) struct PrivateDirectory {
    directory: Arc<Directory>,
}
impl PrivateDirectory {
    pub(crate) fn create_new(
        parent: Arc<Directory>,
        name: ComponentName,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        let security = user.descriptor(true)?;
        let access =
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE;
        let file = parent.open_relative(
            &name,
            access,
            FILE_SHARE_READ,
            FILE_CREATE,
            true,
            Some(&security),
        )?;
        let directory = Directory::from_child(parent, name, file, false)?;
        let result = Self { directory };
        result.verify(user)?;
        Ok(result)
    }
    pub(crate) fn open_existing(
        parent: Arc<Directory>,
        name: ComponentName,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        let access =
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE;
        let file = parent.open_relative(&name, access, FILE_SHARE_READ, FILE_OPEN, true, None)?;
        let directory = Directory::from_child(parent, name, file, false)?;
        let result = Self { directory };
        result.verify(user)?;
        Ok(result)
    }
    /// Exclusive rotating directory, distinct from a concurrently opened stable
    /// recovery root. The same handle carries DELETE through the later rename.
    pub(crate) fn create_renameable_new(
        parent: Arc<Directory>,
        name: ComponentName,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        let security = user.descriptor(true)?;
        let file = parent.open_relative(
            &name,
            FILE_ALL_ACCESS,
            FILE_SHARE_READ,
            FILE_CREATE,
            true,
            Some(&security),
        )?;
        let directory = Directory::from_child(parent, name, file, true)?;
        let result = Self { directory };
        result.verify(user)?;
        Ok(result)
    }
    pub(crate) fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        self.directory.recheck()?;
        user.verify_private_file(self.directory.raw(), true)
    }
    pub(crate) fn directory(&self) -> &Arc<Directory> {
        &self.directory
    }
}
#[derive(Clone, Copy)]
pub(crate) enum FileAccess {
    Read,
    ExclusiveRename,
}
pub(crate) struct PinnedFile {
    pub(super) file: File,
    pub(super) parent: Arc<Directory>,
    pub(super) name: ComponentName,
    identity: FileIdentity,
}
impl PinnedFile {
    pub(super) fn from_file(
        parent: Arc<Directory>,
        _name: ComponentName,
        file: File,
    ) -> io::Result<Self> {
        let meta = metadata(handle(&file))?;
        if meta.directory {
            return Err(blocked("expected a regular file"));
        }
        let name = canonical_child(parent.raw(), handle(&file))?;
        let result = Self {
            file,
            parent,
            name,
            identity: meta.identity,
        };
        result.verify()?;
        Ok(result)
    }
    pub(crate) fn identity(&self) -> &FileIdentity {
        &self.identity
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.parent.recheck()?;
        if metadata(handle(&self.file))?.identity != self.identity
            || final_path(handle(&self.file))? != child_path(self.parent.raw(), &self.name)?
        {
            return Err(blocked("held file identity or location changed"));
        }
        Ok(())
    }
    pub(crate) fn digest(&self) -> io::Result<String> {
        self.verify()?;
        let size = metadata(handle(&self.file))?.size;
        let mut file = &self.file;
        file.seek(SeekFrom::Start(0))?;
        let mut hash = Sha256::new();
        let mut bytes = [0u8; 65536];
        let mut read = 0u64;
        loop {
            let count = file.read(&mut bytes)?;
            if count == 0 {
                break;
            }
            read += count as u64;
            if read > size {
                return Err(blocked("held file grew"));
            }
            hash.update(&bytes[..count]);
        }
        self.verify()?;
        if read != size || metadata(handle(&self.file))?.size != size {
            return Err(blocked("held file size changed"));
        }
        Ok(format!("{:x}", hash.finalize()))
    }
    pub(crate) fn path(&self) -> io::Result<OsString> {
        self.verify()?;
        Ok(OsString::from_wide(&final_path(handle(&self.file))?))
    }
    pub(super) fn rename_to(
        &mut self,
        parent: Arc<Directory>,
        name: ComponentName,
    ) -> io::Result<RenameReceipt> {
        self.verify()?;
        let receipt = rename_handle(handle(&self.file), &self.identity, &parent, &name)?;
        self.parent = parent;
        self.name = name;
        self.verify()?;
        Ok(receipt)
    }
}
pub(crate) struct RenameReceipt {
    identity: FileIdentity,
    destination: Vec<u16>,
}
impl RenameReceipt {
    pub(crate) fn identity(&self) -> &FileIdentity {
        &self.identity
    }
}
fn rename_handle(
    file: HANDLE,
    identity: &FileIdentity,
    parent: &Directory,
    name: &ComponentName,
) -> io::Result<RenameReceipt> {
    parent.recheck()?;
    if identity.volume != parent.identity.volume || metadata(file)?.identity != *identity {
        return Err(blocked("rename requires the same verified NTFS volume"));
    }
    // The Win32 wrapper interprets names relative to the process working
    // directory. Use the native parent-handle-relative contract directly.
    let length = size_of::<FILE_RENAME_INFORMATION>() + name.0.len() * 2;
    let mut buffer = vec![0usize; length.div_ceil(size_of::<usize>())];
    let mut status = IO_STATUS_BLOCK::default();
    unsafe {
        let information = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        (*information).Anonymous.ReplaceIfExists = false;
        (*information).RootDirectory = parent.raw();
        (*information).FileNameLength = (name.0.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(
            name.0.as_ptr(),
            (*information).FileName.as_mut_ptr(),
            name.0.len(),
        );
        let result = NtSetInformationFile(
            file,
            &mut status,
            information.cast(),
            length as u32,
            FileRenameInformation,
        );
        // Handles are synchronous. Pending/other statuses are uncertain and
        // must never cause a retry, absolute-path fallback, or handle release.
        if result.0 != 0 || status.Anonymous.Status.0 != 0 {
            return Err(io::Error::other(format!(
                "relative NTFS rename failed: 0x{:08x}, completion 0x{:08x}",
                result.0 as u32, status.Anonymous.Status.0 as u32
            )));
        }
    }
    // A failure after the syscall is an uncertain effect. Never rename back or
    // replay here. The caller retains the handle for journal reconciliation.
    let destination = child_path(parent.raw(), name)?;
    if metadata(file)?.identity != *identity || final_path(file)? != destination {
        return Err(blocked("rename postcondition is unknown"));
    }
    Ok(RenameReceipt {
        identity: identity.clone(),
        destination,
    })
}
