use super::*;
use crate::{journal_digest::DigestKey, lifecycle_completion::Applied};
fn seal(
    key: &DigestKey,
    scope: [u8; 32],
    op: [u8; 16],
    seq: u64,
    digest: [u8; 32],
    backend: [u8; 32],
    payload: &[u8],
) -> Result<[u8; 32], Error> {
    let mut b = scope.to_vec();
    b.extend(op);
    b.extend(seq.to_be_bytes());
    b.extend(digest);
    b.extend(backend);
    b.extend(payload);
    key.lifecycle_result(&b).map_err(|_| Error::Bounds)
}
type StoredRow = (i64, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);
impl Store {
    pub(crate) fn validate_results(
        &self,
        scope: [u8; 32],
        backend: Option<[u8; 32]>,
        key: &DigestKey,
    ) -> Result<(), Error> {
        self.check()?;
        if self.legacy {
            return Ok(());
        }
        let rows = {
            let mut stmt = self
                .db
                .prepare("SELECT op,seq,digest FROM ops WHERE scope=?1 AND state=3")
                .map_err(storage)?;
            stmt.query_map(params![scope], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?
        };
        for (op, seq, digest) in rows {
            let op: [u8; 16] = op.try_into().map_err(|_| Error::Schema)?;
            let seq = u64::from_be_bytes(seq.try_into().map_err(|_| Error::Schema)?);
            let digest = digest.try_into().map_err(|_| Error::Schema)?;
            let event = self
                .restore_attach(scope, op, seq, digest, backend.ok_or(Error::Conflict)?, key)?
                .ok_or(Error::Schema)?;
            if event.request.operation != op || event.request.sequence != seq {
                return Err(Error::Conflict);
            }
        }
        Ok(())
    }
    pub(crate) fn record_attach(
        &mut self,
        t: &Ticket,
        backend: [u8; 32],
        event: &Applied,
        key: &DigestKey,
    ) -> Result<(), Error> {
        self.check()?;
        if self.legacy
            || backend == [0; 32]
            || !Weak::ptr_eq(&t.owner, &Arc::downgrade(&self.owner))
        {
            return Err(Error::Conflict);
        }
        let payload = event.encode_stored().map_err(|_| Error::Bounds)?;
        if event.request.operation != t.op || event.request.sequence != t.seq {
            return Err(Error::Conflict);
        }
        let tag = seal(key, t.scope, t.op, t.seq, t.digest, backend, &payload)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let (state, seq, digest): (i64, Vec<u8>, Vec<u8>) = tx
            .query_row(
                "SELECT state,seq,digest FROM ops WHERE scope=?1 AND op=?2",
                params![t.scope, t.op],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(storage)?;
        if seq != t.seq.to_be_bytes() || digest != t.digest || ![2, 3].contains(&state) {
            return Err(Error::Conflict);
        }
        if state == 3 {
            let row: (Vec<u8>, Vec<u8>, Vec<u8>) = tx
                .query_row(
                    "SELECT backend,payload,seal FROM results WHERE scope=?1 AND op=?2",
                    params![t.scope, t.op],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(storage)?;
            return if row == (backend.to_vec(), payload, tag.to_vec()) {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        tx.execute(
            "INSERT INTO results VALUES(?1,?2,?3,?4,?5)",
            params![t.scope, t.op, backend, payload, tag],
        )
        .map_err(storage)?;
        tx.execute(
            "UPDATE ops SET state=3,code=NULL WHERE scope=?1 AND op=?2 AND state=2",
            params![t.scope, t.op],
        )
        .map_err(storage)?;
        let witness:StoredRow=tx.query_row("SELECT o.state,o.seq,o.digest,r.backend,r.payload,r.seal FROM ops o JOIN results r ON o.scope=r.scope AND o.op=r.op WHERE o.scope=?1 AND o.op=?2",params![t.scope,t.op],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).map_err(storage)?;
        if witness
            != (
                3,
                t.seq.to_be_bytes().to_vec(),
                t.digest.to_vec(),
                backend.to_vec(),
                payload,
                tag.to_vec(),
            )
        {
            self.poisoned = true;
            return Err(Error::Storage);
        }
        #[cfg(test)]
        if self.fault == Some(Cut::BeforeTransition) {
            self.poisoned = true;
            return Err(Error::Storage);
        }
        if tx.commit().is_err() {
            self.poisoned = true;
            return Err(Error::Storage);
        }
        #[cfg(test)]
        if self.fault == Some(Cut::AfterTransition) {
            self.poisoned = true;
            return Err(Error::Storage);
        }
        Ok(())
    }
    pub(crate) fn restore_attach(
        &self,
        scope: [u8; 32],
        op: [u8; 16],
        seq: u64,
        digest: [u8; 32],
        backend: [u8; 32],
        key: &DigestKey,
    ) -> Result<Option<Applied>, Error> {
        self.check()?;
        if self.legacy {
            return Ok(None);
        }
        let row:Option<StoredRow>=self.db.query_row("SELECT o.state,o.seq,o.digest,r.backend,r.payload,r.seal FROM ops o JOIN results r ON o.scope=r.scope AND o.op=r.op WHERE o.scope=?1 AND o.op=?2",params![scope,op],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(storage)?;
        let Some((state, s, d, b, p, tag)) = row else {
            return Ok(None);
        };
        if state != 3 || s != seq.to_be_bytes() || d != digest || b != backend {
            return Err(Error::Conflict);
        }
        use subtle::ConstantTimeEq;
        if !bool::from(tag.ct_eq(&seal(key, scope, op, seq, digest, backend, &p)?)) {
            return Err(Error::Conflict);
        }
        Ok(Some(Applied::decode_stored(&p).map_err(|_| Error::Schema)?))
    }
}
