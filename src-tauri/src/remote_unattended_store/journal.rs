//! 固定长度、固定上限的事务证据链，不包含授权句柄。
use super::{Result, StoreError};
use sha2::{Digest, Sha256};
pub(crate) const RECORD_BYTES: usize = 224;
pub(crate) const MAX_RECORDS: usize = 8192;
pub(crate) const MAX_JOURNAL: usize = RECORD_BYTES * MAX_RECORDS;
const MAGIC: &[u8; 8] = b"CCDUJ001";
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommitStamp {
    pub(crate) store_id: [u8; 16],
    pub(crate) revision: u64,
    pub(crate) authority_revision: u64,
    pub(crate) target: [u8; 32],
    pub(crate) blob: [u8; 32],
}
#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub(crate) clean: bool,
    pub(crate) sequence: u64,
    pub(crate) transaction: [u8; 16],
    pub(crate) stamp: CommitStamp,
    pub(crate) previous: [u8; 32],
}
impl Record {
    pub(crate) fn bytes(&self) -> [u8; RECORD_BYTES] {
        let mut b = [0; RECORD_BYTES];
        b[..8].copy_from_slice(MAGIC);
        b[8] = if self.clean { 2 } else { 1 };
        b[16..24].copy_from_slice(&self.sequence.to_le_bytes());
        b[24..40].copy_from_slice(&self.transaction);
        b[40..56].copy_from_slice(&self.stamp.store_id);
        b[56..64].copy_from_slice(&self.stamp.revision.to_le_bytes());
        b[64..72].copy_from_slice(&self.stamp.authority_revision.to_le_bytes());
        b[72..104].copy_from_slice(&self.stamp.target);
        b[104..136].copy_from_slice(&self.stamp.blob);
        b[136..168].copy_from_slice(&self.previous);
        let hash: [u8; 32] = Sha256::digest(&b[..192]).into();
        b[192..].copy_from_slice(&hash);
        b
    }
    fn decode(b: &[u8]) -> Result<Self> {
        if b.len() != RECORD_BYTES
            || &b[..8] != MAGIC
            || ![1, 2].contains(&b[8])
            || b[9..16].iter().chain(b[168..192].iter()).any(|v| *v != 0)
        {
            return Err(StoreError::Unknown);
        }
        let hash: [u8; 32] = Sha256::digest(&b[..192]).into();
        if b[192..] != hash {
            return Err(StoreError::Unknown);
        }
        Ok(Self {
            clean: b[8] == 2,
            sequence: u64::from_le_bytes(b[16..24].try_into().unwrap()),
            transaction: b[24..40].try_into().unwrap(),
            stamp: CommitStamp {
                store_id: b[40..56].try_into().unwrap(),
                revision: u64::from_le_bytes(b[56..64].try_into().unwrap()),
                authority_revision: u64::from_le_bytes(b[64..72].try_into().unwrap()),
                target: b[72..104].try_into().unwrap(),
                blob: b[104..136].try_into().unwrap(),
            },
            previous: b[136..168].try_into().unwrap(),
        })
    }
}
#[derive(Default)]
pub(crate) struct Chain {
    pub(crate) records: usize,
    pub(crate) last: Option<Record>,
    pub(crate) hash: [u8; 32],
    transactions: std::collections::BTreeSet<[u8; 16]>,
    anchor: Option<VerifiedCheckpoint>,
}
impl Chain {
    pub(crate) fn parse(bytes: &[u8], allow_empty: bool) -> Result<Self> {
        if bytes.len() > MAX_JOURNAL
            || !bytes.len().is_multiple_of(RECORD_BYTES)
            || (bytes.is_empty() && !allow_empty)
        {
            return Err(StoreError::Unknown);
        }
        Self::parse_suffix(bytes, Self::default())
    }
    fn parse_suffix(bytes: &[u8], mut state: Self) -> Result<Self> {
        if !bytes.len().is_multiple_of(RECORD_BYTES) {
            return Err(StoreError::Unknown);
        }
        for bytes in bytes.chunks_exact(RECORD_BYTES) {
            let r = Record::decode(bytes)?;
            if Some(r.sequence)
                != state
                    .last
                    .as_ref()
                    .map_or(Some(1), |r| r.sequence.checked_add(1))
                || r.previous != state.hash
                || r.transaction == [0; 16]
                || r.stamp.store_id == [0; 16]
                || r.stamp.target == [0; 32]
                || r.stamp.revision == 0
                || r.stamp.authority_revision == 0
                || r.stamp.authority_revision > r.stamp.revision
            {
                return Err(StoreError::Unknown);
            }
            if !r.clean && !state.transactions.insert(r.transaction) {
                return Err(StoreError::Unknown);
            }
            let invalid_transition = match state.last.as_ref() {
                None => r.clean || r.stamp.revision != 1 || r.stamp.authority_revision != 1,
                Some(old) if !old.clean => {
                    !r.clean || r.transaction != old.transaction || r.stamp != old.stamp
                }
                Some(old) => {
                    r.clean
                        || r.stamp.store_id != old.stamp.store_id
                        || r.stamp.target != old.stamp.target
                        || old.stamp.revision.checked_add(1) != Some(r.stamp.revision)
                        || r.stamp.authority_revision < old.stamp.authority_revision
                        || r.stamp.authority_revision
                            > old.stamp.authority_revision.saturating_add(1)
                }
            };
            if invalid_transition {
                return Err(StoreError::Unknown);
            }

            state.hash = Sha256::digest(bytes).into();
            state.records += 1;
            state.last = Some(r);
        }
        if state.last.as_ref().is_some_and(|r| !r.clean) {
            return Err(StoreError::Unknown);
        }
        Ok(state)
    }
    pub(crate) fn pending(&self, stamp: &CommitStamp) -> Result<Record> {
        if self.remaining_commits()? < 1 {
            return Err(StoreError::Capacity);
        }
        let sequence = self
            .last
            .as_ref()
            .map_or(Some(1), |r| r.sequence.checked_add(1))
            .ok_or(StoreError::Invalid)?;
        sequence.checked_add(1).ok_or(StoreError::Invalid)?;
        let transaction = *uuid::Uuid::new_v4().as_bytes();
        if transaction == [0; 16] || self.transactions.contains(&transaction) {
            return Err(StoreError::Conflict);
        }
        let r = Record {
            clean: false,
            sequence,
            transaction,
            stamp: stamp.clone(),
            previous: self.hash,
        };
        // 写入前核对下一版本及事务唯一性，不改变真实日志。
        if let Some(last) = &self.last {
            if stamp.store_id != last.stamp.store_id
                || stamp.target != last.stamp.target
                || last.stamp.revision.checked_add(1) != Some(stamp.revision)
                || stamp.authority_revision < last.stamp.authority_revision
                || stamp.authority_revision > last.stamp.authority_revision.saturating_add(1)
            {
                return Err(StoreError::Conflict);
            }
        } else if stamp.revision != 1 || stamp.authority_revision != 1 {
            return Err(StoreError::Conflict);
        }
        Ok(r)
    }
}

#[path = "checkpoint_codec.rs"]
mod checkpoint;
pub(crate) use checkpoint::{CheckpointBasis, VerifiedCheckpoint, HEADER, PROCESSING_MAGIC};
pub(super) fn seal_checkpoint(
    doc: &super::Document,
    blob: &[u8],
    basis: CheckpointBasis,
    sealer: &dyn super::Sealer,
) -> Result<VerifiedCheckpoint> {
    checkpoint::seal(doc, blob, basis, sealer)
}
