# Release process

## Current policy: signed candidates only

Publishing is intentionally disabled while Native CLI v3 real-CLI certification and promotion policy remain incomplete.

Two repository controls enforce this:

1. `scripts/release-policy.mjs` returns `false` for every publish decision.
2. `.github/workflows/release.yml` builds signed candidate packages and uploads workflow artifacts, but contains no GitHub Release publishing job.

Do not describe a candidate artifact as a published release.

## Candidate workflow

The workflow can run on:

- `main` when `package.json` changes;
- a `v*` tag;
- manual `workflow_dispatch`.

It builds signed packages for Windows, macOS, and Linux.

Expected candidate artifacts:

| Platform | Candidate |
|---|---|
| Windows x64 | NSIS `.exe` + signature |
| macOS arm64 | `.dmg`, `.app.tar.gz`, signature |
| Linux x64 | `.AppImage` + signature |

Candidate artifact names include the commit SHA so the package can be tied to the exact source revision.

## Required secrets

Candidate signing uses:

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

The private key must never be committed, logged, copied into documentation, or included in support bundles.

## Version consistency

When a version change is intentionally prepared, keep these files consistent:

- `package.json`
- `package-lock.json`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/tauri.conf.json`
- `CHANGELOG.md`

A version bump does **not** authorize publishing.

## Native CLI promotion gate

Before a public release path is re-enabled, the project must have an explicit promotion design that answers:

- which exact source commit is being promoted;
- which signed candidate artifacts are immutable inputs;
- what code-side CI is required;
- what D20 real Claude Code / Codex CLI evidence is required;
- which target OSes/architectures are covered;
- how updater manifests are generated and verified;
- how a failed or revoked promotion is rolled back;
- who performs the explicit approval.

Layer A/B CI alone cannot satisfy the D20 Layer-C requirement.

## Current D20 status

The real-CLI harness and target command are implemented:

`scripts/native-cli/run-real-cli-certification.mjs`

A PASS requires real Claude Code and Codex CLI execution in an explicitly authorized isolated target environment.

If either product, account, binary identity, hook schema, lane driver, or evidence binding is unavailable/unverifiable, the result remains BLOCKED.

## Future publishing workflow

A future publishing workflow should be separate from ordinary build/test CI and separate from candidate creation.

It should consume an already verified immutable candidate set and require explicit approval. It must not rebuild different binaries during promotion.

Until that workflow exists, publishing stays disabled.

## Optional mirrors

Legacy scripts may still contain optional mirror helpers for maintainers. They are not the default release channel and must not be treated as an alternative way to bypass the promotion gate.

## Rollback

Because publishing is currently disabled, rollback applies to candidate artifacts and branches rather than a production channel.

Once public promotion is implemented, the rollback procedure must be defined together with updater-channel semantics; do not reuse the historical delete-and-republish procedure without a reviewed design.
