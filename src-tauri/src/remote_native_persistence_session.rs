//! 原登记证据在独立本机记忆批准和浏览器安装批准间移动，不能复制为授权。
use super::*;
use crate::{
    remote_host_approval::{NativePersistenceApproval, NativePersistenceTarget},
    remote_native_pairing_store::{LockedSnapshot, Mutation, MutationAuthorization, StoreError},
};

impl NativeRegistrationOwner {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn request_native_persistence<R: tauri::Runtime>(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        host: FrozenHostApproval,
        target: NativePersistenceTarget,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<(), SafeError> {
        self.check(binding, caller, admission)?;
        let Stage::Evidence(evidence) = std::mem::replace(&mut self.stage, Stage::Closed) else {
            self.cancel();
            return Err(error("REMOTE_HOST_LOCKED"));
        };
        let approval = NativePersistenceApproval::show(
            app,
            window,
            host,
            evidence,
            self.context_binding,
            binding,
            caller,
            admission,
            target,
            self.end,
        )
        .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        self.stage = Stage::AwaitingNativePersistence(approval);
        Ok(())
    }

    pub(crate) fn persist_native(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        write: impl FnOnce(
            &Mutation,
            &mut dyn MutationAuthorization,
        ) -> Result<LockedSnapshot, StoreError>,
    ) -> Result<Option<(FrozenHostApproval, LockedSnapshot)>, SafeError> {
        self.check(binding, caller, admission)?;
        let Stage::AwaitingNativePersistence(approval) = &self.stage else {
            return Err(error("REMOTE_HOST_LOCKED"));
        };
        match approval.commit(binding, caller, admission, write) {
            Ok(Some(persisted)) => {
                self.stage = Stage::Evidence(persisted.evidence);
                Ok(Some((persisted.host, persisted.snapshot)))
            }
            Ok(None) => Ok(None),
            Err(StoreError::Unknown | StoreError::Poisoned) => {
                self.cancel();
                Err(error("REMOTE_NATIVE_STORE_COMMIT_UNKNOWN"))
            }
            Err(_) => {
                self.cancel();
                Err(error("REMOTE_NATIVE_STORE_COMMIT_FAILED"))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn test_request_native_persistence(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        host: FrozenHostApproval,
        target: NativePersistenceTarget,
    ) -> Result<Arc<NativePersistenceApproval>, SafeError> {
        self.check(binding, caller, admission)?;
        let Stage::Evidence(evidence) = std::mem::replace(&mut self.stage, Stage::Closed) else {
            self.cancel();
            return Err(error("REMOTE_HOST_LOCKED"));
        };
        let approval = NativePersistenceApproval::test_pending(
            host,
            evidence,
            self.context_binding,
            binding,
            caller,
            admission,
            target,
            self.end,
        )
        .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        self.stage = Stage::AwaitingNativePersistence(approval.clone());
        Ok(approval)
    }
}

impl<S> NativeRegistrationSession<S> {
    pub(crate) fn native_persistence_pending(&self) -> bool {
        self.owner.as_ref().is_some_and(|o| match &o.stage {
            Stage::AwaitingNativePersistence(a) => a.pending(),
            _ => false,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn request_native_persistence<R: tauri::Runtime>(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        host: FrozenHostApproval,
        target: NativePersistenceTarget,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<(), SafeError> {
        self.check_host(&host)?;
        self.owner
            .as_mut()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?
            .request_native_persistence(binding, caller, admission, host, target, app, window)
    }

    pub(crate) fn persist_native(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        write: impl FnOnce(
            &Mutation,
            &mut dyn MutationAuthorization,
        ) -> Result<LockedSnapshot, StoreError>,
    ) -> Result<Option<(FrozenHostApproval, LockedSnapshot)>, SafeError> {
        self.check_live_decision()?;
        self.owner
            .as_mut()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?
            .persist_native(binding, caller, admission, write)
    }

    #[cfg(test)]
    pub(crate) fn test_request_native_persistence(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        host: FrozenHostApproval,
        target: NativePersistenceTarget,
    ) -> Result<Arc<NativePersistenceApproval>, SafeError> {
        self.check_host(&host)?;
        self.owner
            .as_mut()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?
            .test_request_native_persistence(binding, caller, admission, host, target)
    }
}
