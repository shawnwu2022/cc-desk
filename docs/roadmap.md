# CC Desk roadmap

The current product is the unified Workspace / Projects / Settings shell for real
Claude Code, Codex CLI and opencode sessions. Provider CRUD, bundled CLI installation and an
independent MCP client remain out of scope.

## Current checkpoint: v0.24.0 published (2026-10-22)

Stable v0.24.0 is bound to main/tag
`24316ad9069df76f2de669fe7b17eeca2a2228be`:
[ordinary CI 39718592492/1](https://github.com/shawnwu2022/cc-desk/actions/runs/39718592492)
passed all 23 jobs and
[Release 39718592542/1](https://github.com/shawnwu2022/cc-desk/actions/runs/39718592542)
passed all six jobs. The release has nine assets; publication verified the actual
download bytes and three platform updater signatures. These results supersede the
old source/visual publication block below, without certifying every target behavior.

Already delivered: active-session filtering; OpenCode creation, exact resume,
WAL-consistent history, native rename and activity observation; default Claude-only
enablement that preserves explicit user choices; original compact icon brightness
and immediate provider-list filtering; and Claude/Codex identity and focus fixes.
Do not list these as unimplemented next-version features.

The ordinary Rust inventory executed 1,657 tests and retained 35 original ignores;
22 original tests remain host-unverified (18 Job-free and four elevated-host cases).
This is not a `nativeAll`, logged-in D20, installed WebView or screen-reader PASS.
Rendered browser and installer CI evidence also remains distinct from those targets.
The source-bound [runtime baseline](ci-runtime-v0240-baseline.json) and
[CI analysis](ci-performance.md#v0240-observed-baseline-2026-10-22) retain the
actual observations and comparison limits.

The approved next-version implementation is local only: fixture reliability,
dependency reachability/upgrades, measured CI work and explicit target acceptance
gaps, plus a focus-aware sidebar shortcut. No push, merge, version bump or release
is authorized by that implementation scope. The shortcut may warrant a feature
version (0.25.0); version selection remains a separate release decision.

Retained work is reviewed by source, not blindly merged:

| Material | Current disposition |
| --- | --- |
| Draft [PR43](https://github.com/shawnwu2022/cc-desk/pull/43), `56569595` | Bounded calibration idea reused in the local candidate; old weights/runner not adopted; retain its independent history until measured decisions are complete. |
| `evidence/pr37-reviewed-baselines`, `728ae1fe` | Nine historical PNG/source-map files remain historical evidence; do not copy them over current source-bound visual baselines without review. |
| `test/ci-upload-recovery-oct19`, `12b259b6` | Retain its unique advisory workflow while checking usefulness; current main already has immutable upload recovery. No automatic merge or deletion. |
| Active-session-filter work and provider-defaults manual-test checkout | Delivered behavior is already in main. Preserve the user's live testing checkout/services and any unique local planning material. |

Current candidate calibration remains advisory and disabled by default; exact hosted
measurements and target acceptance are still required before claiming a release-ready
performance result. No old source test, screenshot or timing becomes current evidence
solely because its branch has been retained.

## Historical milestone: source integrated, acceptance incomplete (2026-10-01)

Tasks 1–22 and 24 have source/host checkpoints. Task 23 has a reviewed fixture and
regression harness checkpoint, but remains **BLOCKED_VISUAL**. Task 25 is local
validation and evidence preparation, not a declaration of full freeze or acceptance.
The current evidence matrix is [U01–U10](superpowers/execution/U01-U10.md).

Implemented boundaries include:

- One shell and project/session tree, mixing both CLIs; duplicated Native and Legacy product pages are retired
- Authenticated Native lifecycle, exact attempt/source identity, ordered input and bounded output/ACK; Legacy compatibility remains isolated
- Shared New/Resume flows, explicit consequential confirmations and safe error feedback
- Project registration/visibility guards and six read-only scoped resource categories
- Seven Settings sections, independent GUI/terminal preferences and one serialized compatible settings writer
- Additive display/archive/launch-preference metadata; Desk rename never sends CLI input
- Responsive/accessibility contracts and deterministic fixtures using production components
- Migration/adversarial tests plus 50-project, 100-session and 30-descriptor host stress

These source tests do not certify real disk migration, 30 concurrent real CLI
processes, rendered geometry, screen readers or installed application behavior.
Earlier Native v3 D21 Windows runtime evidence remains valid only for its recorded
historical target; it is not an installed-package PASS for the unified UX head.

## Historical acceptance sequence (2026-10-01)

1. Complete local full-suite/typecheck/build and independent full-branch review
2. Resolve the Task 23 rendering block through an explicitly authorized final workflow/target; retain the genuine missing-baseline failure
3. Inspect all 13 candidate PNGs, repair demonstrated defects, commit only reviewed baselines, and obtain a no-diff/geometry/interaction PASS
4. Bind final ordinary CI and Windows test-package evidence to the exact source and tested PR merge commit
5. Validate old data, mixed runtime sessions, real creation/resume and operations, all GUI/terminal combinations, CJK/emoji and Windows 1024×640 at 100%/125%/150%; also macOS/Linux functional rendering
6. Record independent review, installer hash, install/reinstall/relocation and manual acceptance results before claiming U10 complete

The visual workflow has no development push trigger. A new workflow that exists only
on this feature branch cannot receive `workflow_dispatch` until present on the default
branch. Its PR trigger is prepared for the eventual stacked PR into
`feat/native-cli-finalization`; opening that PR also triggers ordinary CI. Because
pixels remain unavailable locally, that would change the original visual-freeze-before-CI
sequence. The owner approved that final draft/CI sequence on 2026-10-01, with independent
review before the parent performs external actions. Actual runs and pixel acceptance
remain pending. No workflow publication to the default branch is included.

## External certification: D20

**BLOCKED_EXTERNAL_TARGET** until an authorized real target supplies the separate
Claude Code and Codex CLI four-cell matrix: Desk/system terminal × observer off/on.
It requires isolated roots, actual binary identities/hashes, raw hook evidence and
host-payload provenance. Synthetic hooks, model echo, screen scraping, mocked host
receipts and screenshots cannot substitute for missing cells.

## Test and CI performance follow-up

The ordered [performance TODO](ci-performance.md#follow-up-todo-2026-10-11) is the
single checklist for the remaining optimization batches. Local iteration groups,
agent-guide loading and same-job debug frontend reuse have scoped evidence;
actual metrics artifact verification, a controlled hosted comparison and Rust
timing calibration remain prerequisites for further shard/profile decisions. Net CI
savings for the latest worktree remain unverified. The independent acceptance
sequence and D20 above continue to require their own target evidence.

## Historical release track and continuing promotion boundary

Signed candidates and test-only installer artifacts do not grant promotion.
`scripts/release-policy.mjs` stays fail-closed; no version bump, tag, GitHub Release or
updater publication is part of this UX work. Future public promotion needs a separate
explicit decision, immutable artifact/evidence identity and rollback design.

After these gates and owner review, consider selected-version CLI compatibility,
any separate Legacy deprecation plan, dependency/advisory triage, and explicit
candidate-to-release promotion. None is silently bundled into this refactor.
