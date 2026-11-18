//! Candidate lifecycle metadata only; never migrates the PTY journal.
use crate::journal::JournalError as Error;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Prepared,
    InFlight,
    Accepted,
    Completed,
    Uncertain,
    Rejected(u8),
}
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};
// Existing guard recognizes the CC Desk application family. Version+SQL below identify this candidate only.
const APP: i64 = 0x43434452;
const LEGACY_VERSION: i64 = 0x4c4a0001;
const VERSION: i64 = 0x4c4a0002;
const RESIZE_VERSION: i64 = 0x4c4a0003;
const SQL: [&str; 5] = [
    "CREATE TABLE binding(id INTEGER NOT NULL PRIMARY KEY CHECK(id=1),witness BLOB NOT NULL CHECK(length(witness)=32)) STRICT",
    "CREATE TABLE floors(scope BLOB NOT NULL PRIMARY KEY CHECK(length(scope)=32),seq BLOB NOT NULL CHECK(length(seq)=8)) STRICT",
    "CREATE TABLE ops(scope BLOB NOT NULL CHECK(length(scope)=32),op BLOB NOT NULL CHECK(length(op)=16),seq BLOB NOT NULL CHECK(length(seq)=8),digest BLOB NOT NULL CHECK(length(digest)=32),state INTEGER NOT NULL CHECK(state BETWEEN 0 AND 5),code INTEGER,PRIMARY KEY(scope,op),UNIQUE(scope,seq),CHECK((state=5 AND code BETWEEN 0 AND 6) OR (state<>5 AND code IS NULL))) STRICT",
    "CREATE UNIQUE INDEX unresolved ON ops(scope) WHERE state IN (0,1,2,4)",
    "CREATE TABLE results(scope BLOB NOT NULL CHECK(length(scope)=32),op BLOB NOT NULL CHECK(length(op)=16),backend BLOB NOT NULL CHECK(length(backend)=32),payload BLOB NOT NULL CHECK(length(payload) BETWEEN 48 AND 4096),seal BLOB NOT NULL CHECK(length(seal)=32),PRIMARY KEY(scope,op)) STRICT",
];
pub(super) struct Ticket {
    owner: Weak<()>,
    _ownership: Arc<crate::path_guard::Ownership>,
    scope: [u8; 32],
    op: [u8; 16],
    seq: u64,
    digest: [u8; 32],
}
pub(super) enum Prepare {
    New(Ticket),
    Recorded(Phase),
}
pub(super) struct Store {
    db: Connection,
    path: PathBuf,
    _guards: crate::path_guard::Guards,
    owner: Arc<()>,
    ownership: Arc<crate::path_guard::Ownership>,
    poisoned: bool,
    legacy: bool,
    readonly: bool,
    candidate: bool,
    #[cfg(test)]
    pub fault: Option<Cut>,
}
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Cut {
    BeforePrepare,
    AfterPrepare,
    BeforeTransition,
    AfterTransition,
}
fn storage(_: rusqlite::Error) -> Error {
    Error::Storage
}
fn encode(p: Phase) -> (i64, Option<u8>) {
    match p {
        Phase::Prepared => (0, None),
        Phase::InFlight => (1, None),
        Phase::Accepted => (2, None),
        Phase::Completed => (3, None),
        Phase::Uncertain => (4, None),
        Phase::Rejected(c) => (5, Some(c)),
    }
}
fn phase(s: i64, c: Option<u8>) -> Result<Phase, Error> {
    match (s, c) {
        (0, None) => Ok(Phase::Prepared),
        (1, None) => Ok(Phase::InFlight),
        (2, None) => Ok(Phase::Accepted),
        (3, None) => Ok(Phase::Completed),
        (4, None) => Ok(Phase::Uncertain),
        (5, Some(c)) if c <= 6 => Ok(Phase::Rejected(c)),
        _ => Err(Error::Schema),
    }
}
fn verify(db: &Connection) -> Result<(), Error> {
    verify_format(db, false)
}
fn verify_format(db: &Connection, candidate: bool) -> Result<(), Error> {
    let app: i64 = db
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(storage)?;
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(storage)?;
    if app != APP
        || !([LEGACY_VERSION, VERSION].contains(&version) || candidate && version == RESIZE_VERSION)
    {
        return Err(Error::Schema);
    }
    let mut stmt = db
        .prepare("SELECT sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY sql")
        .map_err(storage)?;
    let actual = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage)?;
    let mut wanted = SQL[..if version == LEGACY_VERSION { 4 } else { 5 }]
        .iter()
        .map(|s| (*s).to_owned())
        .collect::<Vec<_>>();
    wanted.sort();
    if actual != wanted {
        return Err(Error::Schema);
    }
    Ok(())
}
impl Store {
    fn resize_format(&self) -> bool {
        self.candidate
            && self
                .db
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .ok()
                == Some(RESIZE_VERSION)
    }
    pub(crate) fn resize_writable(&self) -> bool {
        self.resize_format() && !self.readonly
    }
    pub fn bind_key(&mut self, witness: [u8; 32]) -> Result<(), Error> {
        self.check()?;
        if self.readonly {
            let old: Vec<u8> = self
                .db
                .query_row("SELECT witness FROM binding WHERE id=1", [], |r| r.get(0))
                .map_err(storage)?;
            use subtle::ConstantTimeEq;
            return if bool::from(old.ct_eq(&witness)) {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let old: Option<Vec<u8>> = tx
            .query_row("SELECT witness FROM binding WHERE id=1", [], |r| r.get(0))
            .optional()
            .map_err(storage)?;
        if let Some(old) = old {
            use subtle::ConstantTimeEq;
            return if bool::from(old.ct_eq(&witness)) {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        let occupied: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM ops) OR EXISTS(SELECT 1 FROM floors)",
                [],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if occupied {
            return Err(Error::Schema);
        }
        tx.execute("INSERT INTO binding VALUES(1,?1)", params![witness])
            .map_err(storage)?;
        let actual: Vec<u8> = tx
            .query_row("SELECT witness FROM binding WHERE id=1", [], |r| r.get(0))
            .map_err(storage)?;
        if actual != witness {
            self.poisoned = true;
            return Err(Error::Storage);
        }
        if tx.commit().is_err() {
            self.poisoned = true;
            return Err(Error::Storage);
        }
        Ok(())
    }
    pub fn open(p: &Path) -> Result<Self, Error> {
        Self::open_inner(p, false)
    }
    #[cfg(test)]
    pub(crate) fn open_resize_candidate(p: &Path) -> Result<Self, Error> {
        Self::open_inner(p, true)
    }
    fn open_inner(p: &Path, candidate: bool) -> Result<Self, Error> {
        crate::journal_storage::qualify()?;
        let (guards, created) = crate::path_guard::acquire(p)?;
        let flags = OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        if !created {
            let ro = Connection::open_with_flags(p, flags | OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(storage)?;
            verify_format(&ro, candidate)?;
            let version: i64 = ro
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .map_err(storage)?;
            if version == LEGACY_VERSION || candidate {
                let ownership = guards.ownership();
                return Ok(Self {
                    db: ro,
                    path: p.to_owned(),
                    _guards: guards,
                    owner: Arc::new(()),
                    ownership,
                    poisoned: false,
                    legacy: version == LEGACY_VERSION,
                    readonly: true,
                    candidate,
                    #[cfg(test)]
                    fault: None,
                });
            }
        }
        let mut db = Connection::open_with_flags(p, flags | OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(storage)?;
        db.busy_timeout(std::time::Duration::ZERO)
            .map_err(storage)?;
        db.execute_batch("PRAGMA page_size=4096;PRAGMA max_page_count=4096;PRAGMA synchronous=FULL;PRAGMA wal_autocheckpoint=32;PRAGMA temp_store=MEMORY;PRAGMA trusted_schema=OFF;").map_err(storage)?;
        let mode: String = db
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
            .map_err(storage)?;
        let sync: i64 = db
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .map_err(storage)?;
        if mode != "wal" || sync != 2 {
            return Err(Error::Storage);
        }
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        if created {
            for sql in SQL {
                tx.execute_batch(sql).map_err(storage)?;
            }
            tx.pragma_update(None, "application_id", APP)
                .map_err(storage)?;
            tx.pragma_update(
                None,
                "user_version",
                if candidate { RESIZE_VERSION } else { VERSION },
            )
            .map_err(storage)?;
        } else {
            verify_format(&tx, candidate)?;
            tx.execute(
                "UPDATE ops SET state=4,code=NULL WHERE state IN (0,1,2)",
                [],
            )
            .map_err(storage)?;
            tx.execute("UPDATE ops SET state=4,code=NULL WHERE state=3 AND NOT EXISTS(SELECT 1 FROM results r WHERE r.scope=ops.scope AND r.op=ops.op)",[]).map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        if created {
            let busy: i64 = db
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
                .map_err(storage)?;
            if busy != 0 {
                return Err(Error::Storage);
            }
        }
        verify_format(&db, candidate)?;
        let ownership = guards.ownership();
        Ok(Self {
            db,
            path: p.to_owned(),
            _guards: guards,
            owner: Arc::new(()),
            ownership,
            poisoned: false,
            legacy: false,
            readonly: false,
            candidate,
            #[cfg(test)]
            fault: None,
        })
    }
    fn check(&self) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Storage);
        }
        verify_format(&self.db, self.candidate)?;
        for (suffix, limit) in [("", 16 * 1024 * 1024), ("-wal", 16 * 1024 * 1024)] {
            let mut p = self.path.as_os_str().to_owned();
            p.push(suffix);
            match std::fs::metadata(p) {
                Ok(m) if m.len() > limit => return Err(Error::Capacity),
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => return Err(Error::Storage),
            }
        }
        Ok(())
    }
    pub fn prepare(
        &mut self,
        scope: [u8; 32],
        op: [u8; 16],
        seq: u64,
        digest: [u8; 32],
    ) -> Result<Prepare, Error> {
        self.check()?;
        if scope == [0; 32] || op == [0; 16] || seq == 0 {
            return Err(Error::Bounds);
        }
        let tx = self
            .db
            .transaction_with_behavior(if self.readonly {
                TransactionBehavior::Deferred
            } else {
                TransactionBehavior::Immediate
            })
            .map_err(storage)?;
        let row = tx
            .query_row(
                "SELECT seq,digest,state,code FROM ops WHERE scope=?1 AND op=?2",
                params![scope, op],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, Option<u8>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(storage)?;
        if let Some((old, hash, s, c)) = row {
            use subtle::ConstantTimeEq;
            if old != seq.to_be_bytes() || !bool::from(hash.ct_eq(&digest)) {
                return Err(Error::Conflict);
            }
            let p = phase(s, c)?;
            return Ok(Prepare::Recorded(
                if (self.legacy && !matches!(p, Phase::Rejected(_)))
                    || (self.readonly
                        && matches!(p, Phase::Prepared | Phase::InFlight | Phase::Accepted))
                {
                    Phase::Uncertain
                } else {
                    p
                },
            ));
        }
        if self.readonly {
            return Err(Error::Sequence);
        }
        let sequence_exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM ops WHERE scope=?1 AND seq=?2)",
                params![scope, &seq.to_be_bytes()],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if sequence_exists {
            return Err(Error::Conflict);
        }
        let floor: Option<Vec<u8>> = tx
            .query_row(
                "SELECT seq FROM floors WHERE scope=?1",
                params![scope],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        let last = match floor {
            Some(b) => u64::from_be_bytes(b.try_into().map_err(|_| Error::Schema)?),
            None => 0,
        };
        if last.checked_add(1) != Some(seq) {
            return Err(Error::Sequence);
        }
        let pending: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM ops WHERE scope=?1 AND state IN (0,1,2,4))",
                params![scope],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if pending {
            return Err(Error::Sequence);
        }
        let count: i64 = tx
            .query_row("SELECT count(*) FROM ops", [], |r| r.get(0))
            .map_err(storage)?;
        let scopes: i64 = tx
            .query_row("SELECT count(*) FROM floors", [], |r| r.get(0))
            .map_err(storage)?;
        if count >= 4096 || (last == 0 && scopes >= 1024) {
            return Err(Error::Capacity);
        }
        tx.execute(
            "INSERT INTO ops VALUES(?1,?2,?3,?4,0,NULL)",
            params![scope, op, &seq.to_be_bytes(), digest],
        )
        .map_err(storage)?;
        tx.execute(
            "INSERT INTO floors VALUES(?1,?2) ON CONFLICT(scope) DO UPDATE SET seq=excluded.seq",
            params![scope, &seq.to_be_bytes()],
        )
        .map_err(storage)?;
        let witness: (Vec<u8>, Vec<u8>, i64, Option<u8>) = tx
            .query_row(
                "SELECT seq,digest,state,code FROM ops WHERE scope=?1 AND op=?2",
                params![scope, op],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(storage)?;
        let floor: Vec<u8> = tx
            .query_row(
                "SELECT seq FROM floors WHERE scope=?1",
                params![scope],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if witness != (seq.to_be_bytes().to_vec(), digest.to_vec(), 0, None)
            || floor != seq.to_be_bytes()
        {
            self.poisoned = true;
            return Err(Error::Storage);
        }
        #[cfg(test)]
        if self.fault == Some(Cut::BeforePrepare) {
            self.poisoned = true;
            return Err(Error::Storage);
        }
        if tx.commit().is_err() {
            self.poisoned = true;
            return Err(Error::Storage);
        }
        #[cfg(test)]
        if self.fault == Some(Cut::AfterPrepare) {
            self.poisoned = true;
            return Err(Error::Storage);
        }
        Ok(Prepare::New(Ticket {
            owner: Arc::downgrade(&self.owner),
            _ownership: self.ownership.clone(),
            scope,
            op,
            seq,
            digest,
        }))
    }
    pub fn transition(&mut self, t: &Ticket, next: Phase) -> Result<(), Error> {
        self.check()?;
        if self.readonly {
            return Err(Error::Schema);
        }
        if !Weak::ptr_eq(&t.owner, &Arc::downgrade(&self.owner)) {
            return Err(Error::Conflict);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let (s, c, hash, seq) = tx
            .query_row(
                "SELECT state,code,digest,seq FROM ops WHERE scope=?1 AND op=?2",
                params![t.scope, t.op],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .map_err(storage)?;
        use subtle::ConstantTimeEq;
        if seq != t.seq.to_be_bytes() || !bool::from(hash.ct_eq(&t.digest)) {
            return Err(Error::Conflict);
        }
        let old = phase(s, c)?;
        if old == next {
            return Ok(());
        }
        if !matches!(
            (old, next),
            (Phase::Prepared, Phase::InFlight)
                | (
                    Phase::InFlight,
                    Phase::Accepted | Phase::Uncertain | Phase::Rejected(_)
                )
                | (Phase::Accepted, Phase::Completed | Phase::Uncertain)
        ) {
            return Err(Error::Conflict);
        }
        let (n, code) = encode(next);
        if matches!(next,Phase::Rejected(c)if c>6) {
            return Err(Error::Bounds);
        }
        tx.execute(
            "UPDATE ops SET state=?1,code=?2 WHERE scope=?3 AND op=?4 AND state=?5",
            params![n, code, t.scope, t.op, s],
        )
        .map_err(storage)?;
        let check: (i64, Option<u8>, Vec<u8>, Vec<u8>) = tx
            .query_row(
                "SELECT state,code,digest,seq FROM ops WHERE scope=?1 AND op=?2",
                params![t.scope, t.op],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(storage)?;
        if check != (n, code, t.digest.to_vec(), t.seq.to_be_bytes().to_vec()) {
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
}
#[cfg(all(test, windows))]
#[path = "lifecycle_recovery_tests.rs"]
mod recovery_tests;
#[cfg(test)]
#[path = "lifecycle_resize_journal_tests.rs"]
mod resize_tests;
#[path = "lifecycle_journal_results.rs"]
mod results;
#[cfg(all(test, windows))]
#[path = "lifecycle_journal_tests.rs"]
mod tests;
