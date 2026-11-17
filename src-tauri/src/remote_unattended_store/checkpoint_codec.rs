//! 已验证 checkpoint 基点；文件头只负责界定长度，权限来自原密封文档。
use super::super::{Document, Sealer, MAX_PLAINTEXT, MAX_SEALED};
use super::*;
use serde::{Deserialize, Serialize};
pub(crate) const HEADER: usize = 64;
pub(crate) const CHECKPOINT_MAGIC: &[u8; 8] = b"CCDUJ002";
pub(crate) const PROCESSING_MAGIC: &[u8; 8] = b"!PENDING";
const DOMAIN: &str = "cc-desk.unattended.checkpoint.v1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Capsule {
    domain: String,
    schema: u32,
    document: Document,
    document_digest: [u8; 32],
    blob: [u8; 32],
    prior_chain: [u8; 32],
    sequence: u64,
    segment: u64,
    transaction: [u8; 16],
}
pub(crate) struct CheckpointBasis {
    last: Record,
    hash: [u8; 32],
    segment: u64,
}
pub(crate) struct VerifiedCheckpoint {
    capsule: Capsule,
    bytes: Vec<u8>,
}
impl Chain {
    pub(crate) fn checkpoint_basis(&self) -> Result<CheckpointBasis> {
        let last = self.last.as_ref().ok_or(StoreError::Invalid)?;
        if !last.clean || !self.records.is_multiple_of(2) {
            return Err(StoreError::Unknown);
        }
        Ok(CheckpointBasis {
            last: last.clone(),
            hash: self.hash,
            segment: self.anchor.as_ref().map_or(0, |a| a.capsule.segment),
        })
    }
    pub(crate) fn parse_authenticated(
        bytes: &[u8],
        fresh: bool,
        sealer: &dyn Sealer,
    ) -> Result<Self> {
        if !bytes.starts_with(CHECKPOINT_MAGIC) {
            return Self::parse(bytes, fresh);
        }
        if bytes.len() > MAX_JOURNAL {
            return Err(StoreError::Unknown);
        }
        let anchor = VerifiedCheckpoint::open(bytes, sealer)?;
        let prefix = anchor.bytes.len();
        let last = Record {
            clean: true,
            sequence: anchor.capsule.sequence,
            transaction: anchor.capsule.transaction,
            stamp: anchor.stamp()?,
            previous: anchor.capsule.prior_chain,
        };
        let state = Self {
            records: 0,
            last: Some(last),
            hash: Sha256::digest(&anchor.bytes).into(),
            transactions: std::collections::BTreeSet::from([anchor.capsule.transaction]),
            anchor: Some(anchor),
        };
        Self::parse_suffix(&bytes[prefix..], state)
    }
    pub(crate) fn verify_checkpoint_blob(&self, blob: &[u8], sealer: &dyn Sealer) -> Result<()> {
        let Some(anchor) = &self.anchor else {
            return Ok(());
        };
        let current = super::super::decode_document(sealer, blob)?;
        let old = &anchor.capsule.document;
        if current.schema != old.schema
            || current.mode != old.mode
            || current.store_id != old.store_id
            || current.target != old.target
            || current.revision < old.revision
            || current.authority_revision < old.authority_revision
            || current.observed_wall_ms < old.observed_wall_ms
            || (current.revision == old.revision
                && (current != *old || Sha256::digest(blob)[..] != anchor.capsule.blob))
            || (current.authority_revision == old.authority_revision
                && (current.status != old.status
                    || current.issued_ms != old.issued_ms
                    || current.cutoff_ms != old.cutoff_ms))
        {
            return Err(StoreError::Unknown);
        }
        Ok(())
    }
    pub(crate) fn remaining_commits(&self) -> Result<usize> {
        let prefix = self.anchor.as_ref().map_or(0, |a| a.bytes.len());
        let used = self
            .records
            .checked_mul(RECORD_BYTES)
            .and_then(|n| n.checked_add(prefix))
            .ok_or(StoreError::Unknown)?;
        Ok(MAX_JOURNAL.checked_sub(used).ok_or(StoreError::Unknown)? / (2 * RECORD_BYTES))
    }
}
impl VerifiedCheckpoint {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn matches(&self, chain: &Chain) -> Result<()> {
        let basis = chain.checkpoint_basis()?;
        if self.capsule.sequence != basis.last.sequence
            || self.capsule.transaction != basis.last.transaction
            || self.capsule.prior_chain != basis.hash
            || Some(self.capsule.segment) != basis.segment.checked_add(1)
            || self.stamp()? != basis.last.stamp
        {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }
    fn stamp(&self) -> Result<CommitStamp> {
        let d = &self.capsule.document;
        Ok(CommitStamp {
            store_id: d.store_id,
            revision: d.revision,
            authority_revision: d.authority_revision,
            target: Sha256::digest(serde_json::to_vec(&d.target).map_err(|_| StoreError::Invalid)?)
                .into(),
            blob: self.capsule.blob,
        })
    }
    fn open(bytes: &[u8], sealer: &dyn Sealer) -> Result<Self> {
        if bytes.len() < HEADER
            || &bytes[..8] != CHECKPOINT_MAGIC
            || bytes[48..HEADER].iter().any(|b| *b != 0)
        {
            return Err(StoreError::Unknown);
        }
        let size: usize = u64::from_le_bytes(bytes[8..16].try_into().unwrap())
            .try_into()
            .map_err(|_| StoreError::Unknown)?;
        let end = HEADER.checked_add(size).ok_or(StoreError::Unknown)?;
        if size == 0
            || size > MAX_SEALED
            || end > bytes.len()
            || end > MAX_JOURNAL - 2 * RECORD_BYTES
            || Sha256::digest(&bytes[HEADER..end])[..] != bytes[16..48]
        {
            return Err(StoreError::Unknown);
        }
        let raw = sealer
            .unseal(&bytes[HEADER..end])
            .map_err(|_| StoreError::Unknown)?;
        if raw.len() > MAX_PLAINTEXT {
            return Err(StoreError::Unknown);
        }
        let capsule: Capsule = serde_json::from_slice(&raw).map_err(|_| StoreError::Unknown)?;
        super::super::validate(&capsule.document).map_err(|_| StoreError::Unknown)?;
        if capsule.domain != DOMAIN
            || capsule.schema != 1
            || capsule.segment == 0
            || capsule.sequence == 0
            || !capsule.sequence.is_multiple_of(2)
            || capsule.transaction == [0; 16]
            || capsule.prior_chain == [0; 32]
            || capsule.blob == [0; 32]
            || Sha256::digest(
                serde_json::to_vec(&capsule.document).map_err(|_| StoreError::Unknown)?,
            )[..]
                != capsule.document_digest
        {
            return Err(StoreError::Unknown);
        }
        Ok(Self {
            capsule,
            bytes: bytes[..end].to_vec(),
        })
    }
}
pub(super) fn seal(
    doc: &Document,
    blob: &[u8],
    basis: CheckpointBasis,
    sealer: &dyn Sealer,
) -> Result<VerifiedCheckpoint> {
    super::super::validate(doc)?;
    let capsule = Capsule {
        domain: DOMAIN.into(),
        schema: 1,
        document: doc.clone(),
        document_digest: Sha256::digest(serde_json::to_vec(doc).map_err(|_| StoreError::Invalid)?)
            .into(),
        blob: Sha256::digest(blob).into(),
        prior_chain: basis.hash,
        sequence: basis.last.sequence,
        segment: basis.segment.checked_add(1).ok_or(StoreError::Invalid)?,
        transaction: basis.last.transaction,
    };
    let raw = serde_json::to_vec(&capsule).map_err(|_| StoreError::Invalid)?;
    if raw.len() > MAX_PLAINTEXT {
        return Err(StoreError::Invalid);
    }
    let sealed = sealer.seal(&raw)?;
    if sealed.is_empty() || sealed.len() > MAX_SEALED {
        return Err(StoreError::Invalid);
    }
    let mut bytes = vec![0; HEADER];
    bytes[..8].copy_from_slice(CHECKPOINT_MAGIC);
    bytes[8..16].copy_from_slice(&(sealed.len() as u64).to_le_bytes());
    bytes[16..48].copy_from_slice(&Sha256::digest(&sealed));
    bytes.extend(sealed);
    let verified = VerifiedCheckpoint::open(&bytes, sealer)?;
    if verified.stamp()? != basis.last.stamp {
        return Err(StoreError::Conflict);
    }
    Ok(verified)
}
