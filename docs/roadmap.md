# CC Desk roadmap

The current product is the unified Workspace / Projects / Settings shell for real
Claude Code and Codex CLI sessions. Provider CRUD, bundled CLI installation and an
independent MCP client remain out of scope.

## Current milestone: source integrated, acceptance incomplete

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

## Remaining acceptance sequence

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

## Release track

Signed candidates and test-only installer artifacts do not grant promotion.
`scripts/release-policy.mjs` stays fail-closed; no version bump, tag, GitHub Release or
updater publication is part of this UX work. Future public promotion needs a separate
explicit decision, immutable artifact/evidence identity and rollback design.

After these gates and owner review, consider selected-version CLI compatibility,
any separate Legacy deprecation plan, dependency/advisory triage, and explicit
candidate-to-release promotion. None is silently bundled into this refactor.
