# Existing-writer Attach adapter: memory IO qualification

Code is connected to the real writer actor, not a real PTY or production route.
No Create/Close implementation, process creation, enrollment, credentials, schema
migration, WebView/CallerIdentity bypass or system/network permissions are added.

## Exact seam and custody

local_writer_owner.rs includes private local_writer_attach.rs. AttachBackend
borrows the existing owner via Arc<Mutex<WriterHostOwner>>, derives resource identity
from that owner/Target/principal, and receives local device/lease/baseline/stable ID.
No identity or launch recipe is decoded from remote data. Constructor requires an
existing exact Target; it creates no owner or writer. Rights/controller checks,
live bound scope, generation, grant clocks, key/policy epochs, device and fence
admission remain in the existing authenticated path.

Host::attach_with_completion -> WriterHostOwner::rebind_authorized(expected fence)
-> AuthorizedPendingRebind -> RebindRequest::process on the SAME writer actor.
The replacement enforces expected fence and installs exactly expected+1, revokes
old scope and clears accepted/current view. Owner try_lock covers only enqueue;
there is no global owner/DB lock while awaiting actor work. One pending Attach
or installed peer per adapter prevents repeated native enqueue.

Host::poll_attach uses zero-duration waits on the native pending handle. Once a
peer is returned, a bounded native capture is requested under its existing output
authorization, then polled without blocking. The observed generation/mode/
dimension/sync stamp must equal the strict completion successor; extra capture or
terminal mode drift becomes Unknown rather than publishing guessed epochs.
The fence is expected+1 guaranteed by the native rebind wrapper/actor, not supplied
by a callback DTO. The capture is dropped without delivery or ACK. It supplies
no input window and does not make the peer synchronized or input-eligible.

Only then does the private Sender deposit installation evidence; Durable commits
the sealed typed result before Router state/cache and authorized disclosure.
The private single-use connection in [POST-ATTACH-CONNECTION.md](POST-ATTACH-CONNECTION.md)
is released after cache publication; it still requires delivery/ACK/window.
No production getter exposes the raw peer. Generic history recovery defaults
deny; reviewed fake recovery explicitly opts in, and this native adapter stays
deny. Historical SQL cannot prove a real peer or writer has been recovered.

## Timeout/failure semantics

The local budget is >0 and <=1s, with one monotonic deadline retained across
native wait, capture and final evidence publication checks. This is an observation
budget, NOT a hard native mutation, fsync, cleanup or retirement deadline.
Actor rejection, closed wait, drift, failed capture or expired observation becomes
Unknown/Uncertain and does not retry. Already queued rebind can install later;
pending handles/peer and owner remain retained until adapter teardown/reconciliation.
No cancellation or no-effect promise is inferred from timeout. Native callback
errors attempt durable Uncertain; storage errors still return failure and retain
the durable/pending fail-closed state.

## Executed boundary matrix

The original memory-writer test ran9 cases, now extended to19 with the connection
chain in POST-ATTACH-CONNECTION.md: successful install with premature ACK refused;
extra native capture; supported focus-mode epoch drift; blocked fake IO followed
by timeout and demonstrated late fence5 install; queued revocation leaving fence4;
stale expected fence leaving fence4; expired grant before admission; owner lock
contention returning no-queue Pending; and deadline crossed just before publish.
Distinct pending request is refused before any second native admission. A second
test verifies default-deny history policy. Cleanup seals and joins only the owned
memory writer with bounded waits; no process tree/PTY cleanup is claimed.

Native adapter RED/GREEN is separate from existing fakeHost recovery. Tests run
only memory TLS/public TEST_ONLY keys, fake Write IO and task-owned temporary DBs.
No actual CLI/account, OS child/PTY or historical unknown failure gate is run.

OPEN: production registration/peer handoff, full real writer/process/IO lifetime,
native owner reconstruction, actual network/phone delivery and permissions,
Win11/device/CLI acceptance, capacity/operations and old OS5/WebView/handle gates.
Wire/native size bounds and generation conversion remain future gates unchanged.
