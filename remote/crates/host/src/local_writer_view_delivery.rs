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
        if writer.selected_minor() != record.scope.minor || !record.scope.is_active() {
            return Err(Reject::Sync);
        }
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
            let remaining = end
                .checked_duration_since(tokio::time::Instant::now())
                .filter(|b| !b.is_zero())
                .ok_or(Reject::Continuity)?;
            tokio::select! {
                _=closed.changed()=>return Err(Reject::Sync),
                _=writer_closed.changed()=>return Err(Reject::Sync),
                result=tokio::time::timeout_at(end,writer.write(Kind::Projection,&payload,remaining))=>{
                    result.map_err(|_|Reject::Continuity)?.map_err(|_|Reject::Continuity)?;
                }
            }
            if tokio::time::Instant::now() >= end {
                return Err(Reject::Continuity);
            }
        }
        if !record.scope.is_active() {
            return Err(Reject::Sync);
        }
        record
            .phase
            .compare_exchange(SENDING, DELIVERED, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Reject::Sync)?;
        guard.completed = true;
        Ok(writer)
    }
}
