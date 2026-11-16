//! Forget 只写公开 tombstone，不触碰独立的连接 owner。
use super::*;
use crate::{
    cli::{document::DocumentBinding, launch_service::NativeRun, snapshot::CallerIdentity},
    remote_native_pairing_store::{
        LockedSnapshot, Mutation, MutationAuthorization, StoreError, StoreTarget,
    },
};
use std::sync::Weak;
struct ForgetDescription {
    document: Weak<DocumentBinding<NativeRun>>,
    caller: CallerIdentity,
    origin: NativeRunNetworkOrigin,
    target: StoreTarget,
    mutation: Mutation,
}
impl ForgetDescription {
    fn check(&self, admission: &NativeRunAdmission) -> Result<(), StoreError> {
        self.document
            .upgrade()
            .ok_or(StoreError::Unauthorized)?
            .check_admitted(&self.caller)
            .map_err(|_| StoreError::Unauthorized)?;
        if !self.origin.matches(admission) || admission.network_origin().is_err() {
            return Err(StoreError::Unauthorized);
        }
        Ok(())
    }
}
pub(crate) struct NativeForgetApproval {
    machine: Arc<ApprovalMachine<ForgetDescription>>,
}
struct ForgetAuthorization<'a> {
    description: &'a ForgetDescription,
    admission: &'a NativeRunAdmission,
    lease: Arc<ApprovalLease>,
    end: Instant,
}
impl MutationAuthorization for ForgetAuthorization<'_> {
    fn check(&mut self, target: &StoreTarget, mutation: &Mutation) -> Result<(), StoreError> {
        if self.lease.is_revoked()
            || Instant::now() >= self.end
            || !self.description.target.same_target(target)
            || &self.description.mutation != mutation
        {
            return Err(StoreError::Unauthorized);
        }
        self.description.check(self.admission)?;
        if self.lease.is_revoked() || Instant::now() >= self.end {
            return Err(StoreError::Unauthorized);
        }
        Ok(())
    }
}
impl NativeForgetApproval {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn show<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
        document: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        target: StoreTarget,
        mutation: Mutation,
        record: &crate::remote_native_pairing_store::PublicDeviceRecord,
    ) -> Result<Self, StoreError> {
        let Mutation::Forget { device_id, .. } = &mutation else {
            return Err(StoreError::Unauthorized);
        };
        if device_id != &record.device_id {
            return Err(StoreError::Unauthorized);
        }
        let text = format!("Forget this public device suggestion?\nThis writes an identity-only tombstone. It does not revoke any independently approved running connection or delete browser storage.\nTarget: {}\nMutation: {}\nExact public record: {}", public_text(target.label()), public_text(&serde_json::to_string(&mutation).map_err(|_| StoreError::Invalid)?), public_text(&serde_json::to_string(record).map_err(|_| StoreError::Invalid)?));
        let description = ForgetDescription {
            document: Arc::downgrade(document),
            caller: caller.clone(),
            origin: admission
                .network_origin()
                .map_err(|_| StoreError::Unauthorized)?,
            target,
            mutation,
        };
        description.check(admission)?;
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
                text,
                "Forget public device",
                Instant::now() + Duration::from_secs(30),
            ),
        })
    }
    pub(crate) fn cancel(&self) {
        self.machine.cancel();
    }
    pub(crate) fn pending(&self) -> bool {
        self.machine.waiting()
    }
    pub(crate) fn commit(
        &self,
        admission: &NativeRunAdmission,
        write: impl FnOnce(
            &Mutation,
            &mut dyn MutationAuthorization,
        ) -> Result<LockedSnapshot, StoreError>,
    ) -> Result<Option<LockedSnapshot>, StoreError> {
        let Some(permit) = self.machine.take() else {
            return Ok(None);
        };
        permit
            .consume(|description, end| {
                description.check(admission)?;
                let mut authorization = ForgetAuthorization {
                    description: &description,
                    admission,
                    lease: self.machine.lease.clone(),
                    end,
                };
                write(&description.mutation, &mut authorization)
            })
            .ok_or(StoreError::Unauthorized)?
            .map(Some)
    }
}

#[cfg(test)]
impl NativeForgetApproval {
    pub(crate) fn test_pending(
        document: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        target: StoreTarget,
        mutation: Mutation,
    ) -> Self {
        assert!(matches!(mutation, Mutation::Forget { .. }));
        let description = ForgetDescription {
            document: Arc::downgrade(document),
            caller: caller.clone(),
            origin: admission.network_origin().unwrap(),
            target,
            mutation,
        };
        description.check(admission).unwrap();
        Self {
            machine: ApprovalMachine::pending(
                description,
                Instant::now(),
                Instant::now() + Duration::from_secs(30),
            ),
        }
    }
    pub(crate) fn test_complete(&self, yes: bool) {
        self.machine.complete(yes, Instant::now());
    }
}
