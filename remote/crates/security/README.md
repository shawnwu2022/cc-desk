# E02 candidate: async mutual RPK transport authentication

This crate exposes no endpoint API with its empty default feature set. The explicit
experimental feature enables a fixed software transport-authentication component.
It grants no Host/session permission and cannot be attached to a real terminal.
Default workspace checks that skip the feature are not RPK acceptance; validation
must run all-features and check a nonzero RPK test inventory.

## Fixed profile and backend inputs

rustls0.23.46/ring and tokio-rustls0.26.4 are pinned. Only TLS1.3, X25519, mutual RFC7250
raw public keys, P256 ECDSA/SHA256, ALPN cc-desk-e2e/1 and AES128GCM/ChaCha20 are enabled
by the private config factories. TLS1.2, resumption, early data and server tickets are
disabled. Server storage is NoServerSessionStorage and its locked default ticketer
must report disabled. NoKeyLog is explicit on both sides; client SNI is disabled.
There is no public caller-supplied rustls config or accept-all verifier.

P256Spki accepts exactly91 bytes with the canonical P256 DER algorithm/curve/bitstring
header and uncompressed SEC1 prefix. This is canonical encoding validation, not an
independent elliptic-curve audit. Exact expected-SPKI comparison precedes rustls's
official verify_tls13_signature_with_raw_key; cryptographic validity/signature proof
is checked there. A pin equality alone never substitutes for CertificateVerify.

LocalIdentity takes a backend-held SigningKey trait object, checks its declared
algorithm, public SPKI and supported scheme. It has no private-key loader, system
keystore or credential creation API. Caller identity/enrollment and safe native signer
implementations remain outside this candidate. connect requires Client local/Host pin;
accept requires Host local/Client pin before polling transport IO.

## Unbound and Pending ownership

Unbound owns the input byte stream. Pending privately owns the actual negotiated TLS,
original setup deadline, sticky cancellation token and both role-domain endpoint IDs
derived from the verified local identity and exact expected peer pin. Neither type
exposes plaintext read/write or implements AsyncRead/AsyncWrite. Pending is not
BoundPeer, an application binding, a data grant, a session or an authenticated online
status. TLS1.3 client completion can precede the server rejecting its client key or
signature; tests assert the rejecting endpoint rather than inventing symmetric errors.
The next AppBind owner must complete mutual confirmation and authority checks.

One checked absolute monotonic deadline (chosen max10 seconds) starts at handshake
entry and is not refreshed by byte progress. Select/cancellation and a final sample
after evidence validation reject expiry/cancel before successful Pending publication.
That sample is the eligibility linearization point, not an impossible physical-return
timing guarantee. Synchronous SigningKey/native callbacks can block the executor and
cannot be preempted by async timers; native isolation/queue bounds remain OPEN.

cancellation() creates a private sticky AtomicBool plus watch notification. CancelHandle
only exposes cancel(), never reset/send. Every cancel call sets true and publishes a
wake, including repeated/cloned calls; token clones before/after cancellation remain
canceled. Dropping all handles also cancels. A deterministic unit test parks the wait,
injects the valid first-caller state-before-notify boundary, then checks a second
returned cancel wakes it. This is precise instruction-boundary injection, not a
claimed concurrency stress result.

Timeout/cancel/failure or dropped handshake future drops owned IO. A generic IO wrapper
may have physical aliases/split siblings that dropping this owner cannot close. The
current Pending does not run a background post-handshake cancellation loop; its retained
deadline/token are mandatory inputs to the next AppBind owner. That integration is OPEN.

## Candidate endpoint-ID bytes

This is an unshipped main-repository candidate rule, not a published compatibility
promise or the lab's TEST_ONLY domain:

- Host: SHA256(b"CC-DESK-HOST-ID-v1\0" || canonicalSPKI)
- Client: SHA256(b"CC-DESK-CLIENT-ID-v1\0" || canonicalSPKI)

The domains are literal ASCII including the terminal zero byte; SPKI is its exact
91-byte DER sequence. No text/hex/base64 encoding or JSON is hashed. Tests compare
against root's independent Node crypto oracle:

- RFC6979 public fixture Host ID:27b908a684ebfe768e4613c594495dbf502be7f7ad9d92182f1a3dac35e94d54
- COSE public fixture Client ID:b76b5e237aa760c2b1e65bd44e86a696a8fda16c0fd970366e1f86c1f8a73de2

## Actual validation and remaining gates

```powershell
cargo test --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-security --all-features -- --nocapture
cargo test --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-security --all-features -- --list
cargo clippy --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-security --all-targets --all-features -- -D warnings
cargo check --offline --locked --manifest-path remote/Cargo.toml -p cc-desk-remote-security --no-default-features
```

The real TLS tests assert protocol/group/ALPN/raw SPKI/full handshake and separately
force each allowed cipher with a test-only external client against product accept.
Negative cases cover both pins, both signature directions, mandatory client identity,
wrong ALPN, malformed DER, signer mismatch, roles/budgets before IO, silence, valid
ClientHello drip, half-close, cancellation and dropped tasks. The loopback WebSocket
test runs the production bounded_tunnel/drive codec; a test relay splits opaque TLS
bytes across54 mux fragments. It uses plain loopback WS, not composed outer WSS/WebPKI
or actual relay admission. Driver/proxy tasks are joined after closure/cancel.

Public-known private fixtures exist only in tests/support and tests/vectors, from
the earlier lab's RFC6979/COSE fixtures; everyone knows those values. They must never
be enrolled for real access. There are no TEST_ONLY keys or domain labels in product
source. No real account, persistent Host/Admin authorization or OS security change
was created. Windows10 evidence does not certify Windows11/iOS/Android.

AppBind owner/full DTOs, current policy/issuer synchronization, COSE authorization,
native signer/storage, KeyUpdate, production deployment/resource envelope and platform
acceptance remain OPEN. TLS1.2 disabled configuration is inspected; no TLS1.2 negative
interop run is claimed. This checkpoint is not the complete E02/production release.

## Subsequent limited AppBind software checkpoint

The RPK sections above retain their checkpoint scope. The subsequent implementation
adds HostOffer→ClientOffer→HostConfirm→ClientConfirm with real exporter/HMAC and a
private unsplit management-only owner; see BIND-REVIEW.md, BIND-ACCEPTANCE.md and the
shared APPBIND-CANDIDATE.md. It reuses the original setup deadline and sticky token,
does not require a phone to preknow current Host policy/issuer epochs, and grants no
session authority. Actual store freshness/atomic commit remains OPEN.

BoundContext/BoundSelection are non-Serde, privately constructed readonly snapshots
of this binding's immutable verified metadata. Even when cloned, they are not current
online state or callable authority. Owner close/cancel gives clones no lifetime or
grant. Future dispatch must require a live owner and fresh grant/fence checks, never
only a BoundContext. No dispatcher or arbitrary application IO is present here.

Successful owner tasks watch only the external token (no sender clone), a separate
local stop signal and peer EOF. Unsupported post-bind application bytes close with a
fixed Unsupported reason. close().await confirms owned task termination; Drop requests
stop/abort and does not claim synchronous join. Generic physical aliases and
non-returning synchronous callbacks remain outside the cleanup guarantee. The setup
deadline is not repurposed as a10-second data grant or online-status promise.
