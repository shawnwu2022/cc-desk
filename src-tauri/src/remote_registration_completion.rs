//! 安装完成是历史事实，不是无人值守许可；构造入口仅属于原登记 owner。
use super::{DeliveryComplete, SafeError};
use crate::remote_native_pairing_store::{
    record_digest, ExpectedStore, LockedSnapshot, Mutation, StoreError, StoreTarget,
};

pub(super) struct PendingInstallation {
    target: StoreTarget,
    expected: ExpectedStore,
    device: String,
    digest: [u8; 32],
}
impl PendingInstallation {
    pub(super) fn persisted(
        target: StoreTarget,
        mutation: &Mutation,
        snapshot: &LockedSnapshot,
    ) -> Result<Self, StoreError> {
        let record = match mutation {
            Mutation::Initialize { record, .. } | Mutation::Remember { record, .. } => record,
            Mutation::Forget { .. } => return Err(StoreError::Unauthorized),
        };
        let entry = snapshot
            .devices
            .iter()
            .find(|e| e.device_id == record.device_id)
            .ok_or(StoreError::Conflict)?;
        if entry.record.as_ref() != Some(record) {
            return Err(StoreError::Conflict);
        }
        Ok(Self {
            target,
            expected: snapshot.expected.clone(),
            device: record.device_id.clone(),
            digest: record_digest(record)?,
        })
    }
    pub(super) fn complete(
        self,
        _delivered: DeliveryComplete,
        recheck: &dyn Fn() -> Result<(), SafeError>,
    ) -> Result<CompletedInstallation<'_>, SafeError> {
        recheck()?;
        Ok(CompletedInstallation {
            pending: self,
            recheck,
        })
    }
}

// 不实现 Clone/Deserialize；存储入口必须消费唯一的原交付结果。
pub(crate) struct CompletedInstallation<'a> {
    pending: PendingInstallation,
    recheck: &'a dyn Fn() -> Result<(), SafeError>,
}
impl CompletedInstallation<'_> {
    pub(crate) fn check(&self, target: &StoreTarget) -> Result<(), StoreError> {
        if !self.pending.target.same_target(target) {
            return Err(StoreError::Unauthorized);
        }
        (self.recheck)().map_err(|_| StoreError::Unauthorized)
    }
    pub(crate) fn expected(&self) -> &ExpectedStore {
        &self.pending.expected
    }
    pub(crate) fn device(&self) -> &str {
        &self.pending.device
    }
    pub(crate) fn digest(&self) -> [u8; 32] {
        self.pending.digest
    }
}

#[cfg(test)]
#[path = "tests/native_registration_completion.rs"]
mod tests;
