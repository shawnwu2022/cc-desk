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

## Reproducible 0.18.0 test compiler

Ordinary CI and Windows test packaging (including formatting) use Rust **1.98.1**. This is the exact compiler that passed source `444c2df` in CI `36861864297` and package `36861864197`. A later floating `stable` download changed to 1.99.0 during the same repair batch and introduced `Atomic::fetch_update` deprecation errors under the unchanged `-D warnings` gate.

The pin keeps strict Clippy and the declared Rust 1.89 MSRV unchanged; it does not suppress warnings or modify atomic ownership/transport logic. Compiler upgrades require an intentional validated change. The [official action supports exact version refs](https://github.com/dtolnay/rust-toolchain), and its `1.98.1` action definition explicitly selects that compiler. Other standalone harness workflows and public-promotion policy are outside this narrow repair.

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
