# Raw record transport checkpoint

The production transport crate now consumes shared endpoint framing and provides
bounded RecordReader/RecordWriter over generic AsyncRead/AsyncWrite. It is a
component prerequisite, not an authenticated endpoint or running remote Host.

Each call accepts a positive budget <=30 seconds. One checked absolute monotonic
deadline covers the whole header/body or encoded write/flush, using the existing
strict pre/post deadline helper; per-read progress cannot refresh that deadline.
The complete8-byte header is validated before allocating at most32768 body bytes.
No internal queue or batch of received records is retained. Writer validates the
type-specific budget before allocating/emitting its at-most32776-byte envelope.
Caller-owned records, TLS/socket buffers and simultaneous writer storage still
need reservations in the future global payload ledger.

Once valid IO work begins, the adapter moves its supplied IO out of itself. Error,
deadline, clean EOF or dropped future discards that IO and leaves the adapter
retired. It cannot resume a partial frame or retry a partial write. For a split
half, external clone or shared owner this does not guarantee physical connection
or sibling-direction closure. Future E02 channel ownership must invalidate the
context, cancel both directions and reset the TLS stream on either failure.
Write success acknowledges byte transport only, never HostWritten/CLI completion.

Actual RED evidence: initial framing3 failures, initial stream8 failures, shared
AppBind minor1 rejection failure, then separate writer and reader fixed-minor
adapter failures. After correction, type4 always uses bootstrap minor0; data types
match selected0/1. No arbitrary set_minor or authenticated transition is exposed.
The bootstrap-to-selected codec/owner transition still needs E01/E02 integration.

Actual fixtures use owned loopback TCP/WebSocket/mux and bounded Tokio duplex IO,
not real accounts/terminal data. They cover fragment/coalescing, exact32KiB payload,
malformed headers without body delivery, drip total deadline, truncated/clean EOF,
partial-read cancellation, write deadline/cancellation and budget preflight. A
Debug record exposes only kind/minor/length, never payload. These tests do not prove
inner TLS/RPK/MAC, authentication, native keys, whole-channel cancellation, real
relay deployment, PTY or mobile/platform acceptance. Those remain OPEN.

The new shared crate has its own /target/ ignore; build artifacts are excluded.
The remote lock changes only by adding this local path dependency. No registry
dependency, original desktop source, relay bundle, credential or security setting
is changed.
