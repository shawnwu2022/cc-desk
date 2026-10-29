# Software RPK checkpoint acceptance

Scope: Windows10 existing-PC software tests; no real enrollment/terminal grant.

| Check | Actual evidence / status |
| --- | --- |
| Genuine mutual TLS1.3 RPK, exact expected keys plus official signature helper | PASS; duplex and real loopback WS/mux, full/group/ALPN/raw-SPKI runtime assertions |
| Fixed profile, role/signer/DER/budget gates | PASS; config source + executable negatives; no arbitrary profile config API |
| AES128GCM and ChaCha20 | PASS; each separately negotiated against product accept |
| Canonical role-domain endpoint IDs | PASS; literal expected IDs from independent Node crypto oracle |
| Wrong pins, both signature directions, missing client identity, bad ALPN | PASS on the rejecting endpoint; no symmetric-completion claim |
| Silence, drip, half-close, cancellation, future abort and owned cleanup | PASS within the test's owned IO/tasks; aliases are outside this guarantee |
| Sticky clones/handle closure/repeated concurrent-caller wake boundary | PASS deterministic tests; no reset API, no scheduling stress claim |
| Final post-validation deadline and cancellation sampling | PASS; delayed-validation RED/GREEN; synchronous native callbacks are not preemptible |
| Shared bounded transport with actual WS/mux fragmentation | PASS;54 outer fragments; plain loopback WS, not composed outer WSS/admission |
| No public plaintext/BoundPeer/session API | Source/type review; Pending only exposes transport evidence |
| Security package all-features tests |16/16:4 unit+12 integration;0 failed/ignored, log14 and inventory18 |
| Strict Clippy / format / default-off compile | PASS logs15/16/17; default-off is not RPK test coverage |
| Full main remote workspace all-features regression | Root runs independently; do not infer it from package checks |
| AppBind/full DTO/issuer-policy synchronization, COSE grants, native signer/storage, KeyUpdate, platform and production release | OPEN; this checkpoint does not close E02 or release readiness |

Reproduction commands are in README.md. Key evidence:14-final-rpk-regression.txt,
15-final-clippy.txt,16-final-format.txt,17-default-disabled-check.txt,
18-rpk-inventory.txt. Initial REDs and rejected approaches remain in the same evidence
directory; compile/lint failures are not presented as behavioral REDs.
