//! 同一原 service 的逐条认证业务准入；不在 GrantClock 或 actor 锁内读盘。
use super::*;
use crate::remote_host::delegated::DelegatedAttemptIdentity;
use crate::remote_host_approval::unattended::final_approval::FinalControl;
use cc_desk_remote_host::native_host::{NativeChannel, NativeConnection};
use cc_desk_remote_security::DataRecord;

pub(crate) struct ServiceRequestGate {
    control: Arc<FinalControl>,
    attempt: DelegatedAttemptIdentity,
    check: Box<dyn Fn() -> Result<()> + Send + Sync>,
}
pub(crate) struct AuthorizedRecord {
    record: DataRecord,
    control: Arc<FinalControl>,
    channel: Arc<NativeChannel>,
}
impl ServiceRequestGate {
    pub(crate) fn matches_attempt(&self, attempt: &DelegatedAttemptIdentity) -> bool {
        self.attempt.matches(attempt)
    }
    pub(super) fn new<S, B, P>(
        service: &Arc<parking_lot::Mutex<SingleRunService<S>>>,
        pairing: Arc<PairingStore<B, P>>,
        control: Arc<FinalControl>,
        attempt: DelegatedAttemptIdentity,
    ) -> Self
    where
        S: Sealer + Send + 'static,
        B: PairingBackend + Send + Sync + 'static,
        P: PairingSealer + Send + Sync + 'static,
    {
        let weak = Arc::downgrade(service);
        let closed = control.clone();
        Self {
            control,
            attempt,
            check: Box::new(move || {
                if !closed.active() {
                    return Err(StoreError::Unauthorized);
                }
                let service = weak.upgrade().ok_or(StoreError::Unauthorized)?;
                let mut service = service.try_lock().ok_or(StoreError::Busy)?;
                service.refresh(&pairing)?;
                if !closed.active() {
                    return Err(StoreError::Unauthorized);
                }
                Ok(())
            }),
        }
    }
    pub(crate) fn authorize(
        &self,
        record: DataRecord,
        channel: &Arc<NativeChannel>,
        attempt: &DelegatedAttemptIdentity,
    ) -> std::result::Result<AuthorizedRecord, ()> {
        if !self.control.active()
            || !self.attempt.matches(attempt)
            || !record.source().same_channel(&channel.authority())
            || (self.check)().is_err()
            || !self.control.active()
        {
            // 不能回调并阻塞 service：它可能正在持锁等待本 pump 的原 join。
            self.control.close();
            channel.authority().request_close();
            return Err(());
        }
        Ok(AuthorizedRecord {
            record,
            control: self.control.clone(),
            channel: channel.clone(),
        })
    }
}
impl AuthorizedRecord {
    pub(crate) async fn dispatch(
        self,
        connection: &mut NativeConnection,
    ) -> std::result::Result<(), ()> {
        if !self.control.active() || self.channel.authority().check().is_err() {
            return Err(());
        }
        connection
            .dispatch_received(&self.record, std::time::Duration::from_secs(1))
            .await
            .map_err(|_| ())
    }
}
