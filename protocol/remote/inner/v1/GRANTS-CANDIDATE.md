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

## Opt-in grant management exchange

The new `bind_host_grant_channel` / `bind_client_grant_channel` factories require
both AppBind offers to contain exactly `canonical-json, grant-management-v1` and
select minor1. The original bind factories and their required capability remain
unchanged. Mixing these modes rejects; no downgrade, CLI-version gate or ordinary
application entry is enabled. The negotiated mode is private channel state.

Before data ownership starts, one consuming exchange owns the unsplit TLS channel.
All records are existing type2 Control/minor1. JSON messages are canonical, bounded
at16384 bytes, depth16, with unknown/duplicate/noncanonical fields rejected:

- `Request {nonce,reuse}`: nonzero256-bit nonce, boolean reuse. The client requests
  only its own exchange; native Host policy supplies principal/scope/role/duration.
- `Challenge {nonce,binding,grant}`: exact echoed nonce, actual exporter binding
  and the existing CanonicalGrant. Host Gate creates this only for an issue request.
- One binary record containing the existing exact tagged COSE_Sign1 bytes. Its
  format, protected fields, canonical payload and AAD are unchanged. Only this
  expected phase accepts binary COSE; JSON and data cannot substitute for it.
- `Accepted {nonce,binding,grantId,digest}`: actual Host Gate acceptance, exact
  grant ID and SHA256 of the submitted COSE. Every echoed field must match.

Message names above are literal wire tags; `grantId` is camelCase. The fixed
AppBind profile1/capability defines this narrow grammar. These are management
records, not authenticated Data envelopes; TLS/AppBind ownership is mandatory.
After successful acceptance, type5 Data retains its existing bounded schema.

`NativeGrantSigner::confirm_and_sign` receives only an immutable native verified
challenge object. Its construction/polls must not block the async owner and Drop
must cooperatively cancel owned work. Deadlines cannot preempt blocking native
callbacks. A real platform signer requiring isolation is a separate qualification;
this candidate includes no administrative key provisioning or presence provider.
Endpoint TLS identity keys are refused as administrative issuers before prompting.

Host Gate still owns one pending challenge/device/context, strict creation+30s
response admission and creation-derived expiry <=900s. Client `ClientGrantState`
retains one frozen signed candidate and a native monotonic sample captured before
its initial request. Its mapped expiry consumes exchange/presence delay; neither
arrival nor reconnect resets that anchor. Lost acceptance replies retain the same
candidate; a fresh authenticated binding can submit those identical bytes for Host
idempotent acceptance, keeping original creation/expiry and avoiding a second
presence prompt. Changed context, issuer or expired/revoked Host state refuses.
Cold clients cannot reconstruct this opaque state from COSE alone.

Only an exact Accepted reply makes `RemoteGrant` installable, and then only on its
ACKed Client binding. Host installation/admission refuses RemoteGrant; native Host
VerifiedGrant stays distinct. The existing bounded owner uses a private access enum
with a second client-role guard before starting IO. Legacy native-capability fixture
installation remains compatible; the opt-in Client mode refuses that shortcut.
Client clock rollback/error permanently locks its retained state. Its native clock
must count suspend or explicitly invalidate continuity; no mobile clock/provider
qualification is claimed by synthetic tests.

Each exchange has one absolute positive budget <=30s plus sticky cancellation.
Cancellation, malformed/order errors or timeout drop that TLS owner. There is no
pregrant data API or unbounded management queue. Grant expiry closes the existing
data connection; same-channel renewal remains deferred. Fresh authenticated
management reconnect and a fresh native-approved grant are required for renewal.
Actual platform administrative signing, enrollment/reset persistence, cold recovery
and real-device delivery remain OPEN; this memory-only slice enables no listener.
