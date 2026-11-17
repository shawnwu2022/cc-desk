# Native registration channel

## Scope and design

Build a separate mutual-RPK TLS registration channel using independently approved existing identities and pins. Its fixed ALPN is `cc-desk-enroll/1`; it cannot become the existing AppBind, grant, peer-install or terminal data channel. Keep the original TLS setup cap of 10 seconds and anchor the bounded registration lifetime (at most 120 seconds) before the handshake. Every stage keeps that original cancellation and deadline.

The native channel carries exactly create request, creation response, assertion request, assertion response, historical native decision and exact acknowledgement. Bounded frames and a transcript digest bind the exact exchange and authenticated endpoint identities. A decision reports only the result of the original native run-local candidate installation; it is not a bearer token or durable authorization. Lost decision/ack after installation means locally installed with peer outcome uncertain, never permission to reinstall.

The native implementation reuses the original TLS machinery internally with sealed separate public types. Reusing ManagementOnly or filling a fake enrolled credential to enter the existing BrowserWasm path is rejected. A later [isolated BrowserWasm registration owner](browser-registration-worker.md) reuses this same codec with its own signer, Worker, timer, producer bridge and joint retirement. No public network endpoint or automatic credential action is enabled.

For a native owner whose ceremony already began before Relay admission, use `registration_accept_until` with the original setup and total end instants. It caps TLS by the earlier original end, checks cancellation and expiry again after TLS, and stores the exact original total end in the channel and native approval owner. Reconstructing that end as `now + remaining` can move it right by the invocation delay; closing the outer socket alone cannot fence a synchronous approval or installation. The original duration-based entry remains available for a ceremony that starts at that entry. The new native entry does not alter browser timing, endpoint identity authority or the 10-second setup and 120-second total caps.

## Implementation plan

- [x] Add a failing mutual-RPK test, then separate TLS ALPN admission and sealed registration pending types. Test normal/registration ALPN incompatibility, wrong pins and original setup limits.
- [x] Add failing ordered-exchange tests, then bounded single-use native channel methods. Test wrong phase, malformed/truncated frames, cancellation, original total deadline and transcript/ack mismatch.
- [x] Preserve the actual native controller installation receipt; connect the original registration owner to the channel without a boolean approval substitute.
- [x] Drive actual Edge producer bytes through a TEST_ONLY pipe to a native TLS client and then the real encrypted channel to the original native owner. Prove Yes/deny/revocation and uncertain outcome behavior, zero Host-session activation, and actual connection/fixture retirement.
- [x] Run affected original TLS/approval regressions, strict checks and independent review before the local commit.

## Explicit remaining gates

Pins/handles still require independent approval before real bootstrap signing or connection. This code does not establish account identity, credential uniqueness, persistent authority, native durable trust or a trusted browser installation receipt. It does not call `browserEnrollmentStore.installApproved`. Direct-loopback TLS acceptance does not cover relay idle windows (the original relay cap is at most 30 seconds); a future relay registration owner needs bounded keepalive without extending the total deadline. Real operation requires explicit authorization for the actual origin/RP, account association, authenticator, existing key handles, recipient/pins and intended persistence. No real credentials or deployment are exercised.

## Native ownership and delivery contract

`NativeRegistrationSession` accepts only an initial `Create` owner and fresh authenticated `HostCreate` channel. A previously verified owner cannot be rebound to another TLS transcript. The actual client endpoint must match the registration context, and the frozen Host endpoint must match the channel's actual Host. Verification moves both original owners out before awaits, so dropping the future closes both. Approval and installation recheck the original channel cancellation, deadline and decision phase; a cancelled channel cannot consume a native Yes and install a candidate.

Installation synchronously obtains the controller's real run-local receipt before delivery. `InstalledRegistration` keeps that historical receipt while its delivery method takes the channel exactly once. A failed or cancelled decision/ACK exchange cannot erase the receipt, undo installation or trigger a second delivery/install. The controller remains Locked; no Host session or data writer starts.

## Isolated acceptance (2026-11-14)

- Remote security: 106 library tests, 6 binding tests and 12 RPK tests passed, including 14 new registration channel cases. The ignored browser oracle is exercised explicitly by Edge.
- Actual Edge: 36 registration tests passed, including 11 loopback TLS/native-owner cases: Yes, deny, document revocation, foreign caller, mismatched Host/client endpoints, install failure, lost ACK, lost decision, cancellation after Yes, and attempted owner rebind.
- Original native approval/candidate/scope regressions: 11 + 5 + 2 passed. Remote security and native lib/tests strict Clippy passed.
- Independent review found cancellation and owner-rebinding gaps; both were fixed, regression-tested and reviewed again with no remaining blocker.

The first loopback fixture used a Host test identity different from the original frozen supplier. The exact endpoint check correctly rejected it. The fixture now uses that supplier's original published test endpoint; the production check was not relaxed. Native dialog completion remains a cfg(test) callback into the original approval machine, and WebAuthn uses synthetic API responses. These results do not qualify actual GUI approval, device authenticators, remote BrowserWasm or durable enrollment.
