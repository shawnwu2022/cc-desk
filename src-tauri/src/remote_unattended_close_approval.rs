//! 新的关闭并删除确认；不复用旧 Forget 的“连接不受影响”语义。
use super::*;
use crate::{
    cli::{document::DocumentBinding, launch_service::NativeRun, snapshot::CallerIdentity},
    remote_native_pairing_store::{Mutation, MutationAuthorization, StoreError, StoreTarget},
    remote_unattended_store::service::controller::{CloseTarget, ForgetRequest},
};
use std::sync::Weak;

struct Description {
    document: Weak<DocumentBinding<NativeRun>>,
    caller: CallerIdentity,
    request: ForgetRequest,
}
impl Description {
    fn check(&self) -> Result<(), StoreError> {
        self.document
            .upgrade()
            .ok_or(StoreError::Unauthorized)?
            .check_admitted(&self.caller)
            .map_err(|_| StoreError::Unauthorized)
    }
}
pub(crate) struct CloseAndForgetApproval {
    machine: Arc<ApprovalMachine<Description>>,
    publication: Mutex<()>,
}
struct Authorization<'a> {
    description: &'a Description,
    lease: Arc<ApprovalLease>,
    end: Instant,
}
impl MutationAuthorization for Authorization<'_> {
    fn check(&mut self, target: &StoreTarget, mutation: &Mutation) -> Result<(), StoreError> {
        if self.lease.is_revoked()
            || Instant::now() >= self.end
            || !self.description.request.matches_store(target)
            || self.description.request.mutation() != mutation
        {
            return Err(StoreError::Unauthorized);
        }
        self.description.check()?;
        if self.lease.is_revoked() || Instant::now() >= self.end {
            return Err(StoreError::Unauthorized);
        }
        Ok(())
    }
}
impl CloseAndForgetApproval {
    pub(crate) fn show<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
        document: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        request: ForgetRequest,
    ) -> Result<Self, StoreError> {
        let message = request.render();
        let description = Description {
            document: Arc::downgrade(document),
            caller: caller.clone(),
            request,
        };
        description.check()?;
        if DIALOG_ACTIVE
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_err()
        {
            return Err(StoreError::Busy);
        }
        Ok(Self {
            machine: ApprovalMachine::show(
                app,
                window,
                description,
                message,
                "关闭并删除已配对设备",
                Instant::now() + Duration::from_secs(300),
            ),
            publication: Mutex::new(()),
        })
    }
    pub(crate) fn cancel(&self) {
        self.machine.lease.revoke_only();
        let _gate = self.publication.lock();
        self.machine.cancel();
    }
    // 关门回调仅操作不可复位位；任何 service/store 锁都在其后。
    pub(crate) fn commit<T, E: From<StoreError>>(
        &self,
        target: &CloseTarget,
        store: &StoreTarget,
        write: impl FnOnce(&Mutation, &mut dyn MutationAuthorization) -> Result<T, E>,
    ) -> Result<Option<T>, E> {
        let Some(permit) = self.machine.take() else {
            return Ok(None);
        };
        permit
            .consume(|description, end| {
                description.check().map_err(E::from)?;
                if !description.request.matches_target(target)
                    || !description.request.matches_store(store)
                {
                    return Err(E::from(StoreError::Unauthorized));
                }
                {
                    let _gate = self.publication.lock();
                    if self.machine.lease.is_revoked() || Instant::now() >= end {
                        return Err(E::from(StoreError::Unauthorized));
                    }
                    target.request_close();
                }
                let mut auth = Authorization {
                    description: &description,
                    lease: self.machine.lease.clone(),
                    end,
                };
                write(description.request.mutation(), &mut auth)
            })
            .ok_or_else(|| E::from(StoreError::Unauthorized))?
            .map(Some)
    }
}
#[cfg(test)]
impl CloseAndForgetApproval {
    pub(crate) fn test_pending(
        document: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        request: ForgetRequest,
    ) -> Self {
        let now = Instant::now();
        Self {
            machine: ApprovalMachine::pending(
                Description {
                    document: Arc::downgrade(document),
                    caller: caller.clone(),
                    request,
                },
                now,
                now + Duration::from_secs(300),
            ),
            publication: Mutex::new(()),
        }
    }
    pub(crate) fn test_answer(&self, yes: bool) {
        self.machine.complete(yes, Instant::now());
    }
}
