use crate::{journal::JournalError, path_guard::Guards};
use rusqlite::{Connection, OpenFlags};
const APP: i64 = 0x43434452;
const SCHEMA: [(&str, &str, &str); 3] = [
    (
        "floors",
        "table",
        "CREATE TABLE floors(scope BLOB NOT NULL PRIMARY KEY,admitted BLOB NOT NULL CHECK(length(admitted)=8),settled BLOB NOT NULL CHECK(length(settled)=8),settled_op BLOB,settled_state INTEGER,settled_proof BLOB,settled_ack_hash BLOB,CHECK((settled=X'0000000000000000' AND settled_op IS NULL AND settled_state IS NULL AND settled_proof IS NULL AND settled_ack_hash IS NULL) OR (settled<>X'0000000000000000' AND length(settled_op)=16 AND settled_state IN (1,2) AND length(settled_proof)=32 AND length(settled_ack_hash)=32))) STRICT",
    ),
    (
        "operations",
        "table",
        "CREATE TABLE operations(scope BLOB NOT NULL,op BLOB NOT NULL CHECK(length(op)=16),seq BLOB NOT NULL CHECK(length(seq)=8),digest BLOB NOT NULL CHECK(length(digest)=32),state INTEGER NOT NULL CHECK(state IN (0,1,2,3,4)),proof BLOB,PRIMARY KEY(scope,op),CHECK((state IN (1,2) AND proof IS NOT NULL AND length(proof)=32) OR (state IN (0,3,4) AND proof IS NULL))) STRICT",
    ),
    (
        "unresolved",
        "index",
        "CREATE UNIQUE INDEX unresolved ON operations(scope) WHERE state IN (0,3,4)",
    ),
];
fn error(_: rusqlite::Error) -> JournalError {
    JournalError::Storage
}
pub(crate) fn verify(c: &Connection) -> Result<(), JournalError> {
    let app: i64 = c
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(error)?;
    let version: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(error)?;
    if app != APP || version != 2 {
        return Err(JournalError::Schema);
    }
    let mut stmt = c
        .prepare(
            "SELECT name,type,sql FROM sqlite_schema WHERE substr(lower(name),1,7) <> 'sqlite_' ORDER BY name",
        )
        .map_err(error)?;
    let actual = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(error)?;
    let mut expected = SCHEMA.to_vec();
    expected.sort_by_key(|v| v.0);
    if actual.len() != expected.len()
        || actual
            .iter()
            .zip(expected)
            .any(|(a, b)| a.0 != b.0 || a.1 != b.1 || a.2 != b.2)
    {
        return Err(JournalError::Schema);
    }
    Ok(())
}
pub fn open(path: &std::path::Path) -> Result<(Connection, Guards), JournalError> {
    qualify()?;
    let (guards, created) = crate::path_guard::acquire(path)?;
    let base = OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    if !created {
        let ro = Connection::open_with_flags(path, base | OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(error)?;
        verify(&ro)?;
    }
    let mut c = Connection::open_with_flags(path, base | OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(error)?;
    if !created {
        verify(&c)?;
    }
    c.busy_timeout(std::time::Duration::ZERO).map_err(error)?;
    c.execute_batch("PRAGMA page_size=4096;PRAGMA max_page_count=16384;PRAGMA synchronous=FULL;PRAGMA wal_autocheckpoint=32;PRAGMA temp_store=MEMORY;PRAGMA trusted_schema=OFF;").map_err(error)?;
    let mode: String = c
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .map_err(error)?;
    let sync: u32 = c
        .query_row("PRAGMA synchronous", [], |r| r.get(0))
        .map_err(error)?;
    let page: u32 = c
        .query_row("PRAGMA page_size", [], |r| r.get(0))
        .map_err(error)?;
    if mode != "wal" || sync != 2 || page != 4096 {
        return Err(JournalError::Storage);
    }
    if created {
        let tx = c.transaction().map_err(error)?;
        for (_, _, sql) in SCHEMA {
            tx.execute_batch(sql).map_err(error)?;
        }
        tx.pragma_update(None, "application_id", APP)
            .map_err(error)?;
        tx.pragma_update(None, "user_version", 2).map_err(error)?;
        tx.commit().map_err(error)?;
        let busy: u32 = c
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
            .map_err(error)?;
        if busy != 0 {
            return Err(JournalError::Storage);
        }
    }
    verify(&c)?;
    Ok((c, guards))
}
fn qualify() -> Result<(), JournalError> {
    use rusqlite::ffi;
    if rusqlite::version_number() != 3_053_002 || unsafe { ffi::sqlite3_threadsafe() } == 0 {
        return Err(JournalError::UnsupportedSqlite);
    }
    let source = unsafe { ffi::sqlite3_sourceid() };
    if source.is_null() || unsafe{std::ffi::CStr::from_ptr(source)}.to_bytes()!=b"2026-06-03 19:12:13 d6e03d8c777cfa2d35e3b60d8ec3e0187f3e9f99d8e2ee9cac695fd6fcdf1a24" {return Err(JournalError::UnsupportedSqlite);}
    if unsafe { ffi::sqlite3_compileoption_used(c"THREADSAFE=1".as_ptr()) } == 0 {
        return Err(JournalError::UnsupportedSqlite);
    }
    for flag in [
        c"NO_SYNC",
        c"OMIT_WAL",
        c"OMIT_DISKIO",
        c"OMIT_COMPILEOPTION_DIAGS",
    ] {
        if unsafe { ffi::sqlite3_compileoption_used(flag.as_ptr()) } != 0 {
            return Err(JournalError::UnsupportedSqlite);
        }
    }
    Ok(())
}
