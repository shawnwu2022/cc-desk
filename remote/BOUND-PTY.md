# Default-off native grant to PTY adapter

The Host component now has a native adapter from the verified scoped grant and
actual data owner to the same existing durable canonical writer. No endpoint,
pairing ceremony, account mapping, listener or desktop remote setting is enabled.
The caller must supply the expected enrolled Host/principal; trusted enrollment
and persistent platform key storage remain release gates, not facts proved here.

The proof is minted only by security's RunningChannel. Each received DataRecord
retains its exact original native owner identity, signed context/principal/scope,
role and live cancel/stop/revoke/expiry state. Host admission verifies Host role,
Controller data role, expected principal, Host/device/target/run/generation/
incarnation and common native clock origin. It derives immutable grant/binding/
policy/time from that proof. An authorized owner refuses all qualification
factories without a verified proof. ManagementOnly cannot supply one.

The same writer retains process custody, single pending operation, durable result
and settled floor. Native query/settle APIs verify live authority and exact
target/device; only the existing durable receipt token/outcome can release pending.
No alternative successful receipt definition exists. HostWritten means bytes
accepted by Host IO, not model/tool/task completion. An already-started write may
finish after revoke; queued work checks authority before touching IO.

Snapshots can reach delivered status only after bounded IO through their original
RunningChannel; arbitrary RecordWriter delivery rejects authorized frames. Exact
received ACK is required, followed by a native input window. Abandoning/overrunning
snapshot delivery closes that PeerScope and requests stop of its exact TLS owner,
including admitted sends whose public waits were canceled. Rebind stops only the
old owner, preserves grant/receipt/clock state and advances fence; the shared grant
and new owner are unaffected. Stop requested is not joined: callers use actual
RunningChannel.wait_closed plus writer/PTY-specific retirement evidence. Already
written/in-flight transport bytes cannot be recalled or reclassified as unseen.

The [input candidate](../protocol/remote/inner/v1/INPUT-CANDIDATE.md) intentionally
does not claim staged input, a complete command router or protocol freeze. Native
window descriptors and exact receipt calls in the fixture are not phone APIs.

Qualification uses real mutual-RPK TLS/AppBind over bounded in-memory transport
and one actual Windows10 ConPTY running only local-writer-fixture.exe, not an AI
CLI or a network relay. The fixture checks target/device/generation/run/incarnation
refusal, pre-delivery ACK rejection, actual snapshot/ACK/input, independent marker
effects, duplicate suppression, exact receipt settlement, live-source mismatch,
rebind, old token/deadline refusal and receive-then-revoke. Separate sink/blocking
native IO cases isolate output backpressure and queued/in-flight revocation; these
are explicitly not PTY tests. Root stop, reader EOF and join are scoped observations,
not closure of universal OpenConsole/old-IO/handle-growth or Windows11 gates.

Run with locked official cached dependencies and the established task-owned test
TEMP root; that conditional fixture location does not repair the original default
path OS5 guard failure. Relevant commands:

```powershell
cargo +1.98.1 test --manifest-path remote/Cargo.toml -p cc-desk-remote-host --all-features --locked --offline --lib Authorized
cargo +1.98.1 test --manifest-path remote/Cargo.toml --workspace --all-features --locked --offline
cargo +1.98.1 test --manifest-path protocol/remote/inner/v1/Cargo.toml --locked --offline
cargo +1.98.1 clippy --manifest-path remote/Cargo.toml --workspace --all-targets --all-features --locked --offline -- -D warnings
```

Real CLI/relay/phones, native persistent keys and enrollment/recovery, Win11,
capacity/operational recovery and signed release acceptance remain OPEN. Earlier
default-root OS5, WebView2 initialization and broad resource-retirement gates are
unchanged. No deployment, account/permission change, CLI version restriction or
push/release belongs to this adapter.
