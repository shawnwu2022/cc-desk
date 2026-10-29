# Unreleased grant and data-channel candidate

This is a software contract, not released enrollment or session authority.
`grant.rs` is the executable DTO/field ledger. Grants use strict sorted compact
JSON <=8192 bytes, duplicate/unknown fields rejected, decimal-string u64 values,
lowercase nonzero hex IDs, exact existing AppBind Context, principal, session/run/
generation and Observer/Controller role. Time is Host monotonic creation/expiry;
challenge < expiry <= checked(challenge+900000).

The signed profile is tagged COSE_Sign1 ES256: exactly protected alg=-7, content
type `application/cc-desk-grant+json`, kid=SHA256(literal
`CC-DESK-GRANT-ISSUER-v1\0` || canonical issuer SPKI), empty unprotected map,
attached exact canonical grant payload, signature64-byte r||s. External AAD is
literal `CC-DESK-DATA-GRANT-v1\0`. NUL is one terminal zero byte. The verifier
compares the entire known canonical encoding prefix then uses coset/ring to
verify the signature; it never parses or allocates from attacker CBOR lengths.

AppBind minor1 is now an additional supported candidate, selected only if both
offers include1; otherwise common0 remains unchanged. Legacy ManagementOnly
factories still advertise0. New data factories advertise0,1 and refuse selected0.
This is protocol compatibility, not a CLI software version restriction.

New inner type5=Data is valid only at minor1. Its bounded binary payload is:
binding32 || grantID32 || scopeDigest32 || fence:u64BE || sequence:u64BE ||
bodyKind:u16BE || body. bodyKind2=Control,3=Projection. Prefix=114 bytes.
The complete payload including prefix must fit16384 for Control or32768 for
Projection. Zero IDs/fence/sequence and every other body kind reject.
scopeDigest=SHA256(`CC-DESK-DATA-SCOPE-v1\0` || canonical SessionScope JSON).
binding is actual TLS exporter32, label `EXPORTER-CC-DESK-DATA-BIND-v1`,
context=AppBind transcript hash. It is fresh across TLS connections even with
identical endpoint/policy context. Direction sequences start1 and never wrap.

The fence is compared with the native receiver's expected fence; it does not
replace current Host lease/window/operation/receipt validation at PTY commit.
Opaque body bytes remain untrusted and require their own strict operation or
projection codec. No data dispatcher may select a local/internal writer bypass.

The current in-memory native Gate assumes enrolled issuer/principal/current
policy input. It is not a persistent identity store or user-presence ceremony.
One gate/device/context must be held by the actual Host policy owner. Accepted
grant reuse retains its original expiry; a cloned VerifiedGrant cannot open IO
without a fresh authenticated channel with exact matching context and live gate.
Over-network challenge/renewal and the actual Host actor adapter remain OPEN.
