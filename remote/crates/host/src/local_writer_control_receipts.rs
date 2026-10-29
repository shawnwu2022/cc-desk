use super::*;
#[derive(Debug, PartialEq, Eq)]
pub(in crate::local_writer) enum ControlError {
    Rejected(Reject),
    Journal(JournalError),
}
pub(in crate::local_writer) struct PendingValue<T> {
    reply: mpsc::Receiver<Result<T, ControlError>>,
    _credit: Arc<Credit>,
}
impl<T> PendingValue<T> {
    pub(in crate::local_writer) fn wait(
        &self,
        budget: Duration,
    ) -> Result<Result<T, ControlError>, WaitError> {
        self.reply.recv_timeout(budget).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => WaitError::Uncertain,
            mpsc::RecvTimeoutError::Disconnected => WaitError::Closed,
        })
    }
}
impl PeerViewHandle {
    pub(in crate::local_writer) fn pending_receipt(
        &self,
        mut key: OperationKey,
    ) -> Result<PendingValue<ReceiptLookup>, Reject> {
        compact_key(&mut key)?;
        let credit = self.input.credits.reserve()?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.send_control(ControlRequest {
            scope: self.control.scope.clone(),
            action: ControlAction::Query { key, reply: tx },
            _credit: credit.clone(),
        })?;
        Ok(PendingValue {
            reply: rx,
            _credit: credit,
        })
    }
    pub(in crate::local_writer) fn settle_receipt(
        &self,
        mut proof: ReceiptProof,
    ) -> Result<PendingValue<AckDecision>, Reject> {
        compact_key(&mut proof.key)?;
        let credit = self.input.credits.reserve()?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.send_control(ControlRequest {
            scope: self.control.scope.clone(),
            action: ControlAction::Settle { proof, reply: tx },
            _credit: credit.clone(),
        })?;
        Ok(PendingValue {
            reply: rx,
            _credit: credit,
        })
    }
}
fn compact_key(key: &mut OperationKey) -> Result<(), Reject> {
    if key.run.is_empty() || key.run.len() > 128 {
        return Err(Reject::Bounds);
    }
    key.run = key.run.as_str().to_owned();
    Ok(())
}
impl ControlState {
    fn query_authority(&self, scope: &PeerScope, key: &OperationKey) -> Result<(), ControlError> {
        let now = self.sample(scope).map_err(ControlError::Rejected)?;
        if !scope.is_active()
            || now >= self.grant_deadline
            || key.target() != scope.target
            || key.device != scope.device
        {
            return Err(ControlError::Rejected(Reject::Policy));
        }
        Ok(())
    }
    pub(super) fn query_receipt(
        &self,
        scope: &PeerScope,
        key: &OperationKey,
        journal: &Journal,
    ) -> Result<ReceiptLookup, ControlError> {
        self.query_authority(scope, key)?;
        journal.query(key).map_err(ControlError::Journal)
    }
    pub(super) fn settle(
        &mut self,
        scope: &PeerScope,
        proof: ReceiptProof,
        journal: &mut Journal,
    ) -> Result<AckDecision, ControlError> {
        self.query_authority(scope, &proof.key)?;
        if self
            .pending
            .as_ref()
            .is_some_and(|(key, _)| key != &proof.key)
        {
            return Err(ControlError::Rejected(Reject::Pending));
        }
        let ack = SettlementAck {
            target: scope.target.clone(),
            device: scope.device,
            through: proof.key.sequence,
            proofs: vec![proof],
        };
        let result = journal
            .acknowledge(&scope.device, &ack)
            .map_err(ControlError::Journal)?;
        if matches!(
            result,
            AckDecision::Advanced(_) | AckDecision::AlreadySettled(_)
        ) {
            self.pending = None;
        }
        Ok(result)
    }
}
