# Release process

The user-authorized recovery path promotes one signed platform set from **protected main**. Feature revisions remain candidates until they are merged and the actual merged main SHA passes full CI under the required-checks policy, with a validated disclosure of unavailable native checks. Required CI success does not prove original unfiltered native All or real installation/return roundtrip acceptance. Package metadata changes and manual dispatch can start the release workflow; tags and feature branches cannot promote. The release workflow waits for that exact main SHA's ordinary CI to finish; failed CI and changed main remain blocking.

`release-preflight.mjs` enforces the same `release-policy.mjs` used by fixture tests:

- The workflow SHA equals the current protected main SHA.
- `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` declare the same stable version.
- The latest ordinary `ci.yml` push run and its current attempt for that main SHA are completed and successful, including every job and exactly one Frontend checks, Rust checks and Disposable roundtrip compile-only policy job. Naturally executed failures remain blocking; diagnostic runs and local fixtures cannot substitute.
- Exactly one nonexpired `windows-native-coverage-<SHA>-<CI run>-<attempt>` artifact belongs to that same main CI run. Its downloaded, bounded `windows-native-coverage.json` and original inventory/execution logs pass `windows-native-validation.mjs` under policy `required-checks-and-disclosed-host-unverified-v1`. The artifact ID, name, run, attempt and source binding must agree with a fresh authenticated resolution.
- No tag, release or draft already uses the version.
- Exactly three immutable candidate artifacts, named with the source SHA and platform, belong to this release workflow run and main SHA.

The coverage resolver only outputs the selected CI run/attempt and artifact ID/name; it does not report promotion success. The workflow downloads that exact artifact with `actions/download-artifact`. Complete preflight independently fetches the same exact artifact archive through the authenticated GitHub API and rereads protected main, the latest CI attempt, jobs and artifact metadata. It follows only bounded safe HTTPS download redirects, never forwarding the authorization token; when supplied, the API's known SHA-256 archive digest must match. The classic ZIP reader rejects unsafe paths, duplicate/nonregular entries, encrypted/ZIP64/multidisk formats, overlaps, unknown entries, bad CRCs and excessive compressed/expanded sizes. Preflight stages only the expected report and log bytes into its own checked temporary directory, then validates their actual contents. The local report and every referenced log intended for publication must byte-match the authenticated archive. Matching metadata alone cannot make caller-created JSON or logs authoritative. The same checks run before signing/building, before platform-artifact download, and immediately before publishing. A newer main SHA, newer CI run/attempt, missing artifact or unfinished/failed CI makes the previous resolution ineligible. No skip-CI or Boolean waiver exists. Global release concurrency serializes promotions.

The Rust normal suite includes every default test except the exact 18 source-reviewed manager tests when a successful host query observes an external Windows Job. Newly discovered tests remain included. Original ignored workers stay ignored; unavailable specialists are recorded as unverified, never passed or ignored. The unfiltered and selected inventories, exact exclusions, original ignored set, result counts and process exits must reconcile against the attached original logs. Missing/duplicate policy names, unintended substring matches, arbitrary exclusions, query errors, missing logs, zero eligible library tests and executed failures block promotion. WebView/runtime tests remain selected; Job containment does not authorize further exclusions.

The user's authorization allows the exact observed-unavailable specialist scope to remain nonblocking with disclosure; it does not authorize disabling historical installation or relaxing any runtime admission guard. Original unfiltered native `All` remains **unverified** when it was not run. The retained qualified-host `All` entry and real installation/return acceptance are separate evidence, and required CI success cannot be described as their pass.

The build job has read permissions and references only `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` from GitHub Secrets. Never read, log or copy those values. Only the release job receives `contents: write`; `actions: read` allows source/run-bound artifact and CI verification. Repository protections and client automatic-install policy remain unchanged. The retired `release.js` and npm release/OSS entry points remain refusal shims.

The release job downloads the Windows x64 NSIS installer, macOS arm64 app archive/DMG and Linux x64 AppImage from its own release run. These three platform artifacts are distinct from the coverage artifact downloaded from the selected CI run. Manifest generation rejects duplicate/missing updater coverage and cryptographically verifies every artifact signature against the public key in Tauri configuration before publishing. Both legacy Ed25519 and prehashed BLAKE2b-512 minisign signatures and their trusted-comment signatures are checked. Publication attaches the same validated coverage JSON; generated notes record its SHA-256, source/run/attempt/artifact identity, actual executed and original ignored counts, all exact unverified names, `native All: unverified` and the absence of real native roundtrip proof. Post-publication verification downloads all three updater payloads and checks their bytes/signatures again; URL availability alone is insufficient. See the [minisign verifier](https://github.com/jedisct1/minisign/blob/master/src/minisign.c) and [Tauri signature encoding](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-cli/src/helpers/updater_signature.rs).

Use Rust 1.98.1 consistently for candidate builds and current ordinary CI. Keep strict Clippy, the normal suite's original assertions and the separate qualified native entries. Version changes also synchronize both lockfile root identities and the changelog before merging.

## Existing draft recovery and rollback

Ordinary promotion refuses existing tags, releases and drafts. The 0.18.1 recovery transaction recognizes only unpublished draft `406663556` for old source `5ed35db9a560e093a91a5ef32a1eb6dd171f27fd`, with the exact five recorded asset IDs, names, sizes and SHA-256 digests. These assets are not merged-main outputs. Read-only `--prepare-draft-recovery` permits candidate building around that precise conflict while retaining protected-main, successful current CI and authenticated coverage checks; it never authorizes publication or tag movement.

The existing release job performs the authorized transaction only after all three new same-run platform artifacts pass signature validation:

1. Capture the complete original release JSON and verify the version tag is absent. Download all five original assets and verify their exact byte counts and SHA-256 digests against the reviewed inventory.
2. Upload `cc-desk-old-draft-backup-<main SHA>-<release run>-<attempt>` with 90-day retention. Independently download that exact immutable artifact; recheck its authenticated source/run identity, metadata hash and all five original asset hashes. Recheck current main/CI/coverage and unchanged draft metadata before mutation.
3. Keep draft ID `406663556` and `tag_name: v0.18.1`; change `target_commitish` only to the actual current protected-main SHA. No archival release tag is created and no Git tag is moved. Rename the five original assets to `preserved-5ed35db9-<original asset ID>.bin` with an explicit previous-unpublished-candidate label, preserving their IDs, bytes and API digests. These backups remain separate from current installers.
4. Bind the seven new platform files, `latest.json` and authenticated coverage report to the current main SHA, release run/attempt and complete inventory digest. Upload the nine new assets to the same unpublished draft without replacing or deleting any existing asset. This leaves fourteen assets: nine current and five preserved originals. The updater manifest references only the current signed payloads.
5. Verify the exact original draft ID, source/run marker, nine new plus five preserved asset inventory and all API sizes/digests. Download every new asset, compare its bytes with the validated same-run artifacts, and verify all three updater signatures. Recheck current main, latest CI run/attempt/jobs, authenticated coverage and the version tag before publishing the original draft as Latest. Verify the published tag resolves to the recorded main SHA and verify the public Latest manifest and updater payload signatures.

Backup failure leaves the original draft unchanged. Mutation failures leave the draft unpublished; preserve its source/run identity and reconcile authenticated reads before any further action. Unknown PATCH/POST acknowledgements are resolved by exact metadata, asset IDs and bytes; they are never automatically replayed. The five original asset IDs and bytes are retained even beyond artifact expiry. Recovery can use the independently checked metadata snapshot and original bytes; restoring the previous target SHA may need additional workflow permission if it changes `.github/workflows` relative to the default branch. Do not promise an automatic exact source rollback with the ordinary token, introduce an archival tag to change permission checks, or delete conflicting assets. Deletion of the five original IDs requires separate concrete authorization after verified backups and current staging evidence exist.

Every preparation metadata PATCH explicitly includes `tag_name`, exact current
`target_commitish`, title, disclosure and `draft: true`. Its concrete HTTP status
is retained and logged. A mismatched authenticated read reports the changed
snapshot fields; disclosure and asset metadata are represented by hashes.
An HTTP 200 followed by a tag mismatch is not classified as a permission error.
The exact original draft ID remains a 0.18.1 conflict even if GitHub normalizes
its tag name to the observed `untagged-dce9f75805136bcd2e47`.

An explicit `release-draft-transaction.mjs repair-metadata --directory <verified
backup directory> --backup-artifact-id <exact immutable artifact ID>` repairs
only that reviewed tag normalization on an otherwise completely prepared,
unpublished original draft. It requires the existing workflow authentication,
same protected-main SHA, same release run/attempt, fresh successful exact-source
CI/coverage, all three same-run candidate artifacts and the exact preparation
inventory/disclosure marker. It independently checks the original same-run
backup metadata and all five original bytes, then compares every stable
snapshot/asset field with that backup. Extra uploads, changed assets, source,
run, attempt, marker, title or disclosure are rejected before mutation. The
repair changes no asset and creates no Git tag. A complete state observed after
an unknown acknowledgement is accepted without repeating the write.

This explicit entry is not automatically invoked by ordinary publication or
candidate preparation. It refuses an earlier preparation after main advances
or a new release run/attempt begins; old signed artifacts or CI cannot become
evidence for a new source. Such a transition needs its own reviewed recovery
plan and newly validated current-source outputs. This change alone does not
authorize rerunning or publishing the previously failed release.

After public publication, do not delete/recreate or retarget an existing tag. A failed post-publication check is a failed release workflow requiring investigation; it must not be described as a verified updater channel. Prefer a separately verified new version for repair. No workflow can make the branch read, external release mutation and later verification atomic, so preserve the recorded source/run binding and investigate any concurrent external mutation.

## Acceptance boundaries

D20 real Claude Code/Codex CLI target certification remains separate and BLOCKED unless actual source-bound evidence exists. Local Node fixtures, isolated native diagnostics and runner registration are not hosted CI or D20 certification. Publication gating does not enable automatic client installation. Historical installation has a separate Windows x64 capability policy for the nine exact reviewed official packages; its normal coordinator and restricted return path retain all runtime admission and recovery checks. See [historical installation and return](historical-version-preparation.md) for supported scope and remaining native acceptance conditions. A complete-function release cannot describe a permanently disabled historical installation entry as delivered.

Run the blocking release fixtures with:

`node --test tests/scripts/releaseRecovery.node.cjs tests/scripts/nativeReleasePolicy.node.mjs tests/scripts/releaseCoverage.node.mjs tests/scripts/releaseDraftRecovery.node.mjs`
