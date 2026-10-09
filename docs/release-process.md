# Release process

The user-authorized recovery path promotes one signed platform set from **protected main**. Feature revisions remain candidates until they are merged and the actual merged main SHA passes full CI. Package metadata changes and manual dispatch can start the release workflow; tags and feature branches cannot promote.

`release-preflight.mjs` enforces the same `release-policy.mjs` used by fixture tests:

- The workflow SHA equals the current protected main SHA.
- `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` declare the same stable version.
- The latest ordinary `ci.yml` push run for that main SHA is completed and successful, including every job and exactly one Frontend checks and Rust checks job. Failed native hosted Rust checks keep this gate red; diagnostic runs and local fixtures cannot substitute.
- No tag, release or draft already uses the version.
- Exactly three immutable candidate artifacts, named with the source SHA and platform, belong to this release workflow run and main SHA.

Preflight runs before signing/building, again before download, and immediately before publishing. If CI has not completed yet, the gate fails; after CI succeeds, dispatch the workflow on the same still-current main SHA. No automatic retry bypasses the gate. Global release concurrency serializes promotions; main advancing makes an older run ineligible.

The build job has read permissions and references only `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` from GitHub Secrets. Never read, log or copy those values. Only the release job receives `contents: write`; `actions: read` allows source/run-bound artifact and CI verification. Repository protections and client automatic-install policy remain unchanged. The retired `release.js` and npm release/OSS entry points remain refusal shims.

The release job downloads the Windows x64 NSIS installer, macOS arm64 app archive/DMG and Linux x64 AppImage from its own run. Manifest generation rejects duplicate/missing updater coverage and cryptographically verifies every artifact signature against the public key in Tauri configuration before publishing. Both legacy Ed25519 and prehashed BLAKE2b-512 minisign signatures and their trusted-comment signatures are checked. Post-publication verification downloads all three updater payloads and checks their bytes/signatures again; URL availability alone is insufficient. See the [minisign verifier](https://github.com/jedisct1/minisign/blob/master/src/minisign.c) and [Tauri signature encoding](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-cli/src/helpers/updater_signature.rs).

Use Rust 1.98.1 consistently for candidate builds and current ordinary CI. Keep strict Clippy and native process gates. Version changes also synchronize both lockfile root identities and the changelog before merging.

## Existing draft recovery and rollback

Ordinary promotion refuses existing tags, releases and drafts. Draft `406663556` for old source `5ed35db9a560e093a91a5ef32a1eb6dd171f27fd` and its five assets must remain untouched by this patch or an ordinary run; they are not merged-main outputs.

A separate reviewed recovery transaction is required before replacing that draft: capture the full release/tag metadata and every original asset; record name, asset ID, byte count and SHA-256; download the assets to durable backup; independently verify every backup hash and restoration inventory; and bind a new complete three-platform set to the actual merged main SHA and one successful workflow run. If the backup or any new gate fails, stop with the draft unchanged. Never mix original assets with new outputs and never move an existing tag. The transaction must define draft-only staging, exact publication commit/tag, failure recovery and hash-verified restoration before any old asset can be removed. This document does not execute or approve that transaction.

After public publication, do not delete/recreate or retarget an existing tag. A failed post-publication check is a failed release workflow requiring investigation; it must not be described as a verified updater channel. Prefer a separately verified new version for repair. No workflow can make the branch read, external release mutation and later verification atomic, so preserve the recorded source/run binding and investigate any concurrent external mutation.

## Acceptance boundaries

D20 real Claude Code/Codex CLI target certification remains separate and BLOCKED unless actual source-bound evidence exists. Local Node fixtures, isolated native diagnostics and runner registration are not hosted CI or D20 certification. Publication gating does not enable automatic client installation. Historical installation has a separate Windows x64 capability policy for the nine exact reviewed official packages; its normal coordinator and restricted return path retain all runtime admission and recovery checks. See [historical installation and return](historical-version-preparation.md) for supported scope and remaining native acceptance conditions. A complete-function release cannot describe a permanently disabled historical installation entry as delivered.

Run the blocking release fixtures with:

`node --test tests/scripts/releaseRecovery.node.cjs tests/scripts/nativeReleasePolicy.node.mjs`
