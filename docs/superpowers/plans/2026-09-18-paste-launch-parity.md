# Paste Launch Parity Implementation Plan

> For agentic workers: execute task-by-task with test-driven development and verification before completion.

**Goal:** Compare native Claude started directly with Claude started through the same Git Bash launcher used by CC Desk, without modifying production paste bytes.

**Architecture:** Keep the current production TypeScript payload builder, Rust writer and isolated UserPromptSubmit capture. Add an explicit per-case `launchMode` to select direct/native or production-shell startup using `platform::get_claude_shell`. Unknown modes fail rather than silently using direct startup.

**Tech Stack:** Node.js test runner, TypeScript transpilation, Rust/portable-pty, Windows GitHub Actions.

**Spec:** The September 18 field report and PR #9: full clipboard in Notepad, incomplete Ctrl+G draft, complete Rust frames. The current acceptance starts the executable directly, but production `PtyManager::spawn_claude` starts a shell. Neither path in hosted CI is the affected Windows 10 build 19045.

## Global constraints

- No changes to the production frontend/PTY transport, user configuration, credentials, release versions or main branch.
- Keep strict full-prompt equality and separate tab/control-character differences from large content loss.
- Do not copy user clipboard contents into fixtures or CI.
- No automatic retries of pasted data; preserve the pre-Enter submission check.
- Do not claim this is full WebView, long-lived-session, Windows Terminal or Win10 coverage.

## Task 1: Explicit launch metadata

Files: `.github/scripts/generate_paste_acceptance.cjs`, `tests/scripts/pasteAcceptance.node.cjs`.

- [x] Verify existing generator tests pass against hash-verified production sources (4 passed).
- [x] Add tests for the default launch mode, production-shell mode with identical payload bytes, and rejection of unknown modes.
- [x] Observe the new tests fail against the original generator (4 passed, 3 failed).
- [x] Add validated launch-mode metadata without changing wire/expected strings.
- [x] Verify all generator tests pass locally (7 passed). This isolated partial workspace is not a full application build.

## Task 2: Real production-shell receiver

File: `src-tauri/src/tests/paste_cli_submit.rs`.

- [x] Deserialize the launch mode as a closed enum.
- [x] Reuse `platform::get_claude_shell` for the Git Bash launch; retain direct startup as control.
- [x] Require the known CI Git Bash executable for shell cases; do not silently fall back.
- [x] Preserve isolated config, blocking hook, loopback API and strict prompt comparison.
- [x] Add mode parsing tests and metadata-only per-case results.
- [ ] Verify compilation, formatting and unit results on Windows CI; local Rust tooling is unavailable.

## Task 3: Verification gates

File: `.github/workflows/paste-cli-acceptance.yml`.

- [x] Configure both launch modes for both pinned Claude versions, reusing each job's build.
- [x] Configure explicit Windows comparator/launch unit tests before real CLI acceptance.
- [x] Configure Windows clippy and source-cleanliness checks even after strict CLI comparison fails; retain failure status.
- [x] Record actual launch mode, executable identity and Windows build.
- [x] Configure artifacts containing only per-case metrics/classification results, not submitted prompt text.
- [ ] Obtain and compare real direct vs shell outcomes. Keep PR #9 a draft.

If the content-loss failure is not reproduced, the next runtime work is clipboard/payload/Rust correlation plus non-paste-event tracing, not arbitrary transport changes. Configuration changes and passing generator tests do not establish that the field issue is fixed.
