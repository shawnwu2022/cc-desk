# Native CLI Release Handoff

This document is the maintainer handoff for D28-D31.

## Code-side completion scope

The repository now contains:

- a strict machine acceptance gate: `scripts/native-cli/verify-acceptance.mjs`;
- a repository-owned release target plan: `docs/testing/native-cli-release-targets.json`;
- immutable candidate identity generation: `scripts/native-cli/candidate-manifest.mjs`;
- same-candidate promotion verification: `scripts/native-cli/verify-promotion.mjs`;
- pinned/stable CLI identity canary recording: `scripts/native-cli/canary-identity.mjs`;
- candidate-only signed build workflow;
- explicit acceptance-gate workflow;
- pinned/stable canary workflow;
- protected same-candidate promotion workflow;
- release-policy tests for fail-closed promotion;
- legacy direct-release command disabled except the optional post-release OSS mirror path.

## Invariants maintainers must preserve

1. **No rebuild during promotion.** Promotion must publish the candidate bytes that were accepted.
2. **No implicit latest evidence.** Candidate and acceptance workflow run IDs are explicit inputs; the candidate run must be a successful `Signed candidate packages` run whose `head_sha` matches the candidate manifest.
3. **No A/B-to-C/D promotion.** The candidate commit's target plan declares allowed evidence layers; unit, PTY, or WebView evidence cannot fill a required real-CLI or installed-package record.
4. **No evidence-owned scope.** Acceptance evidence repeats the target plan only for readability; the gate trusts the plan checked out from the exact candidate commit.
5. **No fake N/A.** N/A requires a reason and evidence whose file hash is recomputed by the gate.
6. **No partial target certification.** Missing required case/subcase evidence fails the gate.
7. **No automatic stable release from source changes or tags.**
8. **No credential collection by CC Desk.** Real-CLI testing uses explicitly authorized isolated accounts and roots.
9. **No canary-as-certification.** Canary records version/binary identity and always remains NOT_CERTIFIED or BLOCKED.
10. **No direct legacy release.** Stable publishing is only through the protected promotion environment.
11. **Any build-input change creates a new candidate.**

## GitHub configuration required before a real promotion

Create/protect the GitHub Environment:

`release-promotion`

Recommended protection:

- required maintainer reviewers;
- no self-bypass where organization policy permits;
- restrict deployment branches to the protected release source;
- retain Actions audit history.

Configure updater signing secrets:

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

Configure canary pinned-version variables when pinned canaries should run:

- `CLAUDE_CANARY_PINNED_VERSION`
- `CODEX_CANARY_PINNED_VERSION`

## Real-CLI certification remains external evidence

The D20 harness intentionally refuses to use production credentials or arbitrary user configuration.

A real certification still requires:

- isolated Claude Code and Codex CLI binaries/roots;
- explicitly authorized test accounts;
- CC Desk and system-terminal lanes;
- observer on/off cells;
- raw UserPromptSubmit evidence;
- binary SHA-256, fixture identity, cwd/session/turn provenance.

Until those records exist for the declared target combinations, their required cases remain BLOCKED and D28 must prevent promotion.

## Release sequence

1. Freeze source and release metadata.
2. Change `docs/testing/native-cli-release-targets.json` to a reviewed `READY` target plan before building the candidate.
3. Run **Signed candidate packages**.
4. Preserve the candidate run ID and candidate manifest.
5. Install those exact packages on declared targets and collect acceptance evidence files plus `acceptance-manifest.json`.
6. Run **Native CLI acceptance gate** with both the candidate run ID and explicit evidence run.
7. Review the gate result and target coverage.
8. Run **Promote accepted native CLI candidate** with exact candidate/evidence run IDs, commit SHA, and tag.
9. Approve the protected `release-promotion` environment.
10. Verify the published updater manifest and platform assets.
11. Retain manifests/evidence with release records.

## Rollback

A rollback does not rewrite native CLI history/configuration and does not migrate `~/.cc-box/`.

If a release must be withdrawn:

- stop promoting it as Latest;
- preserve candidate and acceptance evidence for audit;
- promote a previously accepted candidate only if its version/channel policy permits;
- otherwise build and certify a new candidate.

Never rebuild an old candidate ID with different bytes.
