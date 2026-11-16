//! Harmless test driver; never a production Host binary or remote entry point.
#[cfg(windows)]
use cc_desk_remote_host::journal::*;
#[cfg(windows)]
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 5 || args[1] != "TEST_ONLY_CRASH" {
        std::process::exit(64);
    }
    let path = std::path::PathBuf::from(&args[2]);
    let marker = std::path::PathBuf::from(&args[3]);
    let cut = args[4].to_str().unwrap();
    let key = OperationKey {
        host: [1; 32],
        session: [2; 16],
        run: "TEST_ONLY_RUN".into(),
        generation: 1,
        incarnation: 1,
        device: [3; 32],
        operation: [4; 16],
        sequence: 1,
    };
    let mut j = Journal::open(&path).unwrap();
    if cut.starts_with("settlement-") {
        if cut == "settlement-before" {
            std::process::exit(61);
        }
        let receipt = match j.receipt(&key).unwrap() {
            ReceiptLookup::Known(receipt) => receipt,
            _ => std::process::exit(65),
        };
        if cut == "settlement-committed-lost" {
            let ack = SettlementAck {
                target: key.target(),
                device: key.device,
                through: key.sequence,
                proofs: vec![receipt.into_proof().unwrap()],
            };
            assert_eq!(
                j.acknowledge(&key.device, &ack),
                Ok(AckDecision::Advanced(1))
            );
            std::process::exit(61);
        }
        if cut == "settlement-uncommitted" {
            drop(j);
            let db = rusqlite::Connection::open(&path).unwrap();
            let (scope, operation, state, proof): (Vec<u8>, Vec<u8>, u32, Vec<u8>) = db
                .query_row(
                    "SELECT scope,op,state,proof FROM operations WHERE seq=?1",
                    [&key.sequence.to_be_bytes()[..]],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
            db.execute_batch("BEGIN IMMEDIATE").unwrap();
            db.execute(
                "UPDATE floors SET settled=?1,settled_op=?2,settled_state=?3,settled_proof=?4,settled_ack_hash=zeroblob(32) WHERE scope=?5",
                rusqlite::params![
                    &key.sequence.to_be_bytes()[..],
                    operation,
                    state,
                    proof,
                    scope
                ],
            )
            .unwrap();
            db.execute(
                "DELETE FROM operations WHERE scope=?1 AND seq=?2",
                rusqlite::params![scope, &key.sequence.to_be_bytes()[..]],
            )
            .unwrap();
            // The OS closes the connection without COMMIT; SQLite must roll back both
            // floor movement and row deletion together on the next open.
            std::process::exit(61);
        }
        std::process::exit(64);
    }
    j.run_once(&key, &KeyedDigest([5; 32]), || {
        if cut == "before-effect" {
            std::process::exit(61);
        }
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&marker)
            .unwrap();
        f.write_all(b"x").unwrap();
        f.sync_all().unwrap();
        if cut == "after-effect" {
            std::process::exit(61);
        }
        Outcome::HostWritten
    })
    .unwrap();
    // Process exit skips Rust destructors/checkpoint, preserving the actual WAL cut.
    std::process::exit(61);
}
#[cfg(not(windows))]
fn main() {
    std::process::exit(64);
}
