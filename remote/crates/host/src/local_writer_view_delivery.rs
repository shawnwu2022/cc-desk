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
        self.deliver_bound_inner(channel, fence, budget, None).await
    }
    pub(in crate::local_writer) async fn deliver_pull_bound(
        self,
        channel: &cc_desk_remote_security::RunningChannel,
        fence: u64,
        budget: Duration,
        request: u64,
    ) -> Result<(), Reject> {
        self.deliver_bound_inner(channel, fence, budget, Some(request))
            .await
    }
    pub(in crate::local_writer) async fn deliver_bound_inner(
        self,
        channel: &cc_desk_remote_security::RunningChannel,
        fence: u64,
        budget: Duration,
        pull_request: Option<u64>,
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
        let mut encoder = cc_desk_remote_terminal::SnapshotEncoder::new(
            self.view(),
            record.stamp.clone(),
            record.scope.minor,
        )
        .map_err(|_| Reject::Bounds)?;
        if pull_request.is_some() {
            if !authority.snapshot_pull_negotiated() {
                return Err(Reject::Policy);
            }
            encoder = cc_desk_remote_terminal::SnapshotEncoder::with_payload_limit(
                self.view(),
                record.stamp.clone(),
                record.scope.minor,
                cc_desk_remote_record::snapshot_pull::MAX_PART,
            )
            .map_err(|_| Reject::Bounds)?;
        }
        let transfer = if pull_request.is_some() {
            let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
            digest.update(b"cc-desk/snapshot-pull/host/v1");
            digest.update(&authority.binding_id());
            digest.update(&pull_request.unwrap_or(0).to_be_bytes());
            digest.update(
                &cc_desk_remote_record::projection::encode_ack(
                    &cc_desk_remote_record::projection::Ack {
                        profile: 1,
                        stamp: record.stamp.clone(),
                    },
                    record.scope.minor,
                )
                .map_err(|_| Reject::Bounds)?,
            );
            let value: [u8; 32] = digest
                .finish()
                .as_ref()
                .try_into()
                .map_err(|_| Reject::Bounds)?;

            value
        } else {
            [0; 32]
        };
        let mut batch_index = 0u32;
        for part in encoder {
            if !record.scope.is_active() || record.phase.load(Ordering::Acquire) != SENDING {
                return Err(Reject::Sync);
            }
            let payload = part.map_err(|_| Reject::Bounds)?;
            let mut through = 0;
            let mut last = true;
            let payload = if let Some(request) = pull_request {
                let message = cc_desk_remote_record::projection::decode(&payload, 1)
                    .map_err(|_| Reject::Bounds)?;
                let cc_desk_remote_record::projection::Content::SnapshotPart(p) = message.content
                else {
                    return Err(Reject::Bounds);
                };
                through = u32::from(p.start)
                    .checked_add(p.cells.len() as u32)
                    .ok_or(Reject::Bounds)?;
                last = through == u32::from(p.state.rows) * u32::from(p.state.columns);
                cc_desk_remote_record::snapshot_pull::encode_batch(
                    &cc_desk_remote_record::snapshot_pull::Batch {
                        request,
                        transfer,
                        batch: batch_index,
                        through,
                        last,
                        part: &payload,
                    },
                )
                .map_err(|_| Reject::Bounds)?
            } else {
                payload
            };
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
            if let Some(request) = pull_request.filter(|_| !last) {
                batch_index = batch_index.checked_add(1).ok_or(Reject::Bounds)?;
                end = record.emission_deadline(end)?;
                let left = end
                    .checked_duration_since(tokio::time::Instant::now())
                    .filter(|b| !b.is_zero())
                    .ok_or(Reject::Continuity)?;
                let next = tokio::select! {
                    _=closed.changed()=>return Err(Reject::Sync),
                    _=writer_closed.changed()=>return Err(Reject::Sync),
                    r=tokio::time::timeout_at(end,channel.receive(authority.scope(),fence,left)) => r.map_err(|_|Reject::Continuity)?.map_err(|_|Reject::Continuity)?,
                };
                if !next.source().same_channel(authority)
                    || next.scope() != authority.scope()
                    || next.fence() != fence
                {
                    return Err(Reject::Target);
                }
                let cc_desk_remote_record::snapshot_pull::Message::SnapshotNext {
                    request: n,
                    transfer: t,
                    batch,
                    through: o,
                    ..
                } = cc_desk_remote_record::snapshot_pull::decode(next.payload())
                    .map_err(|_| Reject::Bounds)?
                else {
                    return Err(Reject::Bounds);
                };
                let expected: String = transfer.iter().map(|b| format!("{b:02x}")).collect();
                if n != request.to_string()
                    || t != expected
                    || batch != batch_index.to_string()
                    || o != through.to_string()
                {
                    return Err(Reject::Epoch);
                }
                authority.check().map_err(|_| Reject::Policy)?;
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
