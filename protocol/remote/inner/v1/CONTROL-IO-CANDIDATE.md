# Native control replies, candidate profile 1

`control_io.rs` supplies bounded phone-facing syntax for the installed native
connection, not a listener or authorization. Payloads remain inside the original
authenticated Data envelope: client Control direction, Host Projection direction.
That direction tag does not mean every Host payload is a grid. The native adapter
checks original channel, scope, fence and live grant before disclosure.

All messages require profile1. Unknown/duplicate fields, enum-map aliases,
floating spellings of integer fields, invalid UTF-8, depth above16 and payloads
above16270 bytes reject. All counters below are nonzero canonical u64 decimal
strings. Operation IDs are nonzero16-byte lowercase hex; tokens/proofs32-byte hex.
Nullable fields must be present explicitly.

| Message | Other exact fields |
|---|---|
| WindowGranted | request, window, notAfterHostMs |
| ProjectionAcknowledged | exact existing projection stamp |
| InputReceipt | operation, sequence, outcome, proof |
| InputNotAccepted | operation, sequence |
| ReceiptQuery | request, operation, sequence |
| ReceiptStatus | request, operation, sequence, status, outcome, proof |
| ReceiptAck | request, operation, sequence, outcome, proof |
| ReceiptSettled | request, operation, sequence, outcome, disposition |

Echo the Host-issued deadline unchanged; phone time never renews it. Request
counters are monotone within a connection; operation identity is independent.
Only durable HostWritten/Rejected outcomes carry a proof. Partial/Unknown require
null and cannot settle. HostWritten is Host IO acceptance, not CLI completion.
InputNotAccepted represents known pre-write refusal, never storage/IO uncertainty.
Actual uncertain IO can close native authority before even an Unknown reply is
disclosed: connection failure is not no-effect evidence or retry permission.

ReceiptStatus Known follows the same outcome/proof rules; Missing/TooOld have null
outcome/proof and neither attests supplied operation completion. An already held
exact final proof is retained across those statuses for an explicit identical ACK;
no proof is invented. Native settlement checks current target/device, operation,
sequence, outcome, token, pending identity and durable retained ACK witness.
Only Advanced/AlreadySettled can settle; Rejected keeps input pending. A bare
settled prefix, fresh operation ID or inferred receipt cannot replace a lost result.

`local_writer_control_reply.rs` dispatches to the existing authorized peer. One
original operation deadline covers native waits and reply delivery. Error or
cancellation closes only its original data owner and cannot undo in-flight IO.
Queries remain available while busy with original authority checks. Settlement
never creates a snapshot/ACK/window or unlocks unrelated pending work.

The TS client preserves projection continuity across rebind and requires complete
fresh delivery, exact ACK and a Host window before input. Unsettled input survives
disconnect in memory. Durable mobile recovery, encrypted native bridge, enrollment,
key storage, production registration, phone/real-CLI and full compatibility
admission remain OPEN; this is not protocol freeze. Unsupported peers fail closed.

`tests/control_io.rs` optionally emits public TEST_ONLY JSONL via create_new at
CC_DESK_TEST_ONLY_CONTROL_JSONL. The Node native.control.integration.ts consumes
those actual encoder bytes; that is codec evidence, not a phone/network test.
Host tests separately use actual mutual-RPK/AppBind over bounded memory IO and a
temporary journal. No real CLI or deployed relay is exercised by those tests.
