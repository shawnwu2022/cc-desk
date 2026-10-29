//! Backend metadata only. A journal decision never grants endpoint authorization.

const MAX_OPERATIONS: u32 = 4096;
const MAX_SCOPES: u32 = 4096;
const MAX_ACK_PROOFS: usize = 32;
const ZERO_SEQUENCE: [u8; 8] = [0; 8];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub host: [u8; 32],
    pub session: [u8; 16],
    pub run: String,
    pub generation: u64,
    pub incarnation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationKey {
    pub host: [u8; 32],
    pub session: [u8; 16],
    pub run: String,
    pub generation: u64,
    pub incarnation: u64,
    pub device: [u8; 32],
    pub operation: [u8; 16],
    pub sequence: u64,
}

impl OperationKey {
    pub fn target(&self) -> Target {
        Target {
            host: self.host,
            session: self.session,
            run: self.run.clone(),
            generation: self.generation,
            incarnation: self.incarnation,
        }
    }
}

pub struct KeyedDigest(pub [u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    HostWritten,
    Rejected,
    Partial,
    Unknown,
}

#[derive(Clone)]
pub struct ReceiptToken([u8; 32]);

impl ReceiptToken {
    pub const fn encoded_len(&self) -> usize {
        self.0.len()
    }

    fn matches(&self, other: &[u8]) -> bool {
        use subtle::ConstantTimeEq;
        other.len() == 32 && bool::from(self.0.ct_eq(other))
    }
}

impl std::fmt::Debug for ReceiptToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiptToken([REDACTED])")
    }
}

impl PartialEq for ReceiptToken {
    fn eq(&self, other: &Self) -> bool {
        self.matches(&other.0)
    }
}

impl Eq for ReceiptToken {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptEnvelope {
    pub key: OperationKey,
    pub outcome: Outcome,
    pub receipt_proof: Option<ReceiptToken>,
}

impl ReceiptEnvelope {
    pub fn into_proof(self) -> Option<ReceiptProof> {
        if !matches!(self.outcome, Outcome::HostWritten | Outcome::Rejected) {
            return None;
        }
        Some(ReceiptProof {
            key: self.key,
            outcome: self.outcome,
            proof: self.receipt_proof?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptProof {
    pub key: OperationKey,
    pub outcome: Outcome,
    proof: ReceiptToken,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettlementAck {
    pub target: Target,
    /// Self-reported by the ACK payload. It is not authentication evidence.
    pub device: [u8; 32],
    pub through: u64,
    pub proofs: Vec<ReceiptProof>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckRejectReason {
    Malformed,
    WrongDevice,
    WrongTarget,
    MissingOrMismatchedProof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckDecision {
    Advanced(u64),
    AlreadySettled(u64),
    Rejected(AckRejectReason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceiptLookup {
    Known(ReceiptEnvelope),
    Missing,
    TooOld,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalError {
    Storage,
    UnsupportedSqlite,
    Bounds,
    Conflict,
    Capacity,
    Sequence,
    Schema,
    UnsupportedPlatform,
}

pub struct Journal {
    connection: rusqlite::Connection,
    path: std::path::PathBuf,
    poisoned: bool,
    _guards: crate::path_guard::Guards,
}

impl Journal {
    pub fn open(path: &std::path::Path) -> Result<Self, JournalError> {
        let (connection, guards) = crate::journal_storage::open(path)?;
        Ok(Self {
            connection,
            path: path.to_owned(),
            poisoned: false,
            _guards: guards,
        })
    }

    pub fn run_once(
        &mut self,
        key: &OperationKey,
        digest: &KeyedDigest,
        effect: impl FnOnce() -> Outcome,
    ) -> Result<Outcome, JournalError> {
        use rusqlite::{OptionalExtension, TransactionBehavior, params};
        use subtle::ConstantTimeEq;
        if self.poisoned {
            return Err(JournalError::Storage);
        }
        let scope = scope(key)?;
        self.check_storage()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        crate::journal_storage::verify(&tx)?;
        let old = tx
            .query_row(
                "SELECT seq,digest,state,proof FROM operations WHERE scope=?1 AND op=?2",
                params![&scope, &key.operation[..]],
                read_operation,
            )
            .optional()
            .map_err(storage)?;
        if let Some((seq, stored, state, proof)) = old {
            if seq.len() != 8 || stored.len() != 32 {
                return Err(JournalError::Storage);
            }
            let recorded = decode_row(state, proof.as_deref())?;
            if seq != key.sequence.to_be_bytes() || !bool::from(stored.ct_eq(&digest.0[..])) {
                return Err(JournalError::Conflict);
            }
            return Ok(recorded);
        }
        let floor = read_floor(&tx, &scope)?;
        let last = match floor {
            Some(ref floor) => floor.admitted,
            None => {
                let scopes: u32 = tx
                    .query_row("SELECT count(*) FROM floors", [], |row| row.get(0))
                    .map_err(storage)?;
                if scopes >= MAX_SCOPES {
                    return Err(JournalError::Capacity);
                }
                0
            }
        };
        if last.checked_add(1) != Some(key.sequence) {
            return Err(JournalError::Sequence);
        }
        let count: u32 = tx
            .query_row("SELECT count(*) FROM operations", [], |row| row.get(0))
            .map_err(storage)?;
        if count >= MAX_OPERATIONS {
            return Err(JournalError::Capacity);
        }
        let unresolved: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM operations WHERE scope=?1 AND state IN (0,3,4))",
                params![&scope],
                |row| row.get(0),
            )
            .map_err(storage)?;
        if unresolved {
            return Err(JournalError::Sequence);
        }
        tx.execute(
            "INSERT INTO operations VALUES(?1,?2,?3,?4,0,NULL)",
            params![
                &scope,
                &key.operation[..],
                &key.sequence.to_be_bytes()[..],
                &digest.0[..]
            ],
        )
        .map_err(storage)?;
        tx.execute(
            "INSERT INTO floors(scope,admitted,settled,settled_op,settled_state,settled_proof,settled_ack_hash) VALUES(?1,?2,?3,NULL,NULL,NULL,NULL) ON CONFLICT(scope) DO UPDATE SET admitted=excluded.admitted",
            params![&scope, &key.sequence.to_be_bytes()[..], &ZERO_SEQUENCE[..]],
        )
        .map_err(storage)?;
        let admitted = read_floor(&tx, &scope)?.ok_or(JournalError::Storage)?;
        if admitted.admitted != key.sequence || admitted.settled > admitted.admitted {
            return Err(JournalError::Storage);
        }
        tx.commit().map_err(storage)?;

        // Durable intent precedes every externally supplied effect. No automatic retry.
        let outcome = effect();
        let state = encode_outcome(outcome);
        let persisted = (|| -> Result<(), JournalError> {
            let proof = if matches!(outcome, Outcome::HostWritten | Outcome::Rejected) {
                use ring::rand::SecureRandom;
                let mut bytes = [0; 32];
                ring::rand::SystemRandom::new()
                    .fill(&mut bytes)
                    .map_err(|_| JournalError::Storage)?;
                Some(bytes.to_vec())
            } else {
                None
            };
            let tx = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(storage)?;
            crate::journal_storage::verify(&tx)?;
            let admitted = read_floor(&tx, &scope)?.ok_or(JournalError::Storage)?;
            if admitted.admitted != key.sequence {
                return Err(JournalError::Storage);
            }
            let changed = tx
                .execute(
                    "UPDATE operations SET state=?1,proof=?2 WHERE scope=?3 AND op=?4 AND state=0",
                    params![state, proof, &scope, &key.operation[..]],
                )
                .map_err(storage)?;
            if changed != 1 {
                return Err(JournalError::Storage);
            }
            let actual = tx
                .query_row(
                    "SELECT seq,digest,state,proof FROM operations WHERE scope=?1 AND op=?2",
                    params![&scope, &key.operation[..]],
                    read_operation,
                )
                .map_err(storage)?;
            if actual.0 != key.sequence.to_be_bytes()
                || !bool::from(actual.1.ct_eq(&digest.0[..]))
                || actual.2 != state
                || actual.3 != proof
            {
                return Err(JournalError::Storage);
            }
            tx.commit().map_err(storage)?;
            Ok(())
        })();
        if persisted.is_err() {
            self.poisoned = true;
            return Ok(Outcome::Unknown);
        }
        Ok(outcome)
    }

    /// `Known` is returned only from a durable exact operation row. `TooOld` means
    /// this target/device sequence is at or below the durable settled prefix and its
    /// detail is retired; it does not attest that the supplied operation ID existed.
    pub fn receipt(&self, key: &OperationKey) -> Result<ReceiptLookup, JournalError> {
        use rusqlite::{OptionalExtension, params};
        let scope = scope(key)?;
        crate::journal_storage::verify(&self.connection)?;
        let floor = read_floor(&self.connection, &scope)?;
        if floor
            .as_ref()
            .is_some_and(|floor| key.sequence <= floor.settled)
        {
            return Ok(ReceiptLookup::TooOld);
        }
        let row = self
            .connection
            .query_row(
                "SELECT seq,digest,state,proof FROM operations WHERE scope=?1 AND op=?2",
                params![&scope, &key.operation[..]],
                read_operation,
            )
            .optional()
            .map_err(storage)?;
        if let Some((sequence, digest, state, proof)) = row {
            if sequence.len() != 8 || digest.len() != 32 {
                return Err(JournalError::Storage);
            }
            if sequence != key.sequence.to_be_bytes() {
                return Ok(ReceiptLookup::Missing);
            }
            let outcome = decode_row(state, proof.as_deref())?;
            let receipt_proof = match proof {
                Some(bytes) => Some(ReceiptToken(
                    bytes.try_into().map_err(|_| JournalError::Storage)?,
                )),
                None => None,
            };
            return Ok(ReceiptLookup::Known(ReceiptEnvelope {
                key: key.clone(),
                outcome,
                receipt_proof,
            }));
        }
        match floor {
            Some(floor) if key.sequence <= floor.admitted => {
                let sequence_exists: bool = self
                    .connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM operations WHERE scope=?1 AND seq=?2)",
                        params![&scope, &key.sequence.to_be_bytes()[..]],
                        |row| row.get(0),
                    )
                    .map_err(storage)?;
                if sequence_exists {
                    Ok(ReceiptLookup::Missing)
                } else {
                    Err(JournalError::Storage)
                }
            }
            _ => Ok(ReceiptLookup::Missing),
        }
    }

    pub fn query(&self, key: &OperationKey) -> Result<ReceiptLookup, JournalError> {
        self.receipt(key)
    }

    /// `expected_authenticated_device` must come from a future authenticated outer
    /// Host actor. This crate does not authenticate it and never derives it from the ACK.
    /// Only the latest committed ACK batch has a retained hash for exact lost-response
    /// recovery; older already-settled batches are rejected after a later advance.
    pub fn acknowledge(
        &mut self,
        expected_authenticated_device: &[u8; 32],
        ack: &SettlementAck,
    ) -> Result<AckDecision, JournalError> {
        use rusqlite::{OptionalExtension, TransactionBehavior, params};
        if self.poisoned {
            return Err(JournalError::Storage);
        }
        if ack.proofs.is_empty() || ack.proofs.len() > MAX_ACK_PROOFS || ack.through == 0 {
            return Ok(AckDecision::Rejected(AckRejectReason::Malformed));
        }
        if expected_authenticated_device != &ack.device {
            return Ok(AckDecision::Rejected(AckRejectReason::WrongDevice));
        }
        for (index, proof) in ack.proofs.iter().enumerate() {
            if proof.key.device != ack.device || proof.key.target() != ack.target {
                return Ok(AckDecision::Rejected(AckRejectReason::WrongTarget));
            }
            if !matches!(proof.outcome, Outcome::HostWritten | Outcome::Rejected) {
                return Ok(AckDecision::Rejected(AckRejectReason::Malformed));
            }
            if index > 0
                && ack.proofs[index - 1].key.sequence.checked_add(1) != Some(proof.key.sequence)
            {
                return Ok(AckDecision::Rejected(AckRejectReason::Malformed));
            }
        }
        if ack.proofs.last().map(|proof| proof.key.sequence) != Some(ack.through) {
            return Ok(AckDecision::Rejected(AckRejectReason::Malformed));
        }
        let scope = scope_for(&ack.target, &ack.device)?;
        let ack_hash = settlement_witness(&scope, ack);
        self.check_storage()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        crate::journal_storage::verify(&tx)?;
        let floor = match read_floor(&tx, &scope)? {
            Some(floor) => floor,
            None => {
                return Ok(AckDecision::Rejected(
                    AckRejectReason::MissingOrMismatchedProof,
                ));
            }
        };
        if ack.through <= floor.settled {
            let witness_matches = ack.through == floor.settled
                && floor
                    .settled_ack_hash
                    .as_deref()
                    .is_some_and(|candidate| constant_time_eq(&ack_hash, candidate));
            return if witness_matches {
                Ok(AckDecision::AlreadySettled(floor.settled))
            } else {
                Ok(AckDecision::Rejected(
                    AckRejectReason::MissingOrMismatchedProof,
                ))
            };
        }
        let expected_count = ack
            .through
            .checked_sub(floor.settled)
            .and_then(|count| usize::try_from(count).ok());
        if expected_count != Some(ack.proofs.len())
            || ack.proofs[0].key.sequence != floor.settled.checked_add(1).unwrap_or(0)
            || ack.through > floor.admitted
        {
            return Ok(AckDecision::Rejected(AckRejectReason::Malformed));
        }
        for proof in &ack.proofs {
            let row = tx
                .query_row(
                    "SELECT seq,state,proof FROM operations WHERE scope=?1 AND op=?2",
                    params![&scope, &proof.key.operation[..]],
                    |row| {
                        Ok((
                            row.get::<_, Vec<u8>>(0)?,
                            row.get::<_, u32>(1)?,
                            row.get::<_, Option<Vec<u8>>>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(storage)?;
            let Some((sequence, state, token)) = row else {
                return Ok(AckDecision::Rejected(
                    AckRejectReason::MissingOrMismatchedProof,
                ));
            };
            let token_matches = token
                .as_deref()
                .is_some_and(|candidate| proof.proof.matches(candidate));
            if sequence != proof.key.sequence.to_be_bytes()
                || decode_row(state, token.as_deref())? != proof.outcome
                || !token_matches
            {
                return Ok(AckDecision::Rejected(
                    AckRejectReason::MissingOrMismatchedProof,
                ));
            }
        }
        let witness = ack.proofs.last().ok_or(JournalError::Storage)?;
        let witness_state = encode_outcome(witness.outcome);
        let changed = tx
            .execute(
                "UPDATE floors SET settled=?1,settled_op=?2,settled_state=?3,settled_proof=?4,settled_ack_hash=?5 WHERE scope=?6 AND admitted=?7 AND settled=?8",
                params![
                    &ack.through.to_be_bytes()[..],
                    &witness.key.operation[..],
                    witness_state,
                    &witness.proof.0[..],
                    &ack_hash[..],
                    &scope,
                    &floor.admitted.to_be_bytes()[..],
                    &floor.settled.to_be_bytes()[..]
                ],
            )
            .map_err(storage)?;
        if changed != 1 {
            return Err(JournalError::Storage);
        }
        let deleted = tx
            .execute(
                "DELETE FROM operations WHERE scope=?1 AND seq>?2 AND seq<=?3 AND state IN (1,2)",
                params![
                    &scope,
                    &floor.settled.to_be_bytes()[..],
                    &ack.through.to_be_bytes()[..]
                ],
            )
            .map_err(storage)?;
        if deleted != ack.proofs.len() {
            return Err(JournalError::Storage);
        }
        let after = read_floor(&tx, &scope)?.ok_or(JournalError::Storage)?;
        if after.admitted != floor.admitted
            || after.settled != ack.through
            || after.settled_op.as_deref() != Some(&witness.key.operation[..])
            || after.settled_state != Some(witness_state)
            || !after
                .settled_proof
                .as_deref()
                .is_some_and(|candidate| witness.proof.matches(candidate))
            || !after
                .settled_ack_hash
                .as_deref()
                .is_some_and(|candidate| constant_time_eq(&ack_hash, candidate))
        {
            return Err(JournalError::Storage);
        }
        let remnants: u32 = tx
            .query_row(
                "SELECT count(*) FROM operations WHERE scope=?1 AND seq<=?2 AND state IN (1,2)",
                params![&scope, &ack.through.to_be_bytes()[..]],
                |row| row.get(0),
            )
            .map_err(storage)?;
        if remnants != 0 {
            return Err(JournalError::Storage);
        }
        tx.commit().map_err(storage)?;
        Ok(AckDecision::Advanced(ack.through))
    }

    fn check_storage(&self) -> Result<(), JournalError> {
        for (suffix, limit) in [("", 64 * 1024 * 1024), ("-wal", 16 * 1024 * 1024)] {
            let mut path = self.path.as_os_str().to_owned();
            path.push(suffix);
            match std::fs::metadata(path) {
                Ok(metadata) if metadata.len() > limit => return Err(JournalError::Capacity),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(JournalError::Storage),
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Floor {
    admitted: u64,
    settled: u64,
    settled_op: Option<Vec<u8>>,
    settled_state: Option<u32>,
    settled_proof: Option<Vec<u8>>,
    settled_ack_hash: Option<Vec<u8>>,
}

fn read_floor(
    connection: &rusqlite::Connection,
    scope: &[u8],
) -> Result<Option<Floor>, JournalError> {
    use rusqlite::{OptionalExtension, params};
    let row = connection
        .query_row(
            "SELECT admitted,settled,settled_op,settled_state,settled_proof,settled_ack_hash FROM floors WHERE scope=?1",
            params![scope],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Option<Vec<u8>>>(2)?,
                    row.get::<_, Option<u32>>(3)?,
                    row.get::<_, Option<Vec<u8>>>(4)?,
                    row.get::<_, Option<Vec<u8>>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(storage)?;
    let Some((admitted, settled, settled_op, settled_state, settled_proof, settled_ack_hash)) = row
    else {
        return Ok(None);
    };
    let admitted = u64::from_be_bytes(admitted.try_into().map_err(|_| JournalError::Storage)?);
    let settled = u64::from_be_bytes(settled.try_into().map_err(|_| JournalError::Storage)?);
    if settled > admitted {
        return Err(JournalError::Storage);
    }
    let empty_witness = settled_op.is_none()
        && settled_state.is_none()
        && settled_proof.is_none()
        && settled_ack_hash.is_none();
    let final_witness = settled_op.as_ref().is_some_and(|value| value.len() == 16)
        && settled_state.is_some_and(|value| matches!(value, 1 | 2))
        && settled_proof
            .as_ref()
            .is_some_and(|value| value.len() == 32)
        && settled_ack_hash
            .as_ref()
            .is_some_and(|value| value.len() == 32);
    if (settled == 0 && !empty_witness) || (settled != 0 && !final_witness) {
        return Err(JournalError::Storage);
    }
    Ok(Some(Floor {
        admitted,
        settled,
        settled_op,
        settled_state,
        settled_proof,
        settled_ack_hash,
    }))
}

type OperationRow = (Vec<u8>, Vec<u8>, u32, Option<Vec<u8>>);

fn read_operation(row: &rusqlite::Row<'_>) -> rusqlite::Result<OperationRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

fn scope(key: &OperationKey) -> Result<Vec<u8>, JournalError> {
    if key.sequence == 0 {
        return Err(JournalError::Bounds);
    }
    scope_for(&key.target(), &key.device)
}

fn scope_for(target: &Target, device: &[u8; 32]) -> Result<Vec<u8>, JournalError> {
    if target.run.is_empty() || target.run.len() > 128 {
        return Err(JournalError::Bounds);
    }
    let mut bytes = Vec::with_capacity(32 + 16 + 2 + target.run.len() + 8 + 8 + 32);
    bytes.extend(target.host);
    bytes.extend(target.session);
    bytes.extend((target.run.len() as u16).to_be_bytes());
    bytes.extend(target.run.as_bytes());
    bytes.extend(target.generation.to_be_bytes());
    bytes.extend(target.incarnation.to_be_bytes());
    bytes.extend(device);
    Ok(bytes)
}

fn settlement_witness(scope: &[u8], ack: &SettlementAck) -> [u8; 32] {
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    digest.update(b"cc-desk-settlement-ack-v1");
    digest.update(&(scope.len() as u16).to_be_bytes());
    digest.update(scope);
    digest.update(&ack.through.to_be_bytes());
    digest.update(&[ack.proofs.len() as u8]);
    for proof in &ack.proofs {
        digest.update(&proof.key.operation);
        digest.update(&proof.key.sequence.to_be_bytes());
        digest.update(&[encode_outcome(proof.outcome) as u8]);
        digest.update(&proof.proof.0);
    }
    let value = digest.finish();
    let mut bytes = [0; 32];
    bytes.copy_from_slice(value.as_ref());
    bytes
}

fn constant_time_eq(expected: &[u8; 32], candidate: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    candidate.len() == 32 && bool::from(expected.ct_eq(candidate))
}

fn encode_outcome(outcome: Outcome) -> u32 {
    match outcome {
        Outcome::HostWritten => 1,
        Outcome::Rejected => 2,
        Outcome::Partial => 3,
        Outcome::Unknown => 4,
    }
}

fn decode_row(state: u32, proof: Option<&[u8]>) -> Result<Outcome, JournalError> {
    if matches!(state, 1 | 2) {
        if proof.is_none_or(|proof| proof.len() != 32) {
            return Err(JournalError::Storage);
        }
    } else if proof.is_some() {
        return Err(JournalError::Storage);
    }
    match state {
        0 | 4 => Ok(Outcome::Unknown),
        1 => Ok(Outcome::HostWritten),
        2 => Ok(Outcome::Rejected),
        3 => Ok(Outcome::Partial),
        _ => Err(JournalError::Storage),
    }
}

fn storage(_: rusqlite::Error) -> JournalError {
    JournalError::Storage
}
