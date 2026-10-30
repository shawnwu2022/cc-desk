# Bounded opaque WebSocket transport

This crate consumes the shared main-repository outer protocol. `bounded_tunnel()`
returns the application byte stream and an opaque `RouteEndpoint`; only this factory
can supply route buffers. `drive` accepts a WebSocket over any admitted async byte
stream (including TLS), fixed route endpoints, cancellation watch and IO budget.
It is not wired to desktop startup and creates no listener or endpoint authority.

Limits:16 tunnels/link,64KiB per duplex direction,4 queued incoming envelopes/tunnel,
16 outgoing envelopes/link,16KiB outgoing chunks,64KiB WebSocket messages/frames,
4KiB WebSocket read buffer,128KiB max write buffer; IO budget must be >0 and <=30s.
Construction config is explicit and drive rejects oversized/unbounded caller config.
Full ingress route/duplicate validation and all queue reservations precede any
payload copying/delivery. Failure drops reservations and closes the link rather
than silently claiming reliable/lossless continuation. A stalled tunnel currently
fails the whole link; per-tunnel degraded continuation remains future work.

Memory accounting includes both duplex directions (2MiB/link at16 tunnels), incoming
queues (~4MiB) plus one active writer payload/tunnel (~1MiB), outgoing queue (~257KiB),
worker read buffers (256KiB) plus up to16 pending encoded sends (~257KiB), and bounded
WebSocket buffers/message. These are payload component bounds, not an8MiB RSS/SLO
guarantee: task/allocator/kernel/TLS overhead and caller-retained application buffers
remain separate. Global relay admission and aggregate link limits are not in this crate.

Driver polls at most one ready outgoing envelope before the next incoming event,
yields between events so single-thread route producers can run, and preserves
cancellation priority. This is not a measured weighted device/session fairness SLO.
Read deadline is absolute across fragmented pending messages, is not reset by outbound
progress or ping/pong, and resets only after a valid binary envelope. An empty valid
binary envelope counts as application activity; rate/admission policy belongs upstream.
Per-tunnel writes have finite deadlines; cancellation aborts/joins owned workers.
Any IO failure may have transferred a prefix: this layer never retries command bytes
or attests CLI execution. Dropping the drive future aborts JoinSet workers through RAII.

No identity/grant checks, inner encryption implementation, raw terminal interpretation,
credentials, native key storage or OS/network configuration lives here. `wss::connect`
builds outer TLS1.3/ring with pinned Mozilla roots and verified URL hostname,
then a bounded HTTP WebSocket upgrade. It takes an explicit resolved SocketAddr with
matching URL port. No DNS worker is spawned: bounded resolver/cancellation remains an
upstream OPEN gate. Username/password/query/fragment/non-WSS/invalid TLS names reject
before network access. The <=30s monotonic setup deadline covers TCP/TLS/HTTP upgrade;
explicit checks reject already-ready results after expiry. Synchronous CPU cannot be
preempted and sent bytes cannot be rolled back.

Actual TEST_ONLY loopback WSS tests cover valid chain/name/upgrade, normal production
roots rejecting the public test CA, wrong hostname and HTTP403. Public official test
keys/CA are cfg(test) only, never installed/enrolled. No real public origin, relay,
mobile or inner authentication acceptance is inferred. Relay admission and inner RPK/AppBind/Host integration remain mandatory before remote data can be enabled.

## Experimental native relay admission

`wss::connect_admitted(origin, address, budget, &RelayAdmissionIdentity)` retains the
production Mozilla-root TLS1.3 chain/hostname verification of `connect`, then performs
the experimental relay profile before returning a socket to `drive`. The existing
plain `connect` behavior is unchanged. The exact canonical ASCII wss endpoint is <=512
bytes; userinfo/query/fragment/percent escapes/backslashes and mismatched ports reject.
The same monotonic absolute budget covers TCP, TLS, HTTP, challenge, async signature,
proof write and success acknowledgement. Post-sign deadline checks prevent a delayed
ready signer result from authorizing a later write. CPU work cannot be preempted.

`RelayAdmissionIdentity::new(keyID16, RelayRole::{Host,Phone}, Arc<dyn RelaySigner>)`
is a separate native signer interface. The application supplies a signer dedicated to
relay admission; endpoint RPK and grant authority keys are never imported or selected
automatically. `SigningRequest` exposes immutable origin/keyID/role/transcript getters
for signer policy checks. `RelaySigner::sign` returns a bounded async future producing
raw P-256 ECDSA/SHA256 r||s64, or fixed SignError. No key generation, enrollment,
persistence, bearer tokens, renderer bridge or listener is added.

Wire: challenge `[1,0,nonce32,binding32]` (66 bytes), proof `[1,1,keyID16,role1,sig64]`
(83 bytes), accepted `[1,2]`. Transcript is ASCII `CC_DESK_RELAY_ADMISSION`, NUL,
profile byte1, origin length u16 BE, exact origin bytes, nonce32, binding32, keyID16,
role1. The binding is a server-issued random connection identifier, not a TLS exporter.
Unexpected message type/version/length/status, signer refusal/error/stall, close and
expired budget drop the socket. Returning a socket means relay admission only;
endpoint mutual RPK, AppBind, signed grants and exact live session scope still apply.

The explicit `test-only-loopback` feature is off by default and is not production
configuration. Its doc-hidden helper accepts only numeric loopback addresses with
exact fixture hostname foobar.com and matching port, validates TLS1.3 using the fixed
public fixture CA, and accepts no caller roots/config. Only tests and finite local
cross-repository qualification use it. Production `connect`/`connect_admitted` always
use Mozilla roots, including when this test feature is compiled. Never enable it in
a shipping app. No public origin, mobile bridge or release acceptance is inferred.
