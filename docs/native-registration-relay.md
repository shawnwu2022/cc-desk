# Registration-only Relay lifetime owner

This optional transport policy keeps an admitted registration tunnel alive while the original ceremony is awaiting its owner. It neither authorizes registration nor changes the Relay protocol. Normal enrolled transports retain their existing behavior.

## Original ownership

- Native `start_admitted_registration` receives the original admitted tunnel, IO budget, total end instant and cancellation future. The future is polled inside the same transport driver. A caller can pass a future waiting on the original `CancelToken::cancelled()`; no replacement cancellation owner is created.
- Browser `createBrowserRegistrationRelay` receives the original absolute `registrationDeadline` and required original abort signal. Remaining total time must be positive and at most 120 seconds. Native and browser IO budgets must be between 30 milliseconds and 30 seconds.
- An empty, existing per-tunnel subframe is sent every one-third of the IO budget, at most 10 seconds apart. It has no TLS bytes and is not approval, an acknowledgement or authenticated application data. It still consumes Relay routing/budget checks and the native route queue. A busy browser application write suppresses a heartbeat instead of racing that writer.
- Inbound traffic may renew the bounded IO-idle deadline, never the original total end. Outbound heartbeat cannot renew the owner's own read-idle deadline. Cancellation and total expiry stop the original driver and join its route workers and original socket owner.
- Browser timing uses both monotonic and wall elapsed time, retaining the larger value and a nondecreasing high-water mark. A backward or invalid sample permanently expires this owner. These anchors start at factory creation: the caller must retain and validate the earlier ceremony clock before invoking it. Factory construction cannot reconstruct a suspension that preceded it.

## Isolated verification, 2026-11-14

- Transport all-target tests: 53 passed. Six new virtual-time/native cases cover more than 30 seconds idle with no TLS-byte injection, exact original total expiry, outbound-only idle failure, cancellation before IO, cancellation while a write is blocked, invalid bounds and unchanged legacy-owner behavior.
- Browser focused matrix: 21 passed, including nine new registration cases. Full remote Node suite: 107 passed. Source/unit strict TypeScript, transport strict Clippy and transport default-feature check passed.
- Actual Relay WSS test: `RegistrationRelay_ExplicitLegacyWssCrosses30SecondsAndStopsAtOriginalEnd_006` uses the explicitly selected sibling Relay checkout at `7026049026d0ee42bb6d5e8b626c741506184d1b`. TLS verifies the fixed TEST_ONLY CA and server name. Both endpoints stay open for 35 seconds, then exchange opaque payloads in both directions. Three empty frames reach the peer. The browser owner closes at 40,002.612 milliseconds, preserving the original 40-second end, and the Relay exits successfully after its test policy is removed.

The test's opposite peer is a raw TEST_ONLY host using the same bounded empty frames. It is not the original native registration owner. Its browser-shaped WebSocket runs in Node, not Edge. The dedicated BrowserWasm/Edge/native approval tests are separate evidence and must not be presented as this combined path.

## Explicit limits

The specified Relay checkout is legacy-only. An initial rendezvous negotiation correctly failed with `Server sent no subprotocol`; the test now explicitly selects legacy mode, without fallback, weaker negotiation or server changes. Rendezvous registration and the Worker-through-actual-Relay combination are not accepted by this evidence.

Strict checking of the integration fixture itself remains blocked by its existing missing `ws` declarations and associated callback types; no type stubs or packages were installed to hide that gap. Its actual runtime test passed. No real account credentials, persistent enrollment, public endpoint, deployment or push is performed. Default Host entry remains Locked.
