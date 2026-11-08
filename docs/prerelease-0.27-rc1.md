# 0.27.0-rc.1 review and publication boundary

The user authorized prerelease preparation on 2026-11-08, with a one-hour target.
This supersedes earlier local-only/no-version-change instructions for this release
work. It does not authorize bypassing publication gates or accessing live sessions.

Earlier stage11/candidate-review statements forbidding publication are preserved
as historical stage conclusions. For this newly authorized prerelease, the
authoritative `release-process.md` and unchanged
`required-checks-and-disclosed-host-unverified-v1` policy govern promotion. They
permit a signed RC with explicit `nativeAll=unverified` and
`nativeAcceptanceProven=false` after all required checks/coverage/signatures pass.
This is not a claim that full P0 or stage11 UI acceptance is complete. The pending
instance and its leases remain untouched; no denied operation is retried.

Target: `0.27.0-rc.1`, GitHub prerelease, `make_latest: false`. The tag and release
were absent when checked; publication must freshly verify this again. Protected
main was `0aaaf2a171ced55ff2b4d7a3e1731027c45809df`. The feature branch already
contains that source through the recorded non-rewriting integration. Only reviewed
changes may merge to main; signing/publication must use the resulting exact main
source and current ordinary CI run/attempt. No feature tag promotes this branch.

## Delivered scope

- Stable/Test/Preview policy, bounded download/cancel/install lifecycle, source and
  signature admission; historical version selection and manual recovery support.
- Local redacted diagnostic/version collection, exact-candidate ignore, update
  history and retry.
- Windows/Codex narrow permanent deletion with preview, independent second
  confirmation, one-shot per-root receipts, activity/source checks and unknown
  outcome reconciliation. Native subtree semantics are disclosed; count is unknown.
- Stage11 acknowledged-delete refresh and explicit data-directory forwarding.
- Stage12 Git checkout-byte integrity and Codex compressed-history rename fixes.

## Evidence and open conditions

The full four-harness/16-shard local result belongs to old source `68fa6b3d` and
must not be claimed for this RC tree. Later changes have their own bounded checks;
stage12 has 14 Node checks, 18 rename checks and strict independent-harness Clippy.
See the stage-specific records linked from `acceptance-checklist-0.27.md` and
`.audit/v027-stage12-validation.md`. Hosted PR and exact-main CI remain separate.

RC preparation additionally passed 148 release-policy/signature/history Node
checks, six synchronized metadata identity checks, typecheck, production build
and the two English/Chinese production CSP browser checks. The versioned bilingual
release highlights and generated documents are included. An attempted
`node --test tests/scripts/releaseNotes.node.cjs` selected a nonexistent file and
exited 1 without running tests; the actual existing generator test entry point is
`npx vitest run tests/scripts/generateUpdaterManifest.test.ts`.
That actual entry point passed all 8 tests. Independent read-only review accepted
the synchronized metadata, bilingual highlights, generated documents and explicit
acceptance limits. No publication policy or workflow was modified.

The first PR CI (`42457475491`, head `03132ba1`) passed 3234 frontend tests and
failed two pinned package-version expectations that still named 0.26.0 and its
date. Their literal target is synchronized to this authorized RC; equality,
all six identity fields, Tauri major/minor admission, strict installed-version
matching and the historical 0.18.0 roundtrip guard are unchanged. The original
failure is retained in `.audit/v027-stage12-ci-failure.md` and GitHub job logs.

Stage10 proved actual product-backend-to-Codex-0.167 single synthetic original
deletion. Its B001 unknown was retained without replay. Stage11 did not prove
permanent-delete document IPC or the visible UI: its original own instance and
source leases remain unresolved after denied lifecycle observation. Do not repeat
the denied observation, kill an unproven process, touch the default WebView data
directory or open another window. That UI gate requires an authorized resolution
of the retained lifecycle and a verified isolated data directory.

Claude/OpenCode delete stays disabled. Their observed writer/service-ownership
gaps are not fixed by confirmation. Full dynamic descendants and partial native
failures, Unix, installed-app acceptance, D20/native All, and real signed upgrade
and return roundtrip remain unverified. A signed candidate is required before
the latter roundtrip can occur; a candidate artifact alone is not that acceptance.

Before publication: reviewed main integration, successful current exact-main
required CI and disclosed native coverage, validated original coverage bytes and
three same-run platform updater signatures, unused version/tag/draft, and an
explicit decision on the outstanding product acceptance boundaries. Preserve
stable Latest and verify the exact RC tag manifest and payloads after publication.
No secret, real account, real session or installed-app access is requested here.
