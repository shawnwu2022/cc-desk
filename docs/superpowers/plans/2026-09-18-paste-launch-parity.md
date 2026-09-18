# Paste Launch Parity Implementation Plan

> Execute task-by-task with test-driven development and verification before completion.

**Goal:** Compare native Claude started directly with Claude started through the same Git Bash launcher selector used by CC Desk, without modifying production paste bytes.

**Architecture:** Keep the production TypeScript payload builder, Rust writer and isolated UserPromptSubmit capture. Add an explicit per-case `launchMode` to select direct/native or production-shell startup using `platform::get_claude_shell`. Unknown modes fail rather than silently selecting the control.

**Tech Stack:** Node.js test runner, TypeScript transpilation, Rust/portable-pty, Windows GitHub Actions.

**Spec:** September 18 field report and PR #9: full clipboard in Notepad, incomplete Ctrl+G draft, complete Rust frames. The original acceptance started the executable directly, while production `PtyManager::spawn_claude` starts a shell. Hosted CI is not the affected Windows 10 build 19045.

## Constraints

- No production frontend/PTY transport, user configuration, credential, version or main-branch changes.
- Keep strict full-prompt equality; classify tab/control differences separately from content loss.
- Synthetic input only. Do not commit business clipboard data.
- No automatic resending of paste data; retain the pre-Enter submission check.
- Do not claim full WebView, long-lived-session, Windows Terminal or Win10 coverage.

## Task 1: Explicit launch metadata

Files: `.github/scripts/generate_paste_acceptance.cjs`, `tests/scripts/pasteAcceptance.node.cjs`.

- [x] Verify original generator tests against hash-verified production sources (4 passed).
- [x] Add default-mode, identical-wire shell-mode and invalid-mode tests.
- [x] Observe 4 passed / 3 failed before implementation, then 7 passed locally.
- [x] Verify the 7 tests also pass in both Windows CI jobs at c3d099c.

## Task 2: Production-shell comparison

File: `src-tauri/src/tests/paste_cli_submit.rs`.

- [x] Deserialize launch mode as a closed enum.
- [x] Reuse `platform::get_claude_shell`; require known CI Git Bash without fallback.
- [x] Retain isolated config, blocking hook, loopback API and strict prompt equality.
- [x] Add launch-mode/path tests and metadata-only per-case results.
- [x] Compile and run all 6 Windows comparator/launch unit tests successfully.
- [x] Run 18 cases per launch mode for native Claude 2.1.268 and 2.1.274.

## Task 3: Verification gates and results

Run at c3d099c: https://github.com/shawnwu2022/cc-desk/actions/runs/35315839370

Environment: Windows Server 2022 build 20348, Git 2.55.0.windows.5, Bash 5.3.15(2). Both native executable versions were verified with --version and SHA-256.

| CLI | Launch | Cases | Exact passes | Strict failures |
|---|---|---:|---:|---:|
| 2.1.268 | direct | 18 | 6 | 12 |
| 2.1.268 | production-shell | 18 | 6 | 12 |
| 2.1.274 | direct | 18 | 6 | 12 |
| 2.1.274 | production-shell | 18 | 6 | 12 |

The per-case mismatch metrics/classifications were identical across these four combinations. Each group has 10 tab-expansion-only failures, one missing final LF, and one control-character sample difference. The reported short-frame shapes and boundary cases show only tab expansion, not the field report's tail-only or middle-loss symptom. This does not exclude environment-specific shell behavior on Win10 or the complete CC Desk event path.

- [x] Windows fmt, all 6 comparator/launch unit tests, clippy and git-diff cleanliness passed in both jobs.
- [x] Regular frontend and Linux Rust CI passed: https://github.com/shawnwu2022/cc-desk/actions/runs/35315839144
- [x] Keep strict acceptance failed; do not reinterpret it as a repaired production paste path.
- [x] Inspect actual upload logs: metadata artifacts were omitted because upload-artifact excludes hidden directories by default. A successful upload step was not proof of an artifact.
- [x] Add a regression for the exact metadata path; observe 7 passed / 1 failed, then 8 passed locally after enabling hidden-file inclusion for `.ci-claude/acceptance-*.json` only.
- [ ] Verify metadata archives actually exist in the next CI run. Do not upload the entire `.ci-claude` directory or submitted prompt contents.

## Decision / next work

Do not change production to direct native startup, rewrite ESC, insert arbitrary delays, or strip tabs based on these results. Shell-launch selection alone did not reproduce the field symptom in the tested environment.

The next implementation priority is a bounded, opt-in full CC Desk input trace: correlate normalized clipboard and Rust body with a paste transaction ID, and identify interleaved non-paste input without logging business text. These runtime diagnostics are **not implemented by this plan's commits**. Exact input validation must precede attributing the issue to ConPTY or CLI draft state. PR #9 remains an investigation draft, not a release candidate.
