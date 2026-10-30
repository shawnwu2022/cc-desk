# Remote terminal semantic consumer

Internal E05 trial package, independently testable without an npm installation.
`npm test` uses Node24.18+ built-in runner and TypeScript type stripping. It does
not typecheck; run an installed TypeScript compiler with `--noEmit --strict
--target ES2022 --module NodeNext --moduleResolution NodeNext
--allowImportingTsExtensions --lib ES2023,DOM src/reducer.ts src/renderer.ts`.
The shared wire source and fixture are in `protocol/remote/inner/v1/PROJECTION.md`.

Only the authenticated native record adapter may call `begin` with a verified
Host barrier and pass decoded records. The reducer also checks finite fields,
exact target/binding/stamp/base, cell roles, epochs and ordered chunk coverage.
It never parses VT or authorizes input. `disconnect` clears synchronization and
pending install; old frozen display remains historical. A fresh replacement
requires a new increasing sync epoch. `ack()` is just the exact installed stamp;
Host validation and policy remain mandatory. No target auto-selection or retry.

`render(document, installedFrame, palette?)` creates a detached grid with fixed
numeric positions and textContent, never HTML/URI/clipboard/IPC actions. Import
`src/terminal.css` when mounting the result atomically. Wide cells use Host roles,
not client wcwidth. The optional trusted269-entry palette accepts only #rrggbb.
The trial fallback theme is explicit; exact desktop-theme matching, browser
shaping and accessibility/selection behavior still need real shell acceptance.

`npm run test:native` requires explicit TEST_ONLY artifact path in
`CC_DESK_TEST_ONLY_PROJECTION_JSONL`. Generate it once with the Rust terminal
encoding test of the same tree and that variable; it uses create_new and never
overwrites prior evidence. This verifies actual Rust Core encoder bytes into
the TS reducer, not a phone, network, renderer pixels or an AI CLI session.

The portable Rust native bridge is in `remote/crates/client`, with its UI
adapter in `src/native.ts`. No shell IPC registration, enrollment, secret
storage or remote control entry is wired.
Per-Host/peer client memory accounting, Host-issued barriers/ACKs and E09 control
authority remain required before the package can enable a product input UI.

## Bounded client controller candidate

`src/wire.ts` implements existing inner framing, bounded strict UTF-8 JSON with
expanded duplicate-key and depth checks, exact Snapshot/Window/Disconnect,
InputChunk and ProjectionAck commands, and the shared `control_io.rs`
reply/query/receipt-proof contract. Parsing establishes no peer identity or
input authority. Integer schema fields retain their original numeric lexeme in
private WeakMap metadata attached to bounded parsed objects: `1.0`/`1e0` cannot
become an accepted integer through JavaScript conversion. Additive projection
fields retain ordinary JSON numeric compatibility. Native objects passed directly
to the reducer have no JSON lexeme evidence; wire parity applies to the codec path.

`TerminalClient` consumes an injected `NativeConnection` that must own the
verified mutual-RPK/AppBind channel and validate original received DataRecord
direction, channel, grant/scope and fence. `connect` supplies that dependency;
it does not authenticate a peer. No production adapter or app route is registered.
`nativeConnection(port)` is the concrete portable adapter for the Rust owner's
snapshot/exchange operations. Its port must be supplied by an authenticated
native shell resource; dependency injection does not authenticate that resource.
It validates and bounds native outputs, permits no concurrent queue, uses a
monotonic completion deadline and closes on error/cancellation. `waitClosed`
retains native retirement failure instead of interpreting close initiation as
retirement. Original-document admission and actual platform IPC registration
remain required; this module does not implement a browser cryptographic fallback.
The native snapshot method streams its verified barrier and bounded parts and
resolves only after Host full delivery completion. The controller installs through
the reducer, sends the exact ACK, validates ProjectionAcknowledged, obtains
WindowGranted and echoes its opaque window/deadline unchanged in one bounded
input operation. Browser wall time never redefines the Host monotonic deadline.
`render` uses the existing inert text grid renderer.

Only one native request is in flight; competing calls reject and input has no
queue. Native waits have a finite 1..1000ms budget (default 1000ms). Timeout closes
the original connection and never proves that an admitted write did not happen.
Disconnect clears window/synchronization and fences late callbacks. Native
reconnect may supply a new binding for the same exact target. `ProjectionReceiver`
retains its installed frame as an unacknowledgeable continuity baseline: fresh
binding/barrier, increasing sync epoch, nondecreasing revision/epochs and exact
same-revision state/content consistency remain required. Historical display is
retained, and pending input facts survive reconnect. No automatic retry, input
replay, session relaunch or CLI-version gate exists.

Final HostWritten/Rejected receipts retain their exact proof until ReceiptAck and
an exact ReceiptSettled Advanced/AlreadySettled result. Rejected settlement and
Unknown/Partial/Missing/TooOld remain blocked. Missing/TooOld never erase an exact
final proof already retained for that operation. After a lost settlement response,
a separate explicit ReceiptAck with that same proof can receive AlreadySettled;
the query itself never proves settlement. A contradictory Known outcome/proof
fails closed and preserves the original receipt. InputNotAccepted means only the
Host-known pre-write refusal. HostWritten acknowledges Host IO, never CLI
completion. In-memory pending facts do not implement durable mobile crash recovery
or secret storage; disposing the client cannot be treated as settlement.

Run `npm test` in this directory for the complete package unit suite. Typecheck:

```powershell
tsc --noEmit --strict --noUnusedLocals --noUnusedParameters --target ES2022 --module NodeNext --moduleResolution NodeNext --allowImportingTsExtensions --lib 'ES2023,DOM' src/client.ts src/wire.ts
```

`src/native.control.integration.ts` consumes an explicit public TEST_ONLY JSONL
artifact emitted by the current Rust `control_io` encoder test. It validates all
14 actual reply records and negative schema cases. It reads the existing artifact;
it never regenerates or overwrites evidence. Run after the Rust producer:

```powershell
$env:CC_DESK_TEST_ONLY_CONTROL_JSONL='<explicit existing Rust encoder JSONL path>'
node --experimental-strip-types --test src/native.control.integration.ts
```

Browser/PWA reuse covers the pure codecs, reducer, renderer and controller logic.
Browser WebSocket APIs do not expose the existing Rust TLS1.3 mutual RFC7250 raw
public key handshake, pinned-key verification and AppBind channel ownership. A
verified native shell bridge remains required. This package supplies no remote
webpage fallback, JS self-signing, enrollment, credentials, mobile shell or
persistent client recovery. Real-device IME/fonts/layout, phone-to-Host acceptance,
production entry and signed phone prerelease remain separate delivery gates.
