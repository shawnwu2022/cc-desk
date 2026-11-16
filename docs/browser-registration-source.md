# Controlled registration source supply

`browserRegistrationProfile.ts` provides a reusable adapter for the independently
controlled `/browser-registration.js` source. It does not supply a real trusted
provider, select a Host/user, create keys, or grant enrollment permission. The production build now supplies a separate local-operator preparation flow. The reusable provider adapter remains available for explicitly supplied custom sources.

## Public configuration and independent approval

`parseBrowserRegistrationConfiguration(bytes)` accepts at most 16 KiB of strict
UTF-8 JSON with exactly `{version: 1, plan, artifacts: {wasm, glue}}`. It rejects
duplicate/unknown fields, including credential facts, receipts and private keys.
`plan` uses the existing `BrowserRegistrationInstallationPlan` schema. Both the
installer and parser now call `snapshotBrowserRegistrationPlan`; there is no
second installation-plan validator.

The returned public value is deeply frozen. This is syntax validation only:
valid SPKI encodings, a Host pin, user handle or database name establish no trust.
The two `artifacts` objects have separate purposes: `plan.configuration.artifacts`
names the later terminal runtime, while the outer object names the registration
runtime. The latter requires canonical same-origin HTTPS URLs with no credentials,
query, fragment, percent encoding, backslashes or port zero.

`createApprovedBrowserRegistrationReader(candidateBytes, provider?)` returns the
function required by the controlled source's unique export. The trusted provider
has exactly one data-property method:

```ts
readApproved(signal: AbortSignal): Promise<{
  configuration: Uint8Array
  keys: {tlsKey: CryptoKey; grantKey: CryptoKey; relayKey: CryptoKey}
} | undefined>
```

`configuration` is an independently selected, already approved copy of the whole
public configuration above. The provider receives only the original signal,
never the public candidate or an identity selector. Every field must match,
including origin/RP, Host pin, user association, all public keys, database/revision,
scope/role/budgets, relay parameters and both pairs of runtime URLs. An omitted
relay mode does not match explicit `legacy`.

The reader captures the original three handles before parsing provider bytes and
requires three distinct non-exportable ECDSA P-256 private signing keys. It does
not import, generate, export or sign with them. Cryptographic private/public key
matching remains the original entry's pre-socket preflight and installer's
pre-commit proof; a reader success alone is not that evidence. No credential ID
is synthesized by this adapter.

Missing provider/approval returns `undefined`. Invalid or mismatched approval
rejects with a fixed error. Construction has no provider, storage, network,
authenticator or deadline side effects. The first explicit read owns one original
provider promise, including failures and cancellation. Concurrent/repeated calls
join it; cancellation cannot replace the signal, retry, or release a late result.
A later caller's own aborted signal rejects that call without poisoning a still
live original. The outer source and entry still enforce document lifetime,
deadlines, one registration and original retirement.

`createProductBrowserRegistrationReader` adds one restriction: the public plan
must already name `CC_DESK_BROWSER_ENROLLMENT_V1`, and the independent provider
must match it exactly. Other databases remain available through the generic
adapter. Neither helper rewrites or migrates an approved target.

## Building an explicitly supplied source

After separately implementing and approving a trusted provider, an owner-controlled
entry such as `.audit/approved-registration/entry.ts` can use:

```ts
import {createProductBrowserRegistrationReader} from '../../remote/ui-terminal/src/browserRegistrationProfile.ts'
import candidate from './candidate.json'
import {trustedProvider} from './trusted-provider.ts'

export const readApprovedBrowserRegistrationInputs =
  createProductBrowserRegistrationReader(
    new TextEncoder().encode(JSON.stringify(candidate)), trustedProvider)
```

`trusted-provider.ts` and its independent approval are deliberately not shipped.
It must not echo `candidate`, infer approval from registration success, or derive
identities from a receipt/URL/native Ready event. Do not put private key material
in source or JSON. Existing original handles must come from an explicitly
authorized same-origin owner. Malicious same-origin code is outside this boundary.

Build into a fresh or empty project-local directory:

```text
npm run build:browser:registration-source -- --entry .audit/approved-registration/entry.ts --out-dir .audit/approved-registration/build
```

The script produces one standalone `browser-registration.js` with exactly
`readApprovedBrowserRegistrationInputs` as its export, no remaining module
imports, and the same 1 MiB cap as the loader. It rejects outside-project paths,
nonempty output directories, unsupported input kinds, unresolved/dynamic imports,
extra exports and oversized results. It does not execute the supplied module,
certify its behavior or independently verify that the provider is trustworthy.

`manifest.json` records the base source commit plus the exact bytes/hashes of
inputs captured during bundling and the output hash. A dirty source tree is not
identified by commit alone; retain the complete inventory. No upload, deployment,
credential configuration or production runtime build occurs. If deployment is
separately authorized, serve these exact reviewed JS bytes at the fixed
same-origin path and the approved registration/terminal artifacts at their exact
URLs. Do not deploy TEST_ONLY fixture inputs or substitute this bundle for WASM.

## Evidence limits

`browserRegistrationProfile.test.ts` covers frozen syntax, independent identity
and artifact mismatches, original handle policy, missing/invalid providers,
cancellation/replay and delayed original promises. It imports public TEST_ONLY
scalars into isolated Node WebCrypto, not real keys. `buildBrowserRegistrationSource.test.ts`
executes the actual build script and imports the generated standalone module;
missing approval stays Locked, while an independently supplied TEST_ONLY provider
preserves all three original handles and is read once. Existing Edge entry/installation tests
remain separate checks for key proofs, persistence and lifecycle composition.
None is real-device, authenticator, trusted-provider, native GUI or deployment
approval. See [real-device acceptance](browser-real-device-acceptance.md).

## Product local-operator preparation

The product build emits the standalone fixed /browser-registration.js plus
browser-registration.manifest.json using the CLI's captured-input bundler.
An existing public/browser-registration.js is not overwritten: the product
build fails explicitly on that conflict. Custom deployments can still use the
standalone CLI and the same single reader export.

Prepare reads/imports the captured source once. Page load creates no keys.
A separate trusted click generates three distinct non-exportable P-256 private
signing handles in page memory; only public SPKIs are exported for separately
authorized Host/Relay setup. No private key is serialized. One absolute
five-minute preparation limit includes time away. Unapproved keys can survive
temporary visibility loss, but hidden pages cannot approve. Cancellation,
pagehide/freeze, document replacement or expiry ends this document's attempt.
Pending key generation/export is awaited before completion.

Public JSON is syntax, not authorization. The strict parser and local checks
reject mismatched public identities, product database or origin. A separate
immutable review exposes the exact Host SPKI Base64URL pin (not a hash),
requested user association, scope, role, budgets, relay, both artifact pairs
and storage target. Directional controls are escaped. Only another trusted click
approves this frozen local choice. This does not authenticate an account or prove
Host identity. Real Host pin checking and credential operations require separately
authorized use. TLS/key proofs and native/device/storage approvals remain unchanged.

Prepare returns no private inputs. Approved inputs are captured immediately,
consumed once by Start, and invalidated after at most 60 seconds or visibility
loss. Start establishes the original 10-second setup and 120-second registration
deadlines. Direct register without prepare fails. Reload is required to retry.

Isolated production Edge tests stop at a TEST_ONLY socket constructor after the
original entry's public/private preflight. They are not real authenticator,
native installation, live relay, deployment or persistent-trust evidence.

The offline checker reads the fixed emitted browser-registration.manifest.json
when no source manifest directory is specified. An explicit directory still
selects only manifest.json there; missing files do not trigger fallback.
Integrity PASS is not deployment readiness or operator authorization.
