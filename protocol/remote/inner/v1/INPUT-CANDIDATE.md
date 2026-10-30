# Bound native input adapter candidate (minor 1)

This is a default-off candidate for the native Host adapter, not protocol freeze,
enrollment or a remotely enabled command endpoint. It follows v0.5 strict JSON,
canonical decimal counters/base64url, raw input <=8192 bytes, exact Host-issued
window deadline echo, and complete control size <=16384 bytes. The existing data
envelope consumes114 bytes, so this codec rejects JSON larger than16270 bytes
before parsing. Encoders reject rather than truncate or split a logical input.

Exact fields: `message:"InputChunk"`, `profile:1`, `operation` (nonzero16-byte
lowercase hex), `sequence` (nonzero canonical u64 decimal string), `window`
(nonzero32-byte lowercase hex opaque native eligibility token),
`notAfterHostMs` (nonzero canonical u64 decimal string), `bytes` (canonical
unpadded base64url, decoded length1..8192). Unknown/duplicate fields, other message
types/profiles, overflowing/noncanonical counters, excess bytes and padding reject.
Input Debug intentionally redacts contents. This currently represents one complete
bounded native input operation; staged Begin/Chunk/Commit orchestration remains
unimplemented and must not dispatch staging fragments through this adapter.

The opaque32 token is generated once by the native writer when issuing a window,
in addition to its existing16-byte v0.5 internal WindowId. It identifies the exact
stored window, not a portable grant or a substitute for endpoint authorization.
Its bounded serialized descriptor and receipt query/settlement syntax are now in
[CONTROL-IO-CANDIDATE.md](CONTROL-IO-CANDIDATE.md), with internal native dispatch.
Production network registration, renewal and mobile native bridge remain OPEN.
A final shared RemoteMutation schema must
freeze explicit v0.5 identities/epochs before protocol release. This compact
candidate does not authorize a protocol migration or erase that gate.

On dispatch the adapter requires the actual received record's original live TLS
owner and exact fence/scope, compares the echoed token and deadline to the
provided native SyncedView/window, derives OperationKey solely from the admitted
Host/session/run/generation/incarnation/device, and queues to the existing writer.
That writer rechecks current proof, mode/sync/dimension state, lease, fence and
Host monotonic deadline immediately before IO. It never substitutes a fresh view
for a stale token. Request digest includes exact native view/window identity.

Repeated application operation on that same window uses the durable journal;
transport sequence alone is insufficient. After reconnect, query the recorded
OperationKey and settle the exact durable receipt before another mutation. Do not
resend an old operation under a fresh window or interpret HostWritten as CLI
completion. Unknown/partial is not retry permission. General uploads are disabled.
