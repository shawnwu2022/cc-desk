# Independent registration runtime build

The production registration page has its own WASM product. It never substitutes
its registration owner for the enrolled terminal/application runtime.

From the repository root, run:

```powershell
# Use already installed compatible tools. No installation or credential setup is performed.
$env:RUSTUP_TOOLCHAIN = '<installed Rust toolchain with wasm32-unknown-unknown target>'
$env:CC = '<absolute path to compatible installed clang.exe>'
$env:WASM_BINDGEN = '<absolute path to official wasm-bindgen 0.2.129 executable>'
npm run build:browser:registration-runtime
```

The script runs locked, offline Cargo for `cc-desk-remote-security`, with only
`experimental` enabled, and selects exact wasm32 rlib filenames from that
invocation's `--message-format=json` output. It checks the original security
`src/lib.rs` lineage and rejects ambiguous, native or test artifacts. It links
`remote/browser-registration-runtime.rs`, whose four exports only forward to
original registration functions. There is no copied registration state machine,
TEST_ONLY wrapper, client/AppBind export, new protocol or new trust root.

The independent output contract is:

- `public/remote-registration-runtime/fixture_bg.wasm`
- `public/remote-registration-runtime/fixture_bg.js`
- `public/remote-registration-runtime/manifest.json`, kind
  `OriginalBrowserRegistrationRuntime`

The basename is the original Worker import ABI, not a test marker. The Worker
owns initialization and calls the existing four `browser_registration_*`
exports. The optional `fixture.js` auto-start module from wasm-bindgen is kept
only with build audit files, not published in this product directory.
The script requires the registration functions, exported memory and bindgen
startup; rejects TEST_ONLY exports/imports and unrelated browser exports; and
checks the registration and standalone-glue import modules. All checks complete
before copying output. Cargo, link and policy failures leave previous output untouched.

`manifest.json` records compiler/tool versions and exact commands, source and
Cargo lock/manifests hashes, exact input rlib hashes, the original Cargo JSON
record hash, module imports/exports and final WASM/glue bytes and hashes. Source
hashes are rechecked after compilation to reject a mixed source snapshot. This
is build provenance, not a credential, signature or enrollment approval.

Cargo's default target, temporary directories, command logs, raw WASM and staged
products stay in `.audit/build-browser-registration-runtime`. An explicit
`CARGO_TARGET_DIR` must resolve within this checkout; outside paths and escaping
junctions/symlinks are rejected. The generated deployment files above are the
only output outside `.audit`. No dependency download, install, deployment,
credential creation or security-setting change is part of the build.

The existing `npm run build:browser` and `public/remote-runtime` terminal product
are unchanged. `npm run build:browser:product` explicitly builds both runtimes
then runs Vite for a complete static bundle. Set `CARGO_TARGET_DIR` to a path
inside this checkout (prefer `.audit/browser-product/target`) and keep TEMP/TMP/
TMPDIR inside `.audit` when using that combined command, because the unchanged
terminal script retains its existing defaults. This command still does not
provide `/browser-registration.js`, independently approved registration inputs,
real credentials, or deployment. Alternatively, build registration before the Vite production
build when including it in a site. The separately approved registration provider
can select same-origin `/remote-registration-runtime/fixture_bg.wasm` and
`/remote-registration-runtime/fixture_bg.js`; the independent approved plan still
selects terminal runtime artifacts separately. The first registration provider
remains explicitly trusted operator code. Building these bytes does not supply
keys, pins, a user association, WebAuthn permission or native approval.

Focused build-policy regression:

```text
node --test tests/scripts/browserRegistrationRuntime.node.mjs
```

These tests check export policy, exact Cargo artifact/source selection, the
link-only wrapper and project-local output containment. A successful real WASM
build establishes its linkage and artifact policy, not actual browser/native
registration, a genuine authenticator, persistence, a deployed site, or installed
WebView/device qualification. Those require separate acceptance evidence and
specific authorization for real credentials.
