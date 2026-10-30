//! Exact native data-owner replies. No listener, caller identity or enrollment
//! is minted here; the only caller is an already installed authorized peer.
use super::*;
use cc_desk_remote_record::{command, control_io as wire};

pub(super) fn decode_hex<const N: usize>(s: &str) -> Result<[u8; N], Reject> {
    if s.len() != 2 * N {
        return Err(Reject::Bounds);
    }
    let mut out = [0; N];
    for (i, pair) in s.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let text = std::str::from_utf8(pair).map_err(|_| Reject::Bounds)?;
        out[i] = u8::from_str_radix(text, 16).map_err(|_| Reject::Bounds)?;
    }
    Ok(out)
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|n| format!("{n:02x}")).collect()
}
fn native_outcome(outcome: wire::Outcome) -> Outcome {
    match outcome {
        wire::Outcome::HostWritten => Outcome::HostWritten,
        wire::Outcome::Rejected => Outcome::Rejected,
        wire::Outcome::Partial => Outcome::Partial,
        wire::Outcome::Unknown => Outcome::Unknown,
    }
}
fn receipt(envelope: &ReceiptEnvelope) -> Result<(wire::Outcome, Option<String>), Reject> {
    let outcome = match envelope.outcome {
        Outcome::HostWritten => wire::Outcome::HostWritten,
        Outcome::Rejected => wire::Outcome::Rejected,
        Outcome::Partial => wire::Outcome::Partial,
        Outcome::Unknown => wire::Outcome::Unknown,
    };
    let proof = envelope.wire_token().map(|t| hex(&t));
    if matches!(
        outcome,
        wire::Outcome::HostWritten | wire::Outcome::Rejected
    ) && proof.is_none()
    {
        return Err(Reject::Continuity);
    }
    Ok((outcome, proof))
}
struct CloseOnDrop(Option<BoundDataAuthority>);
impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        if let Some(proof) = &self.0 {
            proof.request_close();
        }
    }
}
impl Connection<'_> {
    fn operation_key(&self, operation: [u8; 16], sequence: u64) -> Result<OperationKey, Reject> {
        let c = self.proof.context();
        let scope = self.proof.scope();
        Ok(OperationKey {
            host: decode_hex(&c.host_endpoint_id)?,
            session: decode_hex(&scope.session)?,
            run: scope.run.clone(),
            generation: scope.generation.parse().map_err(|_| Reject::Target)?,
            incarnation: c.incarnation.parse().map_err(|_| Reject::Target)?,
            device: decode_hex(&c.client_endpoint_id)?,
            operation,
            sequence,
        })
    }
    /// Cancellation/error invalidates only this original native data owner.
    /// An uncertain write is reconciled from durable facts, never re-executed.
    pub(in crate::local_writer::owner) async fn dispatch_wire(
        &mut self,
        r: &DataRecord,
        host: &RunningChannel,
        b: Duration,
    ) -> Result<(), Reject> {
        let deadline = end(b)?;
        let mut close = CloseOnDrop(Some(self.proof.clone()));
        self.context(r)?;
        if !host.authority().same_channel(&self.proof) {
            return Err(Reject::Target);
        }
        let left = || deadline.saturating_duration_since(tokio::time::Instant::now());
        let response = match command::decode(r.payload()) {
            Ok(command::Command::Snapshot(_)) => {
                self.snapshot(r, host, left()).await?;
                None
            }
            Ok(command::Command::Ack(ack)) => {
                self.acknowledge(r, left()).await?;
                Some(wire::Message::ProjectionAcknowledged {
                    profile: 1,
                    stamp: ack.stamp,
                })
            }
            Ok(command::Command::Window(request)) => {
                let (token, not_after) = self.window(r, left()).await?;
                Some(wire::Message::WindowGranted {
                    profile: 1,
                    request: request.to_string(),
                    window: hex(&token),
                    not_after_host_ms: not_after.to_string(),
                })
            }
            Ok(command::Command::Input(input)) => {
                let key = self.operation_key(input.operation, input.sequence)?;
                let (outcome, proof) = match self.input(r, left()).await? {
                    WriteResult::Receipt(envelope) => {
                        if envelope.key != key {
                            return Err(Reject::Target);
                        }
                        receipt(&envelope)?
                    }
                    WriteResult::Rejected(_) => {
                        let response = wire::Message::InputNotAccepted {
                            profile: 1,
                            operation: hex(&input.operation),
                            sequence: input.sequence.to_string(),
                        };
                        self.send_wire(host, &response, deadline).await?;
                        close.0 = None;
                        return Ok(());
                    }
                    WriteResult::Partial => (wire::Outcome::Partial, None),
                    WriteResult::Unknown | WriteResult::Journal(_) => {
                        (wire::Outcome::Unknown, None)
                    }
                };
                Some(wire::Message::InputReceipt {
                    profile: 1,
                    operation: hex(&input.operation),
                    sequence: input.sequence.to_string(),
                    outcome,
                    proof,
                })
            }
            Ok(command::Command::Disconnect(request)) => {
                self.counter(request)?;
                self.proof.request_close();
                close.0 = None;
                return Ok(());
            }
            Err(_) => match wire::decode(r.payload()).map_err(|_| Reject::Bounds)? {
                wire::Message::ReceiptQuery {
                    request,
                    operation,
                    sequence,
                    ..
                } => {
                    self.counter(request.parse().map_err(|_| Reject::Bounds)?)?;
                    let pending = self.peer.pending_receipt(
                        decode_hex(&operation)?,
                        sequence.parse().map_err(|_| Reject::Bounds)?,
                    )?;
                    let found = wait(deadline, move |b| {
                        pending.wait(b).map(|r| r.map_err(|_| Reject::Continuity))
                    })
                    .await?;
                    let (status, outcome, proof) = match found {
                        ReceiptLookup::Known(envelope) => {
                            if envelope.key
                                != self.operation_key(
                                    decode_hex(&operation)?,
                                    sequence.parse().map_err(|_| Reject::Bounds)?,
                                )?
                            {
                                return Err(Reject::Target);
                            }
                            let (outcome, proof) = receipt(&envelope)?;
                            (wire::Status::Known, Some(outcome), proof)
                        }
                        ReceiptLookup::Missing => (wire::Status::Missing, None, None),
                        ReceiptLookup::TooOld => (wire::Status::TooOld, None, None),
                    };
                    Some(wire::Message::ReceiptStatus {
                        profile: 1,
                        request,
                        operation,
                        sequence,
                        status,
                        outcome,
                        proof,
                    })
                }
                wire::Message::ReceiptAck {
                    request,
                    operation,
                    sequence,
                    outcome,
                    proof,
                    ..
                } => {
                    self.counter(request.parse().map_err(|_| Reject::Bounds)?)?;
                    let op = decode_hex(&operation)?;
                    let seq = sequence.parse().map_err(|_| Reject::Bounds)?;
                    let candidate = ReceiptProof::wire_candidate(
                        self.operation_key(op, seq)?,
                        native_outcome(outcome),
                        decode_hex(&proof)?,
                    )
                    .ok_or(Reject::Bounds)?;
                    let pending = self.peer.settle_receipt(candidate)?;
                    let decision = wait(deadline, move |b| {
                        pending.wait(b).map(|r| r.map_err(|_| Reject::Continuity))
                    })
                    .await?;
                    let disposition = match decision {
                        AckDecision::Advanced(n) if n == seq => wire::Disposition::Advanced,
                        AckDecision::AlreadySettled(n) if n == seq => {
                            wire::Disposition::AlreadySettled
                        }
                        AckDecision::Rejected(_) => wire::Disposition::Rejected,
                        _ => return Err(Reject::Continuity),
                    };
                    self.proof.check().map_err(|_| Reject::Policy)?;
                    if disposition != wire::Disposition::Rejected
                        && self.pending_input == Some((op, seq))
                    {
                        self.pending_input = None;
                        self.busy = false;
                    }
                    Some(wire::Message::ReceiptSettled {
                        profile: 1,
                        request,
                        operation,
                        sequence,
                        outcome,
                        disposition,
                    })
                }
                _ => return Err(Reject::Bounds),
            },
        };
        if let Some(response) = response {
            self.send_wire(host, &response, deadline).await?;
        }
        self.proof.check().map_err(|_| Reject::Policy)?;
        if tokio::time::Instant::now() >= deadline {
            return Err(Reject::Continuity);
        }
        close.0 = None;
        Ok(())
    }
    async fn send_wire(
        &self,
        host: &RunningChannel,
        message: &wire::Message,
        deadline: tokio::time::Instant,
    ) -> Result<(), Reject> {
        self.proof.check().map_err(|_| Reject::Policy)?;
        if !host.authority().same_channel(&self.proof) {
            return Err(Reject::Target);
        }
        let bytes = wire::encode(message).map_err(|_| Reject::Bounds)?;
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return Err(Reject::Continuity);
        }
        host.send_projection(self.proof.scope(), self.fence, &bytes, left)
            .await
            .map_err(|_| Reject::Continuity)?;
        self.proof.check().map_err(|_| Reject::Policy)
    }
}
