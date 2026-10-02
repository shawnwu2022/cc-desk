//! Retains the very file opened exclusively through its no-replacement rename.
//! The real CreateProcess race probe, complete scope/host-exit and transaction
//! intent remain prerequisites for coordinator admission, not claims of this API.
use super::{
    blocked,
    files::{ComponentName, Directory, FileAccess, FileIdentity, PinnedFile, RenameReceipt},
    handle, win_error,
};
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
};

pub(crate) struct ImageFence {
    image: PinnedFile,
    digest: String,
    original_parent: Arc<Directory>,
    original_name: ComponentName,
}
impl ImageFence {
    /// Only the initial exclusive open reported a sharing conflict. No fence
    /// was acquired and no mutation occurred. This is not evidence that the
    /// unknown holder will exit, or permission to retry any later failure.
    pub(crate) fn is_acquisition_busy(error: &io::Error) -> bool {
        super::files::is_exclusive_open_busy(error)
    }
    pub(crate) fn acquire(
        parent: Arc<Directory>,
        name: ComponentName,
        identity: &FileIdentity,
        digest: &str,
    ) -> io::Result<Self> {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return Err(blocked("invalid expected image digest"));
        }
        let image = parent.open_file(name, FileAccess::ExclusiveRename)?;
        if image.identity() != identity || image.digest()? != digest {
            return Err(blocked("registered image identity changed"));
        }
        Ok(Self {
            original_parent: image.parent.clone(),
            original_name: image.name.clone(),
            image,
            digest: digest.to_owned(),
        })
    }
    /// Read-only same-handle inspection also works after an uncertain rename;
    /// this never treats the old pathname as authoritative or repeats an effect.
    pub(crate) fn observe_location(&self) -> io::Result<(FileIdentity, std::ffi::OsString)> {
        let metadata = super::files::metadata(handle(&self.image.file))?;
        let path = super::files::final_path(handle(&self.image.file))?;
        use std::os::windows::ffi::OsStringExt;
        Ok((metadata.identity, std::ffi::OsString::from_wide(&path)))
    }
    pub(crate) fn identity(&self) -> &FileIdentity {
        self.image.identity()
    }
    /// Bundle capture reads the SAME exclusive image guard; it never reopens
    /// the protected name or weakens sharing to copy the executable.
    pub(super) fn context_metadata(&self) -> io::Result<super::files::Metadata> {
        self.image.verify()?;
        super::files::metadata(handle(&self.image.file))
    }
    pub(super) fn context_descriptor(&self) -> io::Result<Vec<u8>> {
        self.image.verify()?;
        super::security::capture_file_descriptor(handle(&self.image.file))
    }
    pub(super) fn context_read(&self, offset: u64, bytes: &mut [u8]) -> io::Result<usize> {
        self.image.verify()?;
        let mut file = &self.image.file;
        file.seek(SeekFrom::Start(offset))?;
        let count = file.read(bytes)?;
        self.image.verify()?;
        Ok(count)
    }
    pub(super) fn context_named_child(
        &self,
        parent: &Directory,
        name: &ComponentName,
    ) -> io::Result<bool> {
        self.image.verify()?;
        parent.recheck()?;
        Ok(self.image.parent.identity() == parent.identity() && self.image.name == *name)
    }
    pub(super) fn context_original_child(
        &self,
        parent: &Directory,
        name: &ComponentName,
    ) -> io::Result<bool> {
        self.image.verify()?;
        self.original_parent.recheck()?;
        parent.recheck()?;
        Ok(self.original_parent.identity() == parent.identity() && self.original_name == *name)
    }
    pub(super) fn context_verify_streams(&self) -> io::Result<()> {
        self.image.verify()?;
        super::context::verify_streams(handle(&self.image.file), &self.context_metadata()?)
    }
    #[cfg(test)]
    pub(crate) fn probe_file(&self) -> &std::fs::File {
        &self.image.file
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        if self.image.digest()? != self.digest {
            return Err(blocked("fenced image changed"));
        }
        Ok(())
    }
    /// A private quarantine parent cannot substitute for this object ACL
    /// check: Windows traversal privileges can bypass a parent's DACL.
    pub(super) fn verify_confidential(
        &self,
        user: &super::security::CurrentUser,
    ) -> io::Result<()> {
        self.verify()?;
        user.verify_confidential_source(super::handle(&self.image.file), false)
    }
    /// Caller must commit its exact rename intent first. Failure after rename
    /// can be uncertain; this object retains the original handle either way.
    pub(crate) fn rename_to(
        &mut self,
        parent: Arc<Directory>,
        name: ComponentName,
    ) -> io::Result<RenameReceipt> {
        self.verify()?;
        let receipt = self.image.rename_to(parent, name)?;
        unsafe {
            windows::Win32::Storage::FileSystem::FlushFileBuffers(handle(&self.image.file))
                .map_err(win_error)?;
        }
        self.verify()?;
        Ok(receipt)
    }
}
