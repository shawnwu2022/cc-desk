# Browser approval candidate (code only)

This change adds an explicitly enrolled, in-memory browser approval policy to the
existing TLS/RPK/AppBind grant exchange. It does not register credentials, choose
an origin/RP ID, deploy a website, or expose a browser `NativeClientPort`.
Missing enrollment remains unavailable. The application panel still requires its
original authenticated document bridge.

The controlled HTTPS origin is trusted code with permission to use browser keys.
SRI does not confer the native signed-bundle trust boundary. Before requesting
presence, that page must display the verified grant's principal, endpoint/context,
scope, role and expiry. UV proves verification at the authenticator; it does not
prove a trusted display of those grant semantics.

## Keys and enrollment

The browser holds separate non-exportable P-256 WebCrypto private keys for TLS
endpoint authentication and grant-issuer signing. `browserProfileBoundary` checks
the key handles, algorithm, usages and non-exportability; trusted enrollment must
also ensure distinct underlying key material. The authenticator holds a separate
WebAuthn credential. Host policy stores only the approved public keys, credential
ID, exact HTTPS origin, RP ID and explicit backup policy. No CNG export or software
key substitution is provided. Enrollment, revocation distribution, loss/recovery
and replacement require a separate explicit operation; this candidate performs
none. `GrantGate::revoke` immediately revokes its pending and live grants.

## Version 1 presence envelope

The existing bounded grant-management control record carries canonical JSON with
ordered fields `version`, `cose`, `credentialId`, `authenticatorData`,
`clientDataJson`, `signature`. Version is exactly 1; byte fields use canonical
unpadded base64url. The complete record remains at most 16 KiB; decoded COSE is
at most 8192 bytes, credential ID 1024, client data 2048, signature 80 and
authenticator data exactly 37. Unknown/duplicate envelope fields and noncanonical
serialization fail closed. Client data supports only `type`, `challenge`,
`origin`, optional `crossOrigin`, optional `topOrigin`; unknown fields and
duplicate known fields fail closed. This deliberately strict subset may reject
authenticator/browser extensions rather than infer their semantics.

The SHA-256 challenge starts with `CC-DESK-BROWSER-APPROVAL-v1\0`, then
big-endian u32 length-prefixed origin, RP ID, credential ID, credential SPKI,
two backup-policy bytes, management nonce, current TLS/AppBind binding ID, and
the existing full COSE Sig_structure. That structure includes issuer kid,
domain/AAD and every canonical grant field, including Host challenge, endpoint
context, principal, scope, role, grant ID and Host expiry.

The Host checks the original issuer COSE signature and WebAuthn ES256 assertion.
It requires exact credential/origin/RP ID hash, `webauthn.get`, expected challenge,
UP and UV, enrolled BE equality and explicit BS permission. Cross-origin/top-origin
assertions, AT/ED and reserved flags are refused. Signature counters, including
zero, do not replace the pending Host challenge. WebCrypto COSE signatures use
raw 64-byte P1363; WebAuthn assertion signatures use DER. TLS also needs DER and
must not reuse the COSE representation.

Policy selection belongs to the trusted Gate/signer owner, never wire detection.
An enrolled browser Gate cannot fall back to bare native COSE. One pending grant
and the existing per-challenge three-failure/30-second limit remain. A new
management attempt cannot clear a still-live pending challenge or its remaining
failure budget; it receives Busy. An expired or exhausted pending challenge may
be replaced under the existing Gate policy. This is not a cumulative per-device
or cross-challenge rate limiter. Accepted retries
must preserve the exact envelope bytes, nonce and binding and original Host
expiry. Another signature for the same grant is refused. A new nonce/binding
requires a new challenge and presence ceremony; browser client state therefore
does not reuse an old envelope on a newly established management exchange.

## Evidence and remaining work

Rust tests use public TEST_ONLY fixture signatures over in-memory
`tokio::io::DuplexStream` TLS/RPK/AppBind channels, then exchange an actual scoped
control record. They are not network, installed application or phone tests.
Node tests import a public scalar1 fixture into non-exportable WebCrypto handles;
the WebAuthn response is a synthetic API fixture, with no credential registration
or real `navigator.credentials` request. The production producer uses actual
SubtleCrypto and `navigator.credentials.get` APIs, but has no fabricated Ready
adapter and is not connected to the application panel.

The wasm target selects ring's official JS RNG feature and excludes native Tokio
net/multithread features. A workspace-local LLVM compiler now builds ring, and the
upstream `rustls-pki-types/web` feature supplies real browser time through
`web-time` on wasm32-unknown-unknown. The locked security-crate wasm check and
`cargo build --target wasm32-unknown-unknown --lib` pass. The build produces a
Rust `.rlib` archive with library code generation; it is not an executable `.wasm`
or browser bundle, running Tokio/browser clock or TLS session.

## Bounded TLS signing bridge

`ui-terminal/src/browserTlsSigner.ts` supplies the synchronous Worker callback
used by the limited Rust WASM Signer below. `createBrowserTlsSignerBroker`
reuses `browserProfileBoundary` on the enrolled, top-level HTTPS Window and holds
only its non-exportable `tlsKey`. It does not sign with `grantKey`, export a key,
register a credential, request WebAuthn presence or implement another TLS stack.
The controlled page and its DedicatedWorker are trusted key-use code. The owner
must privately transfer `broker.worker.port` once, together with the descriptor's
shared buffer/metadata, to that Worker, then call
`createBrowserTlsWorkerSigner(descriptor)` there. Do not publish the port or pass
the profile/key into the Worker. No public window-message listener is installed.

Both sides require `crossOriginIsolated`, SharedArrayBuffer and Atomics. Only a
DedicatedWorkerGlobalScope may construct the synchronous signer; Window, service
or shared workers and missing capabilities fail closed. Isolation headers and a
controlled worker origin must be supplied by a future deployment. There is no
UI-thread wait or fallback. `sign(message)` either returns DER bytes or throws an
Error that the future rustls adapter must propagate as TLS failure; it establishes
no Ready, channel identity or grant. The WASM adapter below propagates failure as
a rustls signing error. The TLS-only owner below adds the original connect call;
the bound application/data owner remains unimplemented.

The input is exactly 64 spaces, `TLS 1.3, client CertificateVerify`, one zero byte
and a 32-byte transcript hash. This matches the existing SHA256-only cipher-suite
profile. Server CertificateVerify, COSE and arbitrary messages are refused. Input
is copied before validation/use. Actual SubtleCrypto ECDSA/SHA256 returns raw
64-byte P1363; the broker checks scalar bounds and emits minimal positive ASN.1
DER integers (at most 72 bytes). This does not change the separate COSE raw and
WebAuthn DER representations.

Each bridge has a fresh private MessageChannel, fixed 92-byte SharedArrayBuffer,
random generation label and one-time Worker attachment. Its positive int32
sequence increases without wrapping or resetting; exhaustion closes the bridge.
The port, buffer and generation are scoped together and never reused. Only one
request/original SubtleCrypto Promise is retained, with no work queue or retry.
Requests have exactly four fields and one fixed-size message; responses use the
one fixed shared slot. Unexpected, duplicate, out-of-order, concurrent or malformed
requests close the entire bridge. These bounds describe the trusted producer and
broker; browser MessagePort itself cannot impose a transport queue quota against
compromised same-origin code holding the private capability.

A request gets one absolute monotonic deadline from
`performance.timeOrigin + performance.now()` before it is sent. The API requires
an explicit 1..10000ms budget derived from the original TLS setup's remaining
budget, without extending its original deadline. This bridge enforces its own
per-sign deadline; the future Rust adapter must still enforce the original setup
deadline across all handshake work. Both bridge sides compare the same per-sign
deadline; message progress never refreshes it. A queued request first observed after expiry starts zero crypto
operations. Timeout, AbortSignal cancellation, explicit close or protocol failure
sets a sticky shared close flag and wakes the Worker; no later sign or result reuse
is allowed. Close also stores a terminal value into the actual waited-on state
before notification, so a Worker entering native wait after the notification
observes a changed value instead of sleeping through close. Normal request,
completion and consumption transitions use compare/exchange and cannot overwrite
that terminal state. A result is checked again after copying before publication. The final
close/deadline sample is its completion linearization point; cancellation cannot
retract a signature already returned before that point.

SubtleCrypto operations already started cannot be canceled. The broker keeps
their original Promise and key reference until settlement, discards late results,
and only then resolves `waitClosed()` after removing handlers, clearing output and
releasing its key reference. A never-settling API Promise therefore leaves
`waitClosed()` pending. This proves retirement of this bridge's own operation and
references only: it does not prove CryptoKey destruction, Worker-thread exit or
whole browser/TLS-owner retirement. The caller must close the broker when its
Worker/owner is canceled or terminated; no browser thread join is claimed.

Node tests use actual `worker_threads`, MessagePort, SharedArrayBuffer/Atomics and
real SubtleCrypto with public TEST_ONLY scalar1 imported as non-exportable handles;
Node crypto independently verifies the returned DER. Browser platform globals are
explicit fixture shims, not evidence from an actual isolated browser. Held-Promise
fixtures exercise cancellation, timeout, concurrent refusal and late completion;
each fixture awaits termination of its own Node Worker. Existing browser approval
tests still use a synthetic WebAuthn response and never open a real prompt.
A deterministic scheduling fixture pauses immediately before the production
native Atomics.wait call, closes and retires the broker including its original
crypto Promise, then releases the gate. The unchanged V1 implementation returned
`timed-out`; the repaired implementation returns `not-equal` without waiting for
the original budget. The fixture still invokes the real native wait.

Browser WebSocket/mux IO, AppBind/grant/peer-install browser ownership and the
authenticated operation adapter remain unimplemented. Successful native fixtures,
this Node bridge test or a wasm compile must not be presented as an app panel or
phone end-to-end connection. The following TLS-only owner checkpoint adds its
own explicitly bounded runtime/IO evidence.

## Rust WASM Signer ABI candidate

`BrowserWasmSigningKey` (experimental, wasm32-unknown-unknown only) implements the
original rustls SigningKey/Signer traits. Construction requires a canonical P256
SPKI from explicit trusted enrollment metadata and the original setup absolute
deadline in the `performance.timeOrigin + performance.now()` clock domain.
`BrowserProfile` still has no SPKI field; the caller supplies the separately
enrolled public metadata. No key export, default identity, registration or new
authorization stack is provided. The original `LocalIdentity::new` retains its
exact algorithm/public-key/scheme checks. Rust also verifies each returned DER
signature against the declared SPKI using ring before returning success.

The versioned private import namespace is `cc_desk_browser_tls_v1`:

| Import | Contract |
| --- | --- |
| `now() -> f64` | Same monotonic absolute clock; nonfinite/expired samples fail. |
| `init(spkiPtr, 91, setupDeadline) -> i32` | One binding; exact enrolled public bytes and original deadline; 1 succeeds, -1 fails. |
| `sign(seq, setupDeadline, messagePtr, 130, outputPtr, 72) -> i32` | One in flight, increasing positive int32 sequence; return 8..72 DER bytes or -1. |
| `close()` | Sticky close of that bridge, never a CryptoKey-destruction or Worker-join receipt. |

`browserTlsWasm.ts` creates the imports only inside a trusted DedicatedWorker and
requires `broker.worker.setupDeadline`. The owner must pass that original deadline
to `createBrowserTlsSignerBroker({ ...profile }, {timeoutMs, setupDeadline})` and
`BrowserWasmSigningKey::new(public, setupDeadline)`. Repeated calls cannot extend
it: Rust checks before/after the import, the adapter matches it exactly, and both
Worker wait and Window broker enforce it while blocked. The older standalone
bridge may omit this field for its per-sign fixture; the WASM edge cannot.
NaN, infinity, expiry and attempted extension fail closed.

Rust uses ordinary Arc/atomic state, no unsafe Send/Sync implementations and no
lock held across JS. Temporary Signer handles created by LocalIdentity do not close
the owner; explicit close or final Arc-state destruction does. Reentry, protocol
failure, bad lengths, wrong signature, close or expiry seal the owner. The original
Window broker still retains a started SubtleCrypto Promise until settlement.

The adapter copies a fixed 130-byte input, requires a disjoint 72-byte output
range, checks integer offsets before constructing views and takes a fresh view
after the callback because memory.grow can replace the buffer. Shared memory and
changing the bound WebAssembly.Memory object are refused. One adapter owner is
claimed per Worker module lifetime, including after close; a duplicate factory
fails before changing the original owner. This also protects official generated
glue's module-level wasm binding. The controlled Worker loader must keep one
canonical adapter/glue module instance, raw Instance, memory, helper exports and
allocators private. Untrusted UI must never receive them. This source slice has
no general runtime loader. The TLS-only production entry below keeps the calling
boundary private; the independent Signer fixture also demonstrates fixed
init/sign/grow/close test commands and bounded signature bytes.

The fixture at `crates/security/tests/fixtures/browser_tls_wasm_fixture.rs` is a
thin wrapper around the production implementation and original LocalIdentity. It
exports fixed TEST_ONLY cases and bounded signature bytes, not arbitrary pointers
or memory read/write methods. Tests instantiate real compiled WASM in actual Node
Workers with the existing MessagePort/SAB/SubtleCrypto broker and independently
verify DER using Node crypto. Public scalar1 is imported into non-exportable
handles; its opposite public point is used to reject key/metadata mismatch. No
real enrollment or credentials are created. Browser platform globals remain
explicit Node shims, not actual browser acceptance.

The initial Cargo example attempt failed on native dev-dependency getrandom0.3.4;
the fixture therefore uses the current production rlib directly, without changing
RNG/dependencies. Direct linking also required Cargo's host proc-macro dependency
directory. Raw output still contained wasm-bindgen descriptors and could not
instantiate with private imports alone. Matching official wasm-bindgen0.2.129
postprocessing and its unmodified generated JS provide the real helper runtime;
no fake/empty upstream imports are used. The recorded target RED is distinct:
after official processing, LocalIdentity construction succeeded and the actual
Rust Signer returned its then-unimplemented error. GREEN executes the real import
and verifies the signature, including after memory growth.

From the repository root, with an existing wasm target, clang and the matching
official generator, the independent test can be reproduced in PowerShell:

```powershell
$env:CARGO_BUILD_JOBS='2'
$env:CC='<absolute path to existing clang.exe>'
$env:CARGO_TARGET_DIR=Join-Path $PWD 'target-browser-profile'
$artifactDir=Join-Path $env:TEMP 'cc-desk-test-only-wasm'
New-Item -ItemType Directory -Force $artifactDir | Out-Null
cargo build --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-security --all-features --target wasm32-unknown-unknown --lib --message-format=json > "$artifactDir/artifacts.jsonl"
if ($LASTEXITCODE -ne 0) { throw 'Rust library build failed' }
$rows=Get-Content "$artifactDir/artifacts.jsonl" | ForEach-Object { $_ | ConvertFrom-Json }
$securityRlib=($rows | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'cc_desk_remote_security' }).filenames | Where-Object { $_.EndsWith('.rlib') }
$rustlsRlib=($rows | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'rustls' }).filenames | Where-Object { $_.EndsWith('.rlib') }
rustc --edition=2024 --crate-name=browser_tls_wasm_fixture --crate-type=bin --target=wasm32-unknown-unknown -L "dependency=$(Split-Path $rustlsRlib)" -L "dependency=$env:CARGO_TARGET_DIR/debug/deps" --extern "cc_desk_remote_security=$securityRlib" --extern "rustls=$rustlsRlib" remote/crates/security/tests/fixtures/browser_tls_wasm_fixture.rs -o "$artifactDir/fixture-raw.wasm" -C link-arg=--no-entry -C link-arg=--export-memory -C link-arg=--export=fixture_init -C link-arg=--export=fixture_sign -C link-arg=--export=fixture_signature_byte -C link-arg=--export=fixture_close -C link-arg=--export=fixture_grow
if ($LASTEXITCODE -ne 0) { throw 'Fixture link failed' }
& '<absolute path to official wasm-bindgen 0.2.129 executable>' --target bundler --keep-lld-exports --no-typescript --out-name fixture --out-dir "$artifactDir/generated" "$artifactDir/fixture-raw.wasm"
if ($LASTEXITCODE -ne 0) { throw 'Official postprocess failed' }
$env:CC_DESK_TEST_ONLY_TLS_WASM="$artifactDir/generated/fixture_bg.wasm"
$env:CC_DESK_TEST_ONLY_TLS_WASM_GLUE="$artifactDir/generated/fixture_bg.js"
Push-Location remote/ui-terminal
try { npm run test:wasm } finally { Pop-Location }
```

Generated JS/WASM are build evidence, not checked-in product code. `test:wasm`
requires explicit artifact paths and fails rather than silently skipping. Its
negative cases cover purpose/bounds, wrong sequence, reentry, invalid return
length/signature, wrong identity/key, shared/rebound memory, invalid/extended
deadline and actual close/expiry with retained late crypto. AppBind, grants, peer
installation, browser WebSocket/mux IO and Panel integration still need their own
callers and acceptance; this is a real Signer ABI slice, not a complete
phone-to-Host application session.

## TLS-only controlled Worker owner

`ui-terminal/src/browserTlsWorker.ts::startBrowserTlsWorker` is the production
Worker entry. It privately instantiates the real module with unmodified official
wasm-bindgen helpers, initializes that helper context once, and calls production
`browser_tls_start`. The Rust owner in `browser_wasm_owner.rs` constructs the
original LocalIdentity/PeerPin with explicit enrolled public metadata and calls
the original `tls.rs::connect`. It retains the one future/Pending; the fixture
does not implement or replace that owner. No TLS config, certificate verifier,
negotiated evidence, Signer, RPK policy or role check is duplicated or relaxed.

The four fixed owner exports are start(localRole, peerRole, remainingBudgetMs),
poll(), cancel() and event(kind). The private `cc_desk_browser_owner_v1` imports
provide fixed public-key slots, original deadline/real clock, scheduling, IO,
timer and bounded evidence reporting. They are local implementation ABI, not a
new network protocol. Raw instance, memory, allocators and helpers remain inside
the controlled Worker. No plaintext, granted operation or native application IPC
is exported. Role 0 means Client and role 1 means Host; other values reject.

`tls_clock.rs` isolates TLS and AppBind setup timing. Native Deadline remains exactly a
Tokio Instant, with the original checked budget, timer and final eligibility
samples. On wasm32-unknown-unknown, it is a real monotonic absolute sample in the
same performance clock domain as the existing signing bridge. One original
deadline is captured before startup and cannot increase. A JS setTimeout wakes a
registered real Rust Waker; timer callbacks recheck the absolute time and never
refresh it for progress. Real IO/watch/timer wakers coalesce onto Worker
microtasks without synchronously reentering poll. No Tokio WASM runtime,
NoopWaker, periodic spin polling, fake time or replacement handshake is used.
The actual RED entered original connect and trapped in std::time::Instant::now
via Tokio; the clock seam fixes that actual target failure.

`BrowserIo` implements the original AsyncRead/AsyncWrite bounds: one 16KiB RX
slot, one 16KiB TX credit, at most one underlying read and one write Promise.
RX empty is Pending; only explicit null from the transport is EOF. Reads can
complete while a write is blocked. A write admits only the independently copied
bytes held by that credit; poll_flush waits for the original transport.write
Promise to complete. This must represent actual local IO completion, never only
posting a message or enqueueing another unbounded work item. Partial writes or
failed/cancelled completions do not authorize replay. The default test transport
uses actual Node socket callbacks over loopback TCP.

Shutdown is conservative full-duplex closure: drain the outstanding write, call
the original transport.close, then wait for its actual waitClosed result and
started read/write Promises before poll_shutdown can return success. Subsequent
writes reject. Errors fail closed; cancellation/expiry can drop the Rust future
but cannot pretend the outstanding JS operations were canceled or settled.
The shutdown RED demonstrated premature success with the old flush-only method;
the deterministic test holds the real close completion and proves no Rust
shutdown completion until release.

An AbortSignal is accepted before compilation begins. Pre-abort, the original
SAB close flag and the original deadline are checked initially, after the
original compile Promise and after the original instantiate Promise settle.
Canceled or expired loading cannot start TLS, write ClientHello or publish
evidence. The original loading Promises are awaited rather than abandoned in a
timeout race. The metadata/key-capability descriptor and signal are captured
before awaits. On final publication, Rust and JS recheck cancel/deadline; JS also
refuses already closed/error/EOF IO. TEST_ONLY scheduling probes cancel or expire
at the real evidence callback and verify that the consumer sees no success.

The evidence Promise returns only the original TLS protocol/cipher/group/full
handshake/RPK/ALPN/peer endpoint ID. It is historical transport proof, never
AppBind, a grant, session authorization, online Ready or application authority.
Without explicit ClientTrust the owner continues to watch the original deadline,
cancellation and raw EOF while retaining Pending (the TLS-only mode).
The optional AppBind mode below consumes Pending through original establishment.
All eight existing public native Bind factories still reject WASM with Unsupported
before establishment or tokio::spawn; Data/grant/peer capabilities remain closed.

Owner waitClosed waits for Rust future termination plus this entry's original
transport read/write/retirement Promises. A never-settling Promise keeps it
pending; runtime traps or failed transport retirement reject instead of claiming
successful retirement. It is not a Worker-thread join or key destruction proof.
The main-thread signing broker separately owns a started crypto Promise. The
caller must close/cancel that broker too, await broker.waitClosed, await the
Worker owner's waitClosed and finally terminate/join its owned Worker. This
checkpoint's tests do all of those for their own resources only.

The independent `npm run test:browser-tls` requires explicit paths in
CC_DESK_TEST_ONLY_BROWSER_TLS_WASM, CC_DESK_TEST_ONLY_BROWSER_TLS_GLUE and
CC_DESK_TEST_ONLY_BROWSER_TLS_HOST; it never silently skips. The host is a new
ordinary owned fixture process with a single ephemeral 127.0.0.1 listener and
bounded lifetime/output, using the original Rust accept and published TEST_ONLY
keys. Wrong ALPN/cipher modes use explicitly malformed fixture-only rustls server
configs; production connect remains unchanged. The Node Worker fixture supplies
real TCP IO and browser-global shims, not a replacement Rust owner. A separate
in-memory silent sink tests timing/cancellation only and supplies no TLS success
evidence. These are not real browser WebSocket/mux, outer WSS, app or phone tests.

Build the WASM as in the preceding reproducible Signer commands, substituting
tests/fixtures/browser_tls_owner_fixture.rs and explicit exports
browser_tls_start/browser_tls_poll/browser_tls_cancel/browser_tls_event plus
the thin wrapper's one-line forwards. The production library retains all owner
logic without forcing C exports/runtime imports into unrelated Signer bundles.
Keep the same wasm and host dependency search directories, then
run the matching official generator with --target bundler --keep-lld-exports
--no-typescript --out-name fixture. To build the owned native host from the same
tree, run the corresponding native cargo build --lib --all-features
--message-format=json (no target flag), select its exact rlib artifact paths, and
compile tests/fixtures/browser_tls_host_fixture.rs with rustc --edition=2024,
-L dependency=<native target debug/deps>, and --extern entries for
cc_desk_remote_security, rustls, tokio_rustls, tokio, serde_json, base64 and ring.
Only that explicit output executable is passed to the test; no existing process,
user CLI, credentials, installation or deployment is selected or modified.

The TLS group covers both endpoint evidence and exact Host endpoint hash,
wrong pin/roles/ALPN/cipher, EOF, absolute deadline, pre-start/loading cancellation,
final ciphertext flush success/failure, simultaneous RX with blocked TX, retained
in-flight IO/crypto, same-turn publication fences and shutdown retirement. The
final-write gate observes real post-Signer poll_flush Pending rather than guessing
a write ordinal: native accept must already have authenticated the client while
its final write Promise is held, and client evidence remains unpublished until
the original Promise resolves. Rejecting that Promise cannot succeed.

Reference: [WebAuthn Level 3 assertion verification](https://www.w3.org/TR/webauthn-3/#sctn-verifying-assertion).

## Browser ManagementOnly AppBind checkpoint

`startBrowserTlsWorker` accepts an optional explicit `clientTrust` object with
`hostKeyEpoch` and `clientKeyEpoch` as canonical unsigned decimal u64 strings and
`origin` as the already approved canonical relay WSS origin. For example, values
above JavaScript's safe integer range remain strings throughout the boundary.
No default epoch, origin, identity or authority is supplied. Rust validates the
origin with the original `CanonicalOrigin::trusted` and requires exact canonical
spelling. The Worker's HTTPS origin does not stand in for the relay WSS origin.
Enrollment/configuration delivery of this trusted metadata remains a caller duty.

With this configuration the production Rust single-future owner calls the original
`establish_client` through a crate-private facade fixed to minor 0,
ManagementOnly, and grant-management 0. Original canonical encoding, real random
nonces, transcript, TLS exporter, directional HMAC, pinned identities, epochs and
role checks run unchanged. Establishment uses the same captured absolute setup
deadline through the clock seam. Progress does not refresh it. The last
ClientConfirm write and flush must complete, followed by cancellation/deadline
checks, before the separate `management` Promise can resolve. The TLS `evidence`
Promise remains transport evidence alone. Startup failure rejects both Promises;
loading cancellation retains the original compile/instantiate Promises to settle.

Management facts expose only kind/profile/minor, verified endpoint IDs, exact
epoch strings and relay origin. Raw context/binding/exporter, nonces, MACs,
plaintext, memory and instance stay private. The owner retains original
Established IO/context/binding until termination. Successful establishment ends
the setup timer; it does not make setup D a connection lifetime or grant expiry.
Afterward the same management-read behavior as Native waits for cancellation,
local shutdown or peer input: EOF closes, and any unsupported post-bind plaintext
byte rejects. It never dispatches Data or claims RemoteReady. No grant issuance,
grant expiry/revocation, peer install, application operations or Panel is connected.
Cancellation here is the existing owner/broker authority cancellation only.

Closing the main broker first sets the irreversible SAB terminal state and then
posts the fixed private MessagePort close notification. The active Worker listens
and schedules original cancellation even when the bound peer is silent and no
setup timer remains. SAB checks remain the second fence. Close/abort/shutdown
during establishment prevents late management publication. Started IO and crypto
Promises remain with their respective owners until actual settlement; neither
waitClosed is a Worker join or CryptoKey destruction proof. Callers still own
Worker termination and its awaited exit, including suspended-browser recovery.

The independent `npm run test:browser-appbind` requires
CC_DESK_TEST_ONLY_APPBIND_WASM, CC_DESK_TEST_ONLY_APPBIND_GLUE and
CC_DESK_TEST_ONLY_APPBIND_HOST; missing artifacts fail explicitly. Build the first
two exactly as the preceding TLS-owner lib -> thin rustc wrapper -> matching
official wasm-bindgen commands. Build the Host test harness from current source:

```powershell
cargo test --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-security --all-features --lib --no-run --message-format=json
```

Select the executable of the `cc_desk_remote_security` test artifact from that
Cargo JSON and set CC_DESK_TEST_ONLY_APPBIND_HOST to that exact path. The Node
test spawns only this owned executable with
`--exact bind::browser_fixture::BrowserAppBind_LoopbackFixture_001 --nocapture`
and an explicit CC_DESK_TEST_ONLY_APPBIND_MODE. With no such environment variable,
ordinary cargo test executes a genuine native in-memory original connect/accept
and public bind_client/bind_host roundtrip instead of waiting for an external
client or skipping. All mutation hooks are cfg(test) only. The normal loopback
Host runs the original public bind_host with default Hooks.

The actual WASM/Worker matrix checks original Host establishment, exact large
epochs and endpoint facts, wrong identity/role/epochs/origin/MAC/direction,
cross-TLS HostConfirm MAC replay, silence and encrypted byte progress under the
original D, final ClientConfirm flush success/failure/cancel/late completion,
same-turn publication fences, startup/load failures and custody, unsupported
post-bind data, and main-broker-only cancellation of a silent bound owner after
setup D. Cross-TLS replay also changes fresh transcript nonces; isolated exporter
binding remains covered by original native binding tests. A separately recorded
TEST_ONLY temporary qualification probe invokes all eight public native factories
with actual WASM Pending, observes Unsupported without runtime traps, and restores
the exact production source before final codegen. No probe switch/API is shipped.
The test IO is explicitly Node-owned ephemeral loopback TCP with browser-global
shims; this is not real browser WebSocket/mux, outer WSS or phone end-to-end proof.

## Browser grant-management checkpoint

The optional `grantBridge` is a separate private capability created by
`createBrowserGrantBroker`. Its configuration must explicitly supply the approved
issuer SPKI, principal, SessionScope, Observer/Controller role, positive operation
budget <=30000ms, and enrolled WebAuthn origin/RP ID/credential/public SPKI/backup
policy. Public metadata comes from approved enrollment; private keys are never
exported to derive it. The Window profile retains distinct non-exportable TLS and
grant-signing keys. A trusted `present(verifiedGrant, signal)` callback is required
to display and approve the full verified grant semantics before signing/presence.
There is no default presenter, automatic approval, key provisioning or app UI.
Missing/malformed grant configuration rejects startup and never selects plain
ManagementOnly or a signer whose browser_profile is None. Existing plain0 mode
and all eight public Native factories' WASM refusal remain unchanged.

The browser's crate-private bind facade is fixed to minor1 and grant-management1.
The same production Rust future owns the original ManagedChannel and invokes its
original async `grant_client`; no spawn, copied exchange or new network protocol
is introduced. The original Request/Challenge verification yields
VerifiedCanonicalGrant. Its original signing_parts and browser_challenge provide
only the validated candidate/TBS, exact payload and profile/nonce/TLS-binding
challenge for one private request. Window uses original produceBrowserApproval
and grantKey (never the TLS signing bridge), then awaits completeCose. Rust fills
raw64 into the original coset candidate, serializes with coset and pre-verifies
the declared issuer via original grant::signature before WebAuthn is requested.
The original Rust BrowserProfile.verify and Host GrantGate.require_browser /
grant_host still verify the complete COSE/WebAuthn envelope. Raw64 COSE,
WebAuthn DER and TLS DER representations stay separate.

completeCose accepts a Promise while retaining synchronous callback compatibility.
After each await, including the COSE return trip, the original operation D,
AbortSignal, secure context, top-level frame and exact enrolled origin are checked
before entering credentials.get or returning a proof. One private MessagePort
generation/sequence and fixed signature -> COSE -> approval phases permit one
request, no retries or queue. Request JSON <=32768 contains bounded original TBS
<=10000, canonical presentation <=8192 and challenge32; raw signature is64,
COSE <=8192, original approval <=16384. Private WASM views are checked and rebuilt
from nonshared current memory. Wrong phase/sequence/bounds/close is terminal.

The grant operation gets its own absolute D once at original exchange_io, for a
positive budget <=30s. Native retains original Tokio Instant behavior; WASM uses
the existing real monotonic timer/waker seam with an explicit phase-bound capture.
It is neither the TLS setup deadline nor a refreshed per-message budget.
ClientDeadlineClock's original sample stays before Request; approval/Accepted
never re-anchor it. All actual write/flush completion, Accepted nonce/binding/
grantId/digest, final checks and install_remote must succeed before the separate
grantAccepted Promise publishes a small historical GrantAccepted receipt. A
candidate alone never succeeds. Completed opD is not a connection TTL. The
original channel/RemoteGrant/state remain private until owner close; later Data
and peer bytes remain unsupported. No data owner or RemoteReady is exposed.

The browser monotonic source in this checkpoint has no qualified mobile suspend
continuity claim. Callers must invalidate lifecycle discontinuity by closing the
existing broker/owner; no live authority/data path is enabled from this source.
Receipts are historical only. Host Gate expiry/revocation remains original;
continuous revocation delivery, live grant expiry ownership, enrollment/recovery,
actual browser/phone clock qualification and application integration remain open.

Close/deadline seals the private phase and original IO/token. Window retains its
original presentation, SubtleCrypto, COSE callback and credentials.get operation
to real settlement even if Abort cannot preempt it. Worker waitClosed covers its
IO custody, and grantBroker.waitClosed covers its own started Window work; neither
proves Worker join or CryptoKey destruction. The caller closes both brokers,
awaits their custody and the Worker owner, then terminates/awaits its owned Worker.

`npm run test:browser-grant` requires explicit CC_DESK_TEST_ONLY_GRANT_WASM,
CC_DESK_TEST_ONLY_GRANT_GLUE and CC_DESK_TEST_ONLY_GRANT_HOST, without silent skip.
Build WASM/official glue exactly as the preceding owner commands. The Host is the
current `cargo test --offline --locked --manifest-path remote/Cargo.toml -p
cc-desk-remote-security --all-features --lib --no-run --message-format=json`
test executable, selected from Cargo JSON. The test spawns only that owned Child
with `--exact bind::browser_fixture::BrowserGrant_LoopbackFixture_002 --nocapture`
and explicit TEST_ONLY mode. Without the environment variable, ordinary cargo
test performs a genuine original Gate/COSE/WebAuthn acceptance selftest. Normal
network fixture code calls original accept, public bind_host_grant_channel,
GrantGate.require_browser and grant_host. Expiry/revoke and unsupported Data are
test-only Native policy/peer behavior; production has no bypass.

The group uses separate published TEST_ONLY scalars2/1/3 for TLS/issuer/credential,
real SubtleCrypto and synthetic credentials.get with genuine credential signatures.
No real WebAuthn prompt or account is used. Actual WASM loopback cases cover
success, wrong grant key/endpoint issuer/scope/role, credential profile/signature,
cross-TLS assertion replay, Host expiry/revoke, independent D/non-TTL, startup/load
failure, retained presentation/crypto/get/IO operations, protocol-phase rejection,
final proof flush success/failure/cancel and postaccept Data rejection. Separate
direct producer tests reproduce and fix deadline/frame loss across COSE await.
Original Native tests retain exhaustive Accepted-field, isolated exporter and
grant-policy coverage; this matrix does not replace those tests or claim every
Native mutation ran in WASM. All browser globals are Node shims and IO is owned
loopback test TCP, not deployed WSS/WebSocket/mux or phone E2E.

## Browser peer bootstrap checkpoint

An explicit `peerInstall: {budgetMs}` in the approved grant configuration opts in
to original minor1/grant-management2 negotiation. Budget is positive and <=10000ms.
Plain0 and grant1 keep their original modes; grant1 exposes no pending
peerInstalled interface. The eight public Native bind factories still refuse
WASM. No NativeClient, data_owner task, RunningChannel join or data dispatcher is
migrated or enabled by this checkpoint.

Peer mode requires the broker's actual visible Document and Window lifecycle
event sources. Missing Document/events, initial hidden state or invalid config
rejects before work starts. The broker captures those sources and closes on
visibilitychange, pagehide and freeze. It first writes a sticky peer-only SAB
closed flag, then cancels and posts the original private close notification.
This flag cancels; it cannot enable peer/Data authority. Worker IO and final peer
publication sample the sticky flag, so a lifecycle close already issued in the
Window cannot be hidden by later MessagePort delivery. The port still supplies
the asynchronous wake. Original presentation/crypto/COSE/get Promises retain V17
custody to actual settlement; closure is not CryptoKey destruction or Worker join.

The same production Rust future retains the original ManagedChannel IO, active
identity, CancelToken, RemoteGrant and request-time clock anchor. It invokes the
original peer_install::bootstrap and takes its already completed oneshot result
without another concurrent timeout. Only that original protocol can mint
ClientInstalledPeer. Its common one-use validation is shared with Native
consume_for and retains channel identity, local role, context, binding, grant,
scope, clock-domain same-origin and installed fence/state checks. JS receives no
proof constructor or live capability, only PeerInstalledHistorical with exact
fence string, grant ID and scope after successful consumption.

Shared's private installation deadline distinguishes Native Instant from Browser
Deadline. Native constructors/public signatures/timeout/checked-join behavior
remain unchanged. Browser captures one install D <= explicit budget,10s and the
original grant's remaining lifetime. Before original access.check it captures a
conservative floor sample in the same source domain: grant expiry never exceeds
sampleBefore+remaining. A later import sample cannot renew it; no tolerance is
added. One outer sleep shares the bounded eight-slot owner clock registry and its single earliest-deadline Worker timer. After install,
install D is not a connection TTL: the same owner waits under the fixed original
grant expiry/revocation/cancel/IO boundaries and rejects further Data or replayed
control bytes. Graceful shutdown still waits for actual TLS close-notify write,
flush and underlying transport close completion; unfinished install cancels with
terminal failure. No parallel timer framework or no-op runtime is added.

The standalone `npm run test:browser-peer` requires explicit
CC_DESK_TEST_ONLY_PEER_WASM/GLUE/HOST. Build the owner WASM and official glue with
the preceding lib -> thin rustc wrapper -> official wasm-bindgen commands. Select
the current security unit-test executable from Cargo JSON exactly as for the
grant group; its owned process uses the original peer factory, grant_host,
start_peer_pending, typed host_install_request and complete_host_install. Before
completion it commits an actual record to a self-owned TEST_ONLY in-memory fake
workload registry and derives its revision fence. This is fake installation
coordination, not a real Host actor/process/PTY receipt. Installed/ACK success
packets are produced by original bootstrap, never handwritten by JS or a fixture.
Failure-only cfg(test) mutations alter one field of an originally encoded
Installed frame or replay that original complete frame.

Actual WASM/Worker tests distinguish missing peer config startup RED, grant
completion, pending install, Installed field/order rejection, postinstall replay
and Data refusal. The timing RED holds the real deadline import for40ms and
exposes later-now+remaining renewal; the conservative before-sample fix passes
that same boundary. Tests also cover lifecycle cancellation before publication,
initial hidden/missing sources, original request flush holds/failure/cancel,
postinstall expiry, shutdown write/close custody, and Window presentation custody.
Document/browser globals are explicit Node EventTarget shims; loopback IO and fake
workload are test-only. Browser/mobile suspend continuity remains unqualified,
and this historical peer-only entry grants no Data, input or RemoteReady authority. The explicit application entry below is separate. Real lifecycle/clock,
real actor installation, browser network/mux and application/phone acceptance
remain separate work.


## Explicit original application owner (default off)

The opt-in Worker application entry requires all existing enrolled public metadata,
ClientTrust, grant presentation capability and visible-document peer lifecycle
capability. It calls the original connect/AppBind/grant/peer bootstrap. The Rust
application factory receives the original RunningChannel; original
NativeClient::from_installed consumes the opaque ClientInstalledPeer once. No JS
DTO can construct a proof or choose an installed fence. This mode returns an
application descriptor/NativeClientPort; its separate historical peerInstalled
Promise is undefined. The original peer-only mode remains compatible.

Security prepares the original Data Engine once. Native still spawns that same
future and retains its JoinHandle. WASM polls both that owned driver and the
original client application future under the existing root. The driver drops its
inner future/IO before publishing local completion; dropping an incomplete
driver records Uncertain. A watch notification is not an OS join. Operation
Uncertain is retained as an operation error; checked retirement Unknown has a
separate typed result and terminal classification. Window waitClosed rejects the
latter, and waits for both original pending RPC settlement and root retirement.
It does not join the browser Worker or destroy CryptoKeys; the caller still owns
Worker termination and every main broker's waitClosed.

Native clocks/timeouts remain Tokio aliases. Browser sleeps register at most
eight generation-tagged slots and arm only the earliest deadline. Dropping one
sleep cannot unregister another; exhaustion/generation overflow fails closed.
Application requests carry one copied bounded payload and original absolute D
(budget at most1s), clamped to the original grant expiry. Deadline propagation
only tightens D, including Window-to-Worker queuing and each snapshot part/done.
Grant Request anchoring, setup and installation deadlines are not refreshed.

The controlled Worker samples real performance and wall clocks from fixed
anchors. Elapsed time is their conservative cumulative maximum, with rollback,
invalid samples or an unproved gap over1s causing sticky closure. A250ms sampling
heartbeat shares the one earliest-deadline timer; it is not a lease or no-op
runtime. Window visibility/pagehide/freeze closure and the original cancellation
SAB seal admission. Invalidations clear the original TerminalClient window and
synchronization immediately on observation, while retaining pending receipt or
Unknown witnesses. Old-connection callbacks cannot invalidate a newer epoch.
This is fail-closed code with explicit Node platform shims; real mobile suspend
and clock qualification remains a target-device gate. Production UI stays
Locked/off and no browser network/mux/enrollment entry is enabled here.

browserClientPort/serveBrowserClient carry a private single-in-flight RPC to the
Rust NativeClient; they do not implement a second application protocol. The
original nativeConnection and TerminalClient install the complete snapshot before
exact ACK, obtain the original Host window, and preserve original receipts,
cursor/query/settlement and disconnect rules. On conflict or wrong sequence the
original Host can return InputReceipt Unknown/null; this is not a no-effect
promise and cannot unlock replay. A duplicate exact operation returns its
original receipt without another writer effect.

The actual test entry is npm run test:browser-data. Set explicit
CC_DESK_TEST_ONLY_DATA_WASM, CC_DESK_TEST_ONLY_DATA_GLUE and
CC_DESK_TEST_ONLY_DATA_HOST; missing artifacts fail rather than skip. Build the
client library with cargo build --offline --locked --manifest-path remote/Cargo.toml
-p cc-desk-remote-client --target wasm32-unknown-unknown --lib --message-format=json.
Use that JSON's exact client/security/rustls rlibs, both target and host dependency
directories, and the preceding rustc/no-entry/official wasm-bindgen procedure with
remote/crates/client/tests/fixtures/browser_client_fixture.rs. Export the four
browser_tls_* forwards and browser_client_start/browser_client_event. The
TEST_ONLY_uncertain_start extra forward is a fixture classification probe and
is never selected by the production Worker entry. Official generated JS is not
patched; instance/memory/helpers remain private to the trusted Worker.

Build the owned Host harness with cargo test --offline --locked --manifest-path
remote/Cargo.toml -p cc-desk-remote-host --all-features --lib --no-run
--message-format=json. Select its actual executable and run only
browser_data_fixture_tests::BrowserData_OriginalHostFixture_001 with explicit
CC_DESK_TEST_ONLY_DATA_MODE. It uses an ephemeral loopback endpoint, public
TEST_ONLY identities and synthetic WebAuthn, a new temporary Journal and original
NativeRunProducer with an actual in-memory writer. Original NativeHost coordinator
completes installation; original dispatch_received handles every application
request. Reconnection retains that same Host/producer/Journal and obtains fresh
TLS/grant/proof/fence. No PTY/CLI or production database is run or changed.

| Stage1 operation | Current boundary |
| --- | --- |
| Snapshot -> delivery -> exact ACK -> Window | Original full path executable in actual WASM/Worker and memory Host |
| Input / receipt / Cursor / Query / Settle | Original HostWritten effect and exact facts; Unknown retained; no automatic replay |
| Disconnect / reconnect | Original owners retire; same producer/Journal reconciles, no duplicate write |
| Unknown/malformed schema | Original codec rejects; no default rights |
| Create / Attach / Resize / Resume / Close lifecycle backend | Native backend remains OPEN/default deny per LIFECYCLE-ROUTING.md; not implemented or qualified here |
| Real browser network, actor/PTY, phone, production enablement | Separate unverified gates; Node loopback and fake writer are not device evidence |

## Stage2 controlled browser entry

`browser-remote.html` is the ordinary browser entry. It mounts the original
RemoteTerminalPanel through an explicit connection source; the desktop panel
keeps its existing document IPC source by default. The panel still offers manual
snapshots and bounded literal input, rather than continuous terminal deltas or a
complete remote lifecycle UI.

The controlled HTTPS deployment must serve COOP `same-origin` and COEP
`require-corp`. The original top-level visible Document must remain secure and
cross-origin isolated. A missing or ineligible source stays Locked. The fixed
same-origin `/browser-profile.js` module may export only
`readApprovedBrowserInputs(signal)`. Its approved result is the exact
`ApprovedBrowserInputs` type in `ui-terminal/src/browserEntry.ts`: existing
nonextractable P-256 TLS/grant CryptoKey handles, original profile/RP/credential
metadata, exact Host pin, grant scope/principal, independent explicit relay
identity/signer, artifact URLs and the original verified presenter. No production
profile, enrollment, raw private-key JSON, persistent store or default identity
is provided. This module is trusted deployment code, not a remotely supplied
configuration document.

Page/profile origins are HTTPS. The AppBind `trust.origin` is the exact canonical
WSS relay endpoint, including its path. The original relay independently needs
the exact Page Origin in its browser-origin allowlist; no relay change or
wildcard is introduced. The browser sends the original first-binary admission
proof and unchanged v1 outer framing. Inner TLS 1.3 RPK, AppBind, COSE/WebAuthn,
grant, peer installation and Data remain in the original Rust/WASM implementation.

One absolute setup deadline covers operator module load/read, relay admission,
WASM/glue loading, TLS and the original application descriptor. Cancellation
seals new work while retaining original import/read/sign/IO promises until they
settle. Failed/uncertain retirement cannot create a second source to bypass its
owner. Descriptor success ends the setup timer; it does not impose a connection
TTL. Browser-owned pre-event networking allocation is outside JS queue limits.

Run `npm run build:browser` with the already installed Rust wasm32 target,
compatible clang and official wasm-bindgen 0.2.129 available on PATH (or set `CC`
and `WASM_BINDGEN` for that command). The script builds offline/locked original
libraries, links the six direct forwards in `remote/browser-runtime.rs`, then
generates unmodified official glue and checks the exported ABI. Vite copies
`public/remote-runtime` into `dist/remote-runtime`. The approved module may use
same-origin `/remote-runtime/fixture_bg.wasm` and
`/remote-runtime/fixture_bg.js`. The retained output basename is ABI naming;
the production runtime has no `TEST_ONLY_*` export. Generated artifacts are
ignored; their manifest records source, exact rlib and output hashes.

Fresh native admission can now rebind the same application-owned Host resource:
RunKey, SupervisorCore, SupervisedRun and original NativeRun identity must match
the existing pin. The Host derives its own target and latest actual Installed
fence. The original actor rechecks current local authority immediately before
replacement; it never takes the producer again. All bounded peer pumps remain
owned through join. Pending, Unknown, LocalOnly and latest Rejected attempts
fail closed. In particular, this slice conservatively requires local recovery
after a known Rejected installation; it does not search older fences to promise
uninterrupted retry.

The application native provider/identity/bootstrap registration remains Locked
by default. Deployment still needs an approved operator module, trusted HTTPS/RP,
relay browser-origin configuration, existing identities and a native provider
that invokes the original admission path. The browser page does not create those
authorities. Browser lifecycle Create/Attach/Resize/Resume/Close commands remain
outside this entry; disconnect only retires its connection.

The original relay library supplies the verified `Relay::serve` WSS path used
by this slice. The sibling cc-desk-relay checkout now has an explicit production
policy/TLS launcher. Trusted operator policy and certificates, authorized
deployment and device qualification remain separate gates; this fixture still
uses the original Relay library with public TEST_ONLY identities.

Stage2 verification includes an actual Edge TEST_ONLY routed HTTPS/assets/WS
fixture with the final product page/Worker/runtime, original loopback TLS WSS
Relay, original Rust Host, synthetic assertion signatures verified by Rust,
snapshot/exact ACK and one HostWritten effect. The WS bridge verifies a fixed
public test CA/name and connects the Host only on the first actual client TLS
bytes. This does not qualify browser WebPKI, real WebAuthn, phone suspend, or a
real CLI. Separate application composition uses an explicitly self-owned test
Node process and original native writer/pin/rebind/retirement. Its permanent
process admission requires each exact composition test to run in a fresh process.

显式 relay 会合模式、批准输入冻结和 Ready 前禁止内层 TLS 的边界见 [RENDEZVOUS-V1](RENDEZVOUS-V1.md)。Ready 不代替本文件的 BrowserProfile 或最终 grant。

## Supplier through the complete Browser entry and original Relay

The optional page-local approval owner and original-operation retirement contract
are documented in [Browser enrollment lifecycle](../docs/browser-enrollment-lifecycle.md).
This revocable in-memory authority does not implement real enrollment or enable
the default Locked entry.

Controlled modules can supply the production [grant presenter](../docs/browser-grant-presentation.md)
for explicit review of the original verified grant before the existing WebAuthn
step. This dialog does not replace the native Host approval or establish enrollment.

An explicitly installed [origin-local enrollment store](../docs/browser-enrollment-store.md)
can supply existing non-exportable handles across pages. Its persisted revision,
revocation tombstone and original-owner completion marker do not authenticate the
initial enrollment or permit automatic recovery after uncertain retirement.

The optional native [two-stage registration evidence verifier](../docs/browser-registration-evidence.md)
now checks explicit existing-key possession and synthetic WebAuthn creation plus
credential possession. It returns only unapproved evidence, not account authority,
GUI approval or store installation wiring.
The separate backend-only [native registration approval gate](../docs/native-registration-approval.md)
now binds that original evidence to its authenticated native document/run and the
original one-shot native approval machine. It may install only the matching frozen
Host configuration; a separate Host session approval/start remains required and
the controller stays Locked. This is not a public registration command, account
identity root, authenticated bootstrap channel or durable browser installation.

The experimental backend-only [native registration channel](../docs/native-registration-channel.md)
adds separate mutual-RPK TLS with `cc-desk-enroll/1`, original bounded lifetime,
ordered evidence messages and transcript-bound historical decision/ACK. It joins
the original verifier and approval owner, checks both actual endpoint identities,
and preserves the real native installation receipt if delivery fails. A receipt
is not durable trust or a grant. A separate [BrowserWasm registration owner](../docs/browser-registration-worker.md)
now carries actual browser WASM TLS through a dedicated Worker and original-handle
bridge to that native gate. Fifteen actual Edge cases verify the loopback path,
including original-owner retirement and uncertainty after WASM traps. They use
synthetic authenticator responses and the original cfg(test) native approval callback.

The registration-only [Relay lifetime owner](../docs/native-registration-relay.md)
adds bounded opaque keepalive without renewing the original total deadline. Its
independent actual WSS acceptance crosses 35 seconds of idle time in explicitly
selected legacy mode and closes at the original 40-second end. A separate
[rendezvous registration acceptance](../docs/registration-rendezvous-acceptance.md)
passes five actual-service cases with exact Ready and original-deadline gates.
These Node-only checks do not qualify the Worker-plus-Relay combination. Production
identity authority, independent pins, genuine native GUI approval and durable
installation remain separate engineering gates; default entry stays Locked.

The browser-entry.spec.ts fixture supplies the fixed same-origin profile module
through createApprovedBrowserProfileReader, instead of directly assembling
approved inputs. Its independent TEST_ONLY snapshot comes from the actual Host
scope and fixed public vectors. Only a separate candidate copy is mutated. The
provider receives the original signal, not candidate selectors, and returns the
original nonexportable handles and callbacks.

The matching case runs the final Vite page, controlled source, production
WebSocket/brokers/Worker and Rust WASM against the original Relay and Host. It
checks snapshot/exact ACK, one literal input writer effect, original source reuse
on refresh, and Relay retirement. Four valid mismatches (session, run, generation,
Host SPKI) check zero browser signatures, assertions/presentations, Relay
connections, Workers and runtime loads. Second refresh cannot reread or resurrect
the failed source. Test attachments record SHA256 of each actual Relay/Host/WASM/glue input.

The oracle source and lockfile are tracked under tests/remote-browser/relay-host-oracle,
without historic machine paths. Select an existing Relay checkout explicitly;
build output and temporary files stay inside the current project:

```powershell
$env:CC_DESK_TEST_ONLY_RELAY_ROOT = 'E:/source/github/cc-desk-relay'
node tests/remote-browser/build-entry-relay.mjs
$relay = Get-Content .audit/browser-entry-relay-build/artifacts.json -Raw | ConvertFrom-Json
$env:CC_DESK_TEST_ONLY_ORIGINAL_RELAY_ORACLE = $relay.executable
New-Item -ItemType Directory -Force .audit/browser-entry-run/runtime | Out-Null
$env:TEMP = (Resolve-Path .audit/browser-entry-run/runtime).Path
$env:TMP = $env:TEMP
$env:TMPDIR = $env:TEMP
$env:CC_DESK_TEST_ONLY_BROWSER_RESULTS = (Join-Path $PWD '.audit/browser-entry-run/results')
# Set CC_DESK_TEST_ONLY_DATA_HOST / DATA_WASM / DATA_GLUE to exact artifacts
# from the preceding Host/WASM procedure. Missing inputs fail, never skip.
node node_modules/@playwright/test/cli.js test --config playwright.remote-browser.config.ts tests/remote-browser/browser-entry.spec.ts --reporter=line
```

The oracle receives only repository public test TLS material. The WSS bridge
verifies the public test CA/name; HTTPS assets and browser WebSocket routing
remain Playwright-controlled. Real Edge, Relay, Host and WASM run with synthetic
WebAuthn and an in-memory writer. This does not qualify real enrollment,
browser-native WebPKI/DNS, a deployed launcher, a real CLI, phone behavior or the
installed application's native approval dialog. Fixture clicks do not bypass or
replace that real approval gate.

The same fixture now supplies the independent TEST_ONLY record through the real
`createBrowserEnrollmentAuthority` owner. Additional cases revoke an active source,
hold the original presenter pending across revocation, and revoke during source
retirement registration before any side effect. The operator helper and final
product source are separately bundled, exercising the original signal/retirement
contract across that boundary. No real credentials, storage, or deployment are used.
