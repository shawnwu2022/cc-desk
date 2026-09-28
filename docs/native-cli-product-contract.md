# Native CLI product and release contract

This document defines the maintained product boundary for the dual native CLI workspace.
It is a code/release contract, **not** a certification result.

## Product boundary

- CC Desk runs the user's real Claude Code and Codex CLI binaries through the application-owned PTY/terminal host.
- Claude Code and Codex CLI have independent profiles, project scope, run identity, input/output state and restart generations.
- A Codex-only setup must not depend on Claude configuration, Claude credentials or Git Bash.
- New native profiles do not silently override model, approval, sandbox, alternate-screen or permission policy.
- Raw argv is an argument array. CC Desk does not shell-evaluate pipes, redirections or substitutions.
- Native authentication, browser callbacks, keychain use, permissions, slash commands, editors, MCP and extensions remain owned by the CLI.
- Read-only projection/observer panels are optional. Panel failure must not stop or restart the native CLI.
- Legacy Claude Provider/settings tools remain Claude-only and must not become an implicit Codex provider or credential layer.

## Data and rollback boundary

- New workspace state is stored separately from legacy Claude-oriented files.
- Rollback or an older package writing legacy files must not revive explicit `unset` / `false` values in the new workspace.
- New workspace writes do not rewrite native Claude/Codex configuration or transcript/history files.
- Running native sessions never hot-switch transport/input policy because the application was upgraded or rolled back.

## Evidence states

The acceptance catalog contains all `NATIVE-01` through `NATIVE-64` cases.

- A = logic/type/state evidence.
- B = real OS PTY / Channel / WebView evidence.
- C = real CLI workflow evidence.
- D = installed final candidate evidence.
- A/B cannot substitute for required C/D evidence.
- `PASS`, `FAIL`, `BLOCKED`, `NOT_RUN` and `N_A` retain their literal meanings.
- `N_A` requires a structured capability/version basis bound to the exact target identity and to a hashed evidence file; a free-text reason alone cannot pass the machine gate.
- Unknown CLI versions may run, but do not inherit certification from another binary hash/version.
- Unit tests, hosted-runner smoke tests and canary probes do not by themselves certify a real CLI target.

The machine release gate requires every catalog case and catalog subcase for every declared release target.
A required `FAIL`, `BLOCKED`, `NOT_RUN`, missing record, duplicate record, identity mismatch or evidence hash mismatch rejects promotion.

## Candidate and promotion contract

1. Freeze source, lock files and version.
2. Build signed candidate packages once.
3. Compute `candidateId` from the exact source commit plus every candidate file SHA-256.
4. Install those candidate files on each declared target and collect D-layer evidence.
5. Run the machine acceptance gate against the exact candidate/package hashes. Acceptance and promotion workflows must themselves be dispatched from that exact candidate ref.
6. In the promotion workflow, complete provenance checks, candidate re-hashing, the full acceptance gate, updater generation and an exact release-asset-set check **before** any protected publishing approval.
7. Only after the verification job succeeds, enter the protected `native-release-promotion` GitHub environment for explicit maintainer approval.
8. The approved job downloads only the machine-verified staging artifact from the preceding job. **Do not rebuild.**
9. Reverify the staged candidate hashes and updater hash after the approval boundary.
10. Create a draft GitHub Release whose public asset set is exactly the verified candidate files plus `latest.json`; internal candidate/acceptance/promotion audit JSON stays in workflow artifacts rather than public release assets.
11. Download the draft assets again and require the exact asset set and every hash to match the local verified staging files.
12. Publish that already verified draft without rebuilding or replacing assets.
13. Verify the public `latest.json`, version, signatures and platform asset URLs.
14. Mark promotion complete only after the published-byte and updater checks both pass.

Any source/build-input change after candidate creation creates a new candidate and invalidates the previous promotion chain.

## CI and canary boundary

- Regular CI runs type checks, frontend tests, Rust tests/format/lint, release policy tests and machine gate negative tests.
- OS-specific core workflows continue to test transport/observer/runtime behavior.
- The Native CLI canary compares a maintainer-supplied pinned CLI version with the registry's `latest` package.
- Canary execution is credential-free, runs with disposable HOME/config/CODEX_HOME/CLAUDE_CONFIG_DIR roots, and only records installation/executable identity plus `--version` / `--help`.
- Canary reports always use `certificationStatus: NOT_RUN`; they do not create C/D evidence.

## Legacy release route

Local `scripts/release.js` publishing is intentionally disabled.
It cannot create tags, push branches or publish/edit GitHub Releases.
The only supported GitHub publishing route is the protected **Promote verified native candidate** workflow.

## Handoff / operational prerequisites

The code can be merged without pretending real certification has already happened.
Before the first promoted dual-CLI release, maintainers must:

- configure required reviewers for the GitHub environment `native-release-promotion`;
- build a candidate from the final frozen release commit; candidate builds are never triggered by the promotion tag;
- collect authorized real Claude Code and Codex CLI evidence on the declared targets;
- produce the installed-package `plan.json`, `records.json` and evidence files;
- pass the Native CLI acceptance gate;
- run promotion using the exact candidate/evidence workflow run IDs;
- retain candidate, acceptance and promotion metadata with the release evidence.

If authorized real CLI/account evidence is unavailable, the affected certification remains `BLOCKED`; code-side completion does not change that state.
