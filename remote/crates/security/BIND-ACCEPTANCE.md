# Limited AppBind owner acceptance checklist

| Requirement | Evidence / honest scope |
| --- | --- |
| Shared canonical schema and independent complete transcript bytes | Shared5 AppBind tests passed; root standalone shared8/8 includes3 framing tests. |
| Host-first, actual IDs/roles/key epochs/origin, current captured policy/issuer | Real honest TLS binding; ClientTrust intentionally has no prior policy/issuer fields, learns current13/17 despite stale test cache. Real store freshness/epoch commit remains OPEN. |
| Four messages, actual TLS exporter, directional library HMAC verification | Actual duplex and WS/mux; no software-generated identity credentials in product source. |
| Candidate profile1/security1/minor0/ManagementOnly | Explicit negotiation and independently checked selection; unsupported role/floor/caps/minor rejects, no silent fallback. |
| Same transcript on new TLS rejects old Host and Client tags | Test captures A, same-transcript B succeeds with new tags, fresh C/D reject both A-tag directions; independent T digest matches. |
| One original setup deadline, owned unsplit IO | Actual consumed budget, silence, drip, retained-peer half-close, unread bounded-buffer backpressure, truncation/oversize, cancellation/abort DropProbes. |
| Success then backend cancel / last handle closure / local close / owner Drop | Real owner tasks consume token, peer EOF converges, close joins; Drop requests/aborts without claiming synchronous join. |
| No arbitrary record/plaintext/session/grant/dispatch API | Private TLS and opaque verified projections; unsupported post-bind byte closes. Metadata does not grant a current permission. |
| Full actual WS/mux composition | Binding integration6/6 includes160-fragment full protocol, driver/proxy tasks joined. Outer WebPKI/admission is a separate remaining integration gate. |
| Latest binding attack and IO matrix | Bind unit12/12, integration6/6; shared AppBind5/5. Root runs final full-workspace/strict inventory independently. |

Reproduce with cargo test --offline --locked --manifest-path remote/Cargo.toml
-p cc-desk-remote-security --all-features and the standalone shared manifest
protocol/remote/inner/v1/Cargo.toml. Evidence is task/integration-evidence/appbind/:
01..06 RED/GREEN,07 lifecycle,10 adversarial,11 IO/deadline,12 actual WS/mux.

OPEN: persistent enrollment/allowlist/native signers and storage; policy-store freshness
and atomic binding commit/revocation/rollback; actual management DTOs and liveness;
CanonicalGrant/fence/session/PTY execution; KeyUpdate/lifetime rules; composed outer
WSS with real relay admission; devices/platform and production release evidence.
This checkpoint does not close full E02 or production delivery. No deploy/push/merge,
persistent access or OS security setting changes were performed.
