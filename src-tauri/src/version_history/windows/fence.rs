//! Retains the very file opened exclusively through its no-replacement rename.
//! The real CreateProcess race probe, complete scope/host-exit and transaction
//! intent remain prerequisites for coordinator admission, not claims of this API.
use super::{
    blocked,
    files::{ComponentName, Directory, FileAccess, FileIdentity, PinnedFile, RenameReceipt},
    handle, win_error,
};
use std::{io, sync::Arc};

pub(crate) struct ImageFence {
    image: PinnedFile,
    digest: String,
}
impl ImageFence {
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
