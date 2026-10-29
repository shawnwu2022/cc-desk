# Candidate AppBind wire contract

Unreleased software profile1/security1, major1/minor0; required capability is exactly
[canonical-json]. Security version is a compiled property, not minor+1. This schema
does not implement data grants, session operations or a released N/N-1 matrix.

All messages use existing type4/minor0 framing with complete JSON payload<=16384 bytes.
Precheck nesting<=16, directly deserialize strict typed DTOs (including duplicate and
unknown-field rejection), validate all fields, then require byte-equal canonical
re-encoding. Public struct construction or standalone Serde is not validated authority:
encode/encode_selection/transcript/negotiate also validate their inputs.

Canonical JSON is compact UTF8 with recursively lexicographically sorted ASCII field
names; no optional whitespace/alternative escapes/order. Epoch/floor/security u64s
are canonical decimal strings, no leading zero except0 and no overflow. IDs,
activation and32-byte nonces are lowercase64 hex. Minor/capability arrays are sorted
unique, at most8 entries; tokens match [a-z0-9-]{1,32}. Numeric profile/major/minors are
u16 JSON integers. Confirmation MAC Debug is redacted; only lowercase64 hex is valid.

CanonicalOrigin has a2048-byte raw and normalized limit. Reject raw nonprintable or
non-ASCII characters, quote/backslash, @ and % before url2.5.8 parsing can erase banned
syntax. Require wss, host, no userinfo/query/fragment; canonical path uses only ASCII
letters/digits and / . _ ~ -. Trusted construction normalizes once; wire must equal
the exact serialized result. This restricted recipe is not RFC8785 or arbitrary URL
semantic equivalence.

## Exact fields

Context: hostEndpointId, clientEndpointId, hostKeyEpoch, clientKeyEpoch, activation,
incarnation, policyEpoch, issuerEpoch, origin.

HostOffer: context, hostNonce, profile, major, minors, minimumSecurity,
requiredCapabilities. It contains no prediction of requestedRole.

ClientOffer: context, hostNonce, clientNonce, profile, major, minors, minimumSecurity,
requiredCapabilities, requestedRole. Role names: ManagementOnly, Observer, Controller;
Only ManagementOnly is supported by AppBind itself; other requested roles reject.
The additional minor1 native data-channel candidate and separately verified
session grant are specified in GRANTS-CANDIDATE.md. Legacy factories retain0.

HostSelection: profile, major, minor, securityVersion, requestedRole.
HostConfirm: selection, mac. ClientConfirm: mac.

Message envelope: {"message":<exact variant name>,"body":<exact corresponding object>}.
Struct/enum Serde declarations in appbind.rs are the executable field ledger.

## Transcript and channel confirmations

Transcript bytes: b"CC-DESK-APPBIND-v1\0" then LP32 of canonical full HostOffer Message,
LP32 of canonical full ClientOffer Message, LP32 of canonical HostSelection object
(no envelope/MAC). LP32 is a big-endian u32 byte count. Literal domains end in one NUL.

T=SHA256(transcript). TLS exporter32 label b"EXPORTER-CC-DESK-APPBIND-v1", context Some(T).
HMAC-SHA256 input is b"CC-DESK-HOST-CONFIRM-v1\0"||T or
b"CC-DESK-CLIENT-CONFIRM-v1\0"||T. Ring verifies tags; semantic selection is independently
recomputed, never accepted merely because a peer can MAC it.

vectors/appbind-independent.json contains independent Node byte/digest expectations:
canonical parts606/722/88, T=f430d1db63dd8dca60e82e54eba1441fbc255bfb6b56638d75597145e5203e1f.
Codec tests match full bytes; actual security tests match that digest and reject both
captured confirmation directions across fresh TLS with identical full transcript.

The pure transcript helper validates encoding, not authorization or negotiation.
The security owner additionally validates pins/roles, exact immutable Host context,
nonce echo/distinctness, compiled profile/floor/capability/minor/role selection.
