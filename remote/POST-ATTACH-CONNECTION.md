# Post-Attach bound connection: memory IO candidate

Scope: same existing writer's installed peer -> fresh native snapshot actual TLS
delivery -> exact ACK -> bounded input window -> existing bound input/receipt.
No Create/Resize/Close, production registration, CLI/PTY, account, persistent grant,
schema migration, external listener or WebView CallerIdentity path is added.

## Handoff ordering

Router::complete_attach first receives the durable sealed result, updates its
cache/state, then calls Durable::publish_attach -> native adapter publication.
The adapter requires the exact observed Applied value and retains the original
authority. Its private connection factory is unavailable before publication and
can be taken once. Dropping a connection never resets that eligibility. A test
cuts after durable commit/before cache and confirms no connection can be obtained.
Raw AuthorizedPeer is not exported and no production route invokes this factory.

local_writer_attach_connection.rs is independent of the protected uncommitted
local_writer_router.rs. It reuses the committed AuthorizedPeer capture,
acknowledge_received, input_window/window_token and submit_received APIs.

## Authorization and continuity

Each request retains the original live source channel, scope and installed fence;
snapshot output channel must also be that original owner. Per-connection bounded
Snapshot/Window counters reject repeats. All wire payloads use existing strict
command/input/ACK decoders. No CLI software version is tested or restricted.

Snapshot clears old view/ACK eligibility before await, captures on the same writer,
then consumes frame.deliver_bound. Only after the complete multipart write and
live-authority/deadline checks is its exact stamp eligible for ACK. No manual
delivered flag exists. ACK requires that exact stamp once and the actor's current
epoch/source/delivery checks. Window requires the returned SyncedView and existing
lease/grant/epoch checks; its token/deadline descriptor is internal, not a new
mobile reply schema. Attach completion alone supplies neither view nor window.

Input requires that window, exact source/fence/token/deadline and existing native
single-writer journal checks. HostWritten is host IO acceptance only, not CLI
completion. Exact duplicate input returns the original receipt with one fake-IO
write; a new identity is not automatically generated or submitted.

Busy is reserved before awaits/queue work. One absolute <=1s per request covers
blocking-pool queue, native wait and snapshot delivery budget. Drop aborts only
the exact queued blocking wait; already running waits and native operations may
finish later. Snapshot cancellation/partial failure uses the existing sending
guard to revoke the scope. Other uncertain/cancelled waits keep busy and never
make a view/window available. Any failure after admission is not a no-effect
promise: a late admitted input can write once. No operation is blindly retried.
Connection Drop discards local view/stamp without killing the writer or resetting
the adapter's one-time handoff. Explicit receipt/reconnect reconciliation is OPEN.

## Qualification

Only memory TLS/public TEST_ONLY identities, existing memory writer with fake Write
IO and task-owned temporary SQLite are used. The fixture receives/assembles actual
multipart records before ACK. Its19-case actor matrix includes the original native
Attach negatives plus positive full chain, wrong peer/fence/old/altered stamp,
premature/repeated ACK and early window; large partial capture cancelled, slow
receiver timeout, expiry/revoke during send; ACK/window/input waits cancelled
behind blocked fake IO; expired window and revoked grant before input; and handoff
before durable/cache publication. Factory remains unavailable after Drop. Late
input after cancellation is counted once and further input rejected.

The complete standalone protocol and selected lifecycle tests are separate checks.
No full Host/remote suite, real PTY, phone, CLI, Win11, throughput/SLO or historical
unknown failure gate is qualified. Production caller/peer handoff, mobile response
codecs, real runtime/IO retirement, key/authorization provisioning, reconnect and
receipt reconciliation, capacity/operations and old OS5/WebView/handle gates remain
OPEN. No safety limit, transport credit or timeout was increased to pass fixtures.
