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
