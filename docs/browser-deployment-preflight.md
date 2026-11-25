# Offline browser artifact preflight

Run `node scripts/check-browser-deployment.mjs --site <project-local-dist>`.
Without an explicit source manifest directory, the checker reads only the fixed
`browser-registration.manifest.json` emitted in the site by the product build.
For a separately reviewed standalone source build, explicitly supply
`--source-manifest-dir <project-local-source-build>`; this selects only its
`manifest.json` instead. Neither mode falls back to another manifest when the
selected file is missing or invalid. No source inputs, credentials, or manifest
command paths are read.

The command only reads fixed public artifacts and writes JSON to stdout. Exit 0
means `localArtifactIntegrity: PASS` for the listed inventory, not deploy-ready.
Exit 1 reports missing/invalid files with stable codes. Redirect output only into
the project's `.audit` directory if a retained report is needed.

The two HTML entry files and browser-profile.js must be present and bounded.
Runtime manifests must identify the correct runtime and exactly fixture_bg.js and
fixture_bg.wasm. Bytes, hashes, actual WASM imports/exports and TEST_ONLY markers
are checked. WASM is compiled for inspection, never instantiated. Registration
source must match its separate manifest and have exactly the expected standalone
export. JS is parsed, never executed. Symlinks in supplied paths are rejected;
manifest targets cannot select arbitrary files. Run on a quiescent snapshot:
concurrent hostile filesystem replacement is not an isolation boundary.

Hashes establish consistency with the supplied manifest, not independent trusted
provenance. HTML/JS dependency closure and behavior are explicitly UNVERIFIED.
Provider authorization, Host/browser/RP identity, real public configuration,
HTTPS/WebPKI/COOP/COEP, native GUI/authenticator acceptance and deployment always
remain UNVERIFIED. The product build now emits browser-registration.js and its
fixed manifest, but this is not operator approval or deployment readiness.
A missing registration source is still a concrete MISSING blocker. Do not replace
it with a test provider or infer authorization from matching manifest bytes.

Process regressions: `node --test tests/scripts/browserDeploymentPreflight.node.mjs`.
Fixtures and temporary files stay under `.audit/deployment-preflight-tests`.

The browser-product output uses relative static paths. Keep both entry HTML
files, the built-in operator modules, the runtime directories, and all assets
together under the same HTTPS directory. Access the entry HTML by filename.
If the operator supplies the runtime artifact URLs, their approved values must
explicitly identify the WASM and glue of that directory; the build does not
rewrite those URLs. A path prefix is not a browser isolation boundary:
WebAuthn and browser storage remain origin-scoped. Use a dedicated origin for
independent enrollment/paired-session testing.

Only the ordinary 0.28 stable/RC release workflow stages the downloadable
mobile site. After the independent signed Windows installer check, the
same source builds an isolated browser-product site and runs this preflight.
The exact Windows candidate artifact carries the ZIP and a separate manifest
binding the archive and contained files to the source SHA, workflow run and
attempt. Promotion rechecks both after downloading the same-run artifact.
Neither file is an updater asset or a signed platform package. The manifest
states `deployReady: false`; availability, deployment authorization and real
devices are still not verified.

Check Vite-generated preview-path references without a browser or server:
`node --test tests/scripts/browserProductPaths.node.mjs`.
