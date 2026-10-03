//! Opaque coordinator evidence shared by actual return executors. Source-only
//! quiescence cannot be passed as return admission. Constructors belong to the
//! complete live coordinator, never IPC/deserialization or digest parameters.
use super::{
    blocked,
    fence::ImageFence,
    files::{ComponentName, Directory, FileAccess},
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{journal::JournalBinding, maintenance::SnapshotBoundary},
};
use parking_lot::Mutex;
use std::{io, sync::Arc};

#[derive(Clone)]
pub(crate) struct VerifiedImageAbsence {
    directory: Arc<Directory>,
    name: ComponentName,
}
impl VerifiedImageAbsence {
    pub(super) fn capture(directory: Arc<Directory>, name: ComponentName) -> io::Result<Self> {
        let value = Self { directory, name };
        value.verify()?;
        Ok(value)
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.directory.recheck()?;
        match self
            .directory
            .open_file(self.name.clone(), FileAccess::Read)
        {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Ok(_) => return Err(blocked("expected current image absence changed")),
            Err(error) => return Err(error),
        }
        self.directory.recheck()
    }
}
#[derive(Clone)]
pub(crate) enum CurrentImageEvidence {
    Fenced(Arc<Mutex<ImageFence>>),
    Absent(VerifiedImageAbsence),
}
/// A separately observed return barrier retains historical/installer/job and
/// browser terminal evidence, fresh exclusive lease, and actual later roots.
/// Its fields are private: a source SnapshotBoundary cannot mint this object.
pub(crate) struct ReturnBoundary {
    snapshot: SnapshotBoundary,
    installation: Arc<Directory>,
    image_name: ComponentName,
    current_image: CurrentImageEvidence,
}
impl ReturnBoundary {
    pub(super) fn from_native(
        guards: super::return_boundary::ReturnSnapshotGuards,
        current_image: CurrentImageEvidence,
    ) -> Result<Self, SafeError> {
        guards.verify_live()?;
        let installation = guards.installation().clone();
        let image_name = guards.image_name().clone();
        let value = Self {
            snapshot: SnapshotBoundary::from_return(guards)?,
            installation,
            image_name,
            current_image,
        };
        value.verify_current_image()?;
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn fixture(
        snapshot: SnapshotBoundary,
        installation: Arc<Directory>,
        image_name: ComponentName,
        fence: Option<Arc<Mutex<ImageFence>>>,
    ) -> io::Result<Self> {
        let current_image = match fence {
            Some(fence) => CurrentImageEvidence::Fenced(fence),
            None => {
                let absence = VerifiedImageAbsence {
                    directory: installation.clone(),
                    name: image_name.clone(),
                };
                absence.verify()?;
                CurrentImageEvidence::Absent(absence)
            }
        };
        let value = Self {
            snapshot,
            installation,
            image_name,
            current_image,
        };
        value
            .verify_current_image()
            .map_err(|_| blocked("return fixture observation differs"))?;
        Ok(value)
    }
    pub(crate) fn verify_live(&self) -> Result<(), SafeError> {
        self.snapshot.verify_live()?;
        self.installation
            .recheck()
            .map_err(|_| error("HISTORY_INSTALLATION_CHANGED"))?;
        match &self.current_image {
            CurrentImageEvidence::Fenced(fence) => {
                let fence = fence.lock();
                fence
                    .verify()
                    .map_err(|_| error("HISTORY_INSTALLATION_CHANGED"))?;
                if !fence
                    .context_original_child(&self.installation, &self.image_name)
                    .map_err(|_| error("HISTORY_INSTALLATION_CHANGED"))?
                {
                    return Err(error("HISTORY_INSTALLATION_CHANGED"));
                }
            }
            CurrentImageEvidence::Absent(absence) => {
                if absence.directory.identity() != self.installation.identity()
                    || absence.name != self.image_name
                {
                    return Err(error("HISTORY_INSTALLATION_CHANGED"));
                }
                absence
                    .directory
                    .recheck()
                    .map_err(|_| error("HISTORY_INSTALLATION_CHANGED"))?;
            }
        }
        Ok(())
    }
    /// Exact initial slot admission. After a journaled restore creates the
    /// original image, its retained effect guard replaces this historical
    /// absence observation; verify_live continues to check quiescence/leases.
    pub(crate) fn verify_current_image(&self) -> Result<(), SafeError> {
        self.verify_live()?;
        if let CurrentImageEvidence::Absent(absence) = &self.current_image {
            absence
                .verify()
                .map_err(|_| error("HISTORY_INSTALLATION_CHANGED"))?;
        }
        Ok(())
    }
    pub(crate) fn binding(&self) -> &JournalBinding {
        self.snapshot.binding()
    }
    pub(crate) fn snapshot(&self) -> &SnapshotBoundary {
        &self.snapshot
    }
    pub(crate) fn installation(&self) -> &Arc<Directory> {
        &self.installation
    }
    pub(crate) fn image_name(&self) -> &ComponentName {
        &self.image_name
    }
    pub(crate) fn current_image(&self) -> &CurrentImageEvidence {
        &self.current_image
    }
}
