//! Bounded native transport completion; not endpoint authentication.
use super::*;
use cc_desk_remote_transport::records::{Kind, RecordWriter};
struct SendingGuard {
    record: Arc<IssuedView>,
    completed: bool,
}
impl Drop for SendingGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.record.phase.store(REVOKED, Ordering::Release);
            self.record.scope.close();
        }
    }
}
impl IssuedView {
    fn emission_deadline(&self, end: tokio::time::Instant) -> Result<tokio::time::Instant, Reject> {
        match &self.read_grant {
            None => Ok(end),
            Some(grant) => {
                let remaining = grant.remaining(&self.scope)?;
                let grant_end = tokio::time::Instant::now()
                    .checked_add(remaining)
                    .ok_or(Reject::Bounds)?;
                Ok(end.min(grant_end))
            }
        }
    }
}
impl projection::CapturedProjection {
    pub(in crate::local_writer) fn issued_stamp(&self) -> Result<&Stamp, Reject> {
        self.issued
            .as_ref()
            .map(|r| &r.stamp)
            .ok_or(Reject::Unsupported)
    }
    /// Consumes the native frame+writer; no manual delivered setter exists.
    /// Once sending starts, partial/unknown IO or cancellation drops the supplied
    /// adapter and closes this peer scope. Never-polled/pre-IO refusals cannot
    /// confirm delivery. The authenticated channel owner must cancel its sibling.
    pub(in crate::local_writer) async fn deliver<
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    >(
        self,
        mut writer: RecordWriter<W>,
        budget: Duration,
    ) -> Result<RecordWriter<W>, Reject> {
        if budget.is_zero() || budget > cc_desk_remote_transport::MAX_IO_BUDGET {
            return Err(Reject::Bounds);
        }
        let end = tokio::time::Instant::now()
            .checked_add(budget)
            .ok_or(Reject::Bounds)?;
        let record = self.issued.as_ref().ok_or(Reject::Unsupported)?.clone();
        if record
            .read_grant
            .as_ref()
            .is_some_and(|g| g.authority.is_some())
        {
            return Err(Reject::Policy);
        }
        if writer.selected_minor() != record.scope.minor || !record.scope.is_active() {
            return Err(Reject::Sync);
        }
        let mut end = record.emission_deadline(end)?;
        record
            .phase
            .compare_exchange(ISSUED, SENDING, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Reject::Sync)?;
        let mut guard = SendingGuard {
            record: record.clone(),
            completed: false,
        };
        let mut closed = record.scope.closed.subscribe();
        let mut writer_closed = record.scope.writer_closed.subscribe();
        let encoder = cc_desk_remote_terminal::SnapshotEncoder::new(
            self.view(),
            record.stamp.clone(),
            record.scope.minor,
        )
        .map_err(|_| Reject::Bounds)?;
        for part in encoder {
            if !record.scope.is_active() || record.phase.load(Ordering::Acquire) != SENDING {
                return Err(Reject::Sync);
            }
            let payload = part.map_err(|_| Reject::Bounds)?;
            end = record.emission_deadline(end)?;
            let part_end = end;
            let remaining = part_end
                .checked_duration_since(tokio::time::Instant::now())
                .filter(|b| !b.is_zero())
                .ok_or(Reject::Continuity)?;
            tokio::select! {
                _=closed.changed()=>return Err(Reject::Sync),
                _=writer_closed.changed()=>return Err(Reject::Sync),
                result=tokio::time::timeout_at(part_end,writer.write(Kind::Projection,&payload,remaining))=>{
                    result.map_err(|_|Reject::Continuity)?.map_err(|_|Reject::Continuity)?;
                }
            }
            if tokio::time::Instant::now() >= part_end {
                return Err(Reject::Continuity);
            }
        }
        if !record.scope.is_active() {
            return Err(Reject::Sync);
        }
        record.emission_deadline(end)?;
        record
            .phase
            .compare_exchange(SENDING, DELIVERED, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Reject::Sync)?;
        guard.completed = true;
        Ok(writer)
    }
    /// Completion is the actual bounded TLS write by the original native owner.
    pub(in crate::local_writer) async fn deliver_bound(
        self,
        channel: &cc_desk_remote_security::RunningChannel,
        fence: u64,
        budget: Duration,
    ) -> Result<(), Reject> {
        if budget.is_zero() || budget > cc_desk_remote_transport::MAX_IO_BUDGET {
            return Err(Reject::Bounds);
        }
        let record = self.issued.as_ref().ok_or(Reject::Unsupported)?.clone();
        let authority = record
            .read_grant
            .as_ref()
            .and_then(|g| g.authority.as_ref())
            .ok_or(Reject::Policy)?;
        if !authority.same_channel(&channel.authority()) || !record.scope.is_active() {
            return Err(Reject::Sync);
        }
        if record.read_grant.as_ref().is_none_or(|g| g.fence != fence) {
            return Err(Reject::Epoch);
        }
        let mut end = record.emission_deadline(
            tokio::time::Instant::now()
                .checked_add(budget)
                .ok_or(Reject::Bounds)?,
        )?;
        record
            .phase
            .compare_exchange(ISSUED, SENDING, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Reject::Sync)?;
        let mut guard = SendingGuard {
            record: record.clone(),
            completed: false,
        };
        let mut closed = record.scope.closed.subscribe();
        let mut writer_closed = record.scope.writer_closed.subscribe();
        let encoder = cc_desk_remote_terminal::SnapshotEncoder::new(
            self.view(),
            record.stamp.clone(),
            record.scope.minor,
        )
        .map_err(|_| Reject::Bounds)?;
        for part in encoder {
            if !record.scope.is_active() || record.phase.load(Ordering::Acquire) != SENDING {
                return Err(Reject::Sync);
            }
            let payload = part.map_err(|_| Reject::Bounds)?;
            end = record.emission_deadline(end)?;
            let remaining = end
                .checked_duration_since(tokio::time::Instant::now())
                .filter(|b| !b.is_zero())
                .ok_or(Reject::Continuity)?;
            tokio::select! {
                _=closed.changed()=>return Err(Reject::Sync),
                _=writer_closed.changed()=>return Err(Reject::Sync),
                result=tokio::time::timeout_at(end,channel.send_projection(authority.scope(),fence,&payload,remaining))=>{result.map_err(|_|Reject::Continuity)?.map_err(|_|Reject::Continuity)?;}
            }
            if tokio::time::Instant::now() >= end {
                return Err(Reject::Continuity);
            }
        }
        record.emission_deadline(end)?;
        if !record.scope.is_active() {
            return Err(Reject::Sync);
        }
        record
            .phase
            .compare_exchange(SENDING, DELIVERED, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Reject::Sync)?;
        guard.completed = true;
        Ok(())
    }
}
