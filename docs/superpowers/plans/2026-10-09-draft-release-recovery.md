# 0.18.1 draft recovery implementation plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task. Preserve the user's authorization to complete the normal merge and release without another approval loop.

**Goal:** Publish the complete current-main 0.18.1 platform set while preserving old draft 406663556 and every old asset.

**Architecture:** Read-only candidate preparation may recognize only the exact reviewed old draft while retaining current protected-main, same-SHA successful CI and authenticated coverage checks. After all three new platform artifacts have been verified, download and independently verify a durable backup, update the original draft to the actual current protected-main SHA, preserve the five original asset IDs/bytes under labelled .bin backup names, and stage the nine current assets on that same draft, verify it against the source/run-bound local artifacts, then publish and verify Latest.

**Tech Stack:** Existing GitHub Actions release workflow, Node built-ins, GitHub release API, existing Tauri updater signature verifier.

**Spec:** User delegation and `docs/release-process.md` existing-draft recovery requirements.

## Global constraints

- Keep version 0.18.1; no new branch, force push, credential, runner, branch-protection change or Job escape.
- Preserve every production historical/runtime guard and all nine reviewed historical packages.
- Keep the exact 18 host-dependent exclusions unverified; all other ordinary tests execute and their failures remain blocking.
- Preserve old release ID and all asset IDs/bytes; retain the original source/body in the verified durable backup before updating the draft. Do not delete assets/releases or move a tag.
- Signing secrets remain confined to existing build-job environment references.

## Review focus

- Changed old-draft source, assets, metadata or an existing version tag must refuse recovery.
- Partial/corrupt durable backups must refuse source or asset mutation.
- Main, CI attempt or coverage changes during building/staging must refuse publication.
- An uncertain source/asset/publication response must be resolved by a read, never by blind mutation replay.
- Partial staging stays unpublished; nine current assets and five preserved originals are checked as separate exact inventories.

### Task 1: Exact read-only candidate recovery admission

**Files:** `scripts/release-policy.mjs`, `scripts/release-preflight.mjs`, new `scripts/release-draft-recovery.mjs`, new `tests/scripts/releaseDraftRecovery.node.mjs`.

- [ ] Add failing tests for the exact old draft and refusal of every metadata/asset/tag/source/CI mismatch.
- [ ] Separate source/CI checks from ordinary unused-version checks without changing ordinary promotion refusal.
- [ ] Add a narrowly scoped read-only recovery-preparation mode that validates the authenticated old draft identity and the normal downloaded coverage archive.
- [ ] Run new recovery tests and all existing release/coverage/policy tests; preserve default draft/tag conflict tests.

### Task 2: Preserve, stage and publish in the existing workflow

**Files:** `scripts/release-draft-recovery.mjs`, `.github/workflows/release.yml`, `docs/release-process.md`, `AGENTS.md`, recovery tests.

- [ ] Add failing tests for backup hash/metadata mismatch, partial staging, unknown receipts and changed current-source checks.
- [ ] Back up exact old metadata and five assets; upload then download the immutable same-run backup artifact and independently verify its complete hashes before mutation.
- [ ] Retarget only the original draft to actual current main SHA, rename original assets without deletion, and bind current source/run notes; reread exact identity and inventory.
- [ ] Rerun exact prepared-draft preflight, stage all nine current assets on the original draft, verify downloaded bytes/hashes/signatures against the three same-run artifacts and coverage, recheck current main/CI, and publish the exact staged ID.
- [ ] Document preserved original IDs/bytes, durable backup and source-rollback permission limits; retain incomplete staging unpublished for investigation. No archive tag or automatic deletion.
- [ ] Run complete frontend/release policy tests and independent review, expected-SHA nonforce update, actual CI, ordinary main merge and source-bound release verification.
