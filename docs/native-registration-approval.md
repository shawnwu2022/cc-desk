# Native registration approval gate

This slice closes one concrete gap: the existing authenticated native document/run owner holds the original two-stage registration attempt, presents its verified result through the existing native `ApprovalMachine`, and consumes one Yes permit to install the matching frozen Host configuration. Installation remains Locked until the separate Host session approval/start flow. Registration approval never starts a connection, opens a key or grants terminal access.

The original `OwnerIdentity` checks caller, exact document binding and `NativeRunNetworkOrigin` at every stage and again at installation. A fresh backend-owned context label binds this attempt; that label is not a new trust root. Failed or repeated stages cannot reconstruct a verifier from JSON. Final confirmation owns the original `UnapprovedBrowserRegistration` and frozen configuration, compares the original TLS/issuer/browser fields, and displays the exact public evidence plus full Host description. The original registration deadline and frozen Host cap remain in force. Cancellation revokes verification and any outstanding approval.

## Implementation and isolated acceptance

1. Add an exact-match check on the typed verified evidence; no serialized approval import or authority conversion.
2. Wrap the original private `ApprovalMachine` for registration, retaining dialog exclusion and one-shot permits. Hold registration beside `RemoteHostController`, reusing `OwnerIdentity` directly.
3. Exercise the original native composition fixture and actual Edge producer with fixed TEST_ONLY platform responses. Use only the existing cfg(test) dialog-result seam, never a real GUI approval. Success installs one frozen candidate but stays Locked; denial, mismatch, cancellation and foreign owner do not install or start anything.
4. Run original approval/controller regressions, composed registration tests, strict compilation and independent review before local commit.

## Remaining production gates

This is a backend installation gate, not a newly exposed command or registration service. The full Browser entry already requires an enrolled credential, while AppBind does not negotiate registration messages. A separate registration channel must start from independently approved TLS pins and cannot bootstrap its own trust from a public candidate. Browser persistence must receive its final authorization through that authenticated channel. Plain SQLite/JSON approval rows are not restart trust authority; native durable trust still needs an explicit integrity/reload policy. These gaps are not replaced by fixtures or mutable callback booleans.

No real authenticator, key store, native dialog approval, persistent user credentials or deployment is exercised. Actual registration and installation require explicit authorization for the origin/RP, account association, authenticator and existing handles. Native confirmation is local-owner approval, not global account authentication or hardware provenance.

## Reproduce the composed acceptance

Follow the dual-oracle build and Edge commands in [registration evidence](browser-registration-evidence.md). The native fixture additionally requires `CC_DESK_TEST_RELAY_ROOT` pointing at the sibling `cc-desk-relay` checkout and `CC_DESK_TEST_RELAY_EXE` pointing at its project-local built `cc-desk-relay.exe`. It runs that original relay with published TEST_ONLY TLS material and a loopback, short-lived synthetic policy, but the registration gate must create zero Host network attempts and open zero supplier keys.

The native oracle uses the actual `DocumentAuthority`, `NativeRunAdmission`, registration verifier, original `ApprovalMachine`, frozen supplier/configuration and controller. Only platform credential responses and the native dialog callback are TEST_ONLY. Scenarios cover accepted Yes, denial, cancel, original owner expiry, frozen Host expiry, production clock clamp, failed installation followed by attempted permit reuse, different run, foreign caller, document replacement, credential mismatch and repeated create. Original resources must retire before the fixture emits its final receipt. This is isolated composition evidence, not genuine GUI, production identity or durable installation acceptance.

## Isolated validation, 2026-11-14

- Actual Edge: all 25 registration tests passed, including all 12 original-native approval combinations and 13 preceding producer/verifier regressions. Each native result records the exact libtest executable hash, zero supplier key opens, zero Host network attempts and Locked state. Only Yes installs a candidate, once.
- Security native suite: 92 library, 6 binding and 12 RPK tests passed. Exact-match tests cover changed owner, TLS, issuer, origin, RP, credential, credential key and backup policy, plus cancellation and the original deadline. The ignored browser oracle is exercised by Edge.
- Original native approval/candidate/pairing-scope regressions: 9 + 1 + 2 passed. Strict Clippy passed for the security all-targets and application remote-host library/tests. The security default-feature check passed.
- Independent read-only review found no blocking defect. A fixture's held admission and shared-supervisor retirement sequence were corrected without weakening original shutdown assertions; both original runs must satisfy the same bounded retirement checks.

These checks do not include a real native dialog interaction, real WebAuthn registration, production identity verification, durable installation, a deployed service or a complete production first-registration channel.
