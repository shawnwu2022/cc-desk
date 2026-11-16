# Journal independent review disposition

Fresh read-only integration_transport_review inspected the actual source and fault
tests. No remaining Critical/Important/Minor blocked this local component checkpoint.
It does not close full E07/P0 or production acceptance.

Real RED -> corrected GREEN: foreign DB mutation; missing/invalid final proof still
reported successful; AFTER UPDATE could replace final write witness; malformed row
shape misreported as client conflict; oversize main/WAL processed before cap check.
Writes now freeze exact schema under Immediate lock, verify current durable floor and
full row/readback, return Unknown+poison if final persistence is unproved. Open checks
actual SQLite qualification before target access and verifies app/schema read-only
before RW. Native guards reject reparse/hardlink and pin names for lifetime, rather
than assuming SQLITE_OPEN_NOFOLLOW works on Windows VFS. Platform behavior is explicit.

Three real self-owned process-exit cuts retain no-retry semantics; independent marker
count is0/1/1 respectively. Temp reparse/hardlink targets are unchanged; no user data,
real trust keys or services were involved. Standard-user outside-sandbox tests passed;
AdminRole=False was verified. Initial sandbox OS5 and failed regressions remain in
evidence, not described as green. Readme retains storage-growth/full-disk, ACK/floor,
rollback, Host/PTY and platform gaps rather than calling the component production-ready.

## Settlement increment

Schema2 adds an independent settled floor, exact latest-batch witness and atomic final
row retirement. Version1 opens fail read-only before mutable PRAGMAs. Receipt tokens
have redacted Debug and no public constructor; nonfinal and TooOld results have none.
The backend ACK call keeps its expected outer-authenticated device argument distinct
from the payload's self-reported device, but no authenticator or Host actor exists here.

RED evidence covered the missing API, multi-proof lost-ACK recovery and settled-prefix
identity drift. Real SQLite tests reject bare through, wrong peer/target/key/outcome/
token, duplicate/reordered/oversize proofs and unresolved holes transactionally. ACK
advance writes and revalidates floor/witness then deletes final rows in one Immediate
transaction. Process exits before ACK, before transaction commit, and after commit with
lost response preserve one effect and honest recovery.

TooOld is deliberately a scope-and-sequence prefix classification, not proof that an
operation ID existed. Only the latest exact committed batch can return AlreadySettled;
older ACKs reject without changing admission, settlement, grant, window or lease state.
Wire serialization/authentication, JSON16384 sizing, irreversible retirement and
rollback freshness remain OPEN. This backend slice is not a running or authenticated
Host and does not close E07 production acceptance.
