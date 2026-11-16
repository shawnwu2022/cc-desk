# Offline browser artifact preflight

Run `node scripts/check-browser-deployment.mjs --site <project-local-dist>`.
If the separately reviewed registration source is installed, also supply
`--source-manifest-dir <project-local-source-build>` containing its generated
`manifest.json`. No source inputs, credentials, or manifest command paths are read.

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
remain UNVERIFIED. A normal build without browser-registration.js is a concrete
MISSING blocker. Do not replace it with a test or self-approved provider.

Process regressions: `node --test tests/scripts/browserDeploymentPreflight.node.mjs`.
Fixtures and temporary files stay under `.audit/deployment-preflight-tests`.
