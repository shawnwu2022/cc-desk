# CC Desk roadmap

This roadmap reflects the Native CLI v3 direction. Older plans for Provider CRUD, bundled CLI installation, or an independent MCP client are superseded.

## Current milestone — Native CLI v3 code-side completion

### Completed

| Area | Status |
|---|---|
| Native profile/workspace storage and compatibility boundary | Done |
| Independent project registration and authenticated document bridge | Done |
| Scoped read-only native projections | Done |
| Owned launch lifecycle and exact run identity | Done |
| Optional observer isolation | Done |
| Bounded output transport and ACK protocol | Done |
| Ordered/staged input with no-replay failure semantics | Done |
| Clipboard/keyboard/IME arbitration | Done |
| Terminal protocol provenance routing | Done |
| Native Claude/Codex workbench integration | Done |
| New/resume/session-ID/raw-argv launch entry | Done |
| Mixed-version rollback safety | Done |
| Log/DOM/IPC security hardening | Done |
| Low-resource fairness and fault stress | Done |
| Tested Windows installed ConPTY runtime behavior | Done for the tested target |

Detailed execution evidence is recorded under `docs/superpowers/execution/`.

## External certification gate — D20

**Status: BLOCKED pending an authorized real target environment.**

The harness and target-machine command are implemented. A real PASS requires both Claude Code and Codex CLI to run the complete four-cell matrix with:

- an explicitly authorized test account;
- isolated HOME/config/project/report roots;
- the actual CLI binary identity and SHA-256;
- CC Desk observer off/on;
- system-terminal observer off/on;
- raw hook evidence and host-payload provenance.

Unavailable or unverifiable cells remain BLOCKED. Synthetic hooks, screen scraping, model echo, and writer-success receipts are not substitutes.

## Finalization before merge/release

Code-side closeout consists of:

1. keep the Native CLI workspace visible as a first-class entry;
2. keep product/developer/release documentation aligned with the implemented boundary;
3. maintain regression gates against deleted Provider/mutating-resource/legacy-PTY fallbacks;
4. run one final unified CI on the finalization PR;
5. perform owner review of the stacked diff.

No release is implied by these steps.

## Release track

Current release automation is candidate-only.

Before public promotion is implemented:

- `scripts/release-policy.mjs` must continue to deny publishing;
- the workflow may build signed candidates and upload artifacts;
- no workflow may silently create or update a GitHub Release;
- an explicit promotion design must define required evidence, immutable artifact identity, updater manifest verification, and rollback behavior.

## Post-v3 roadmap

After D20 and owner approval:

### UX consolidation

- native workbench as the primary discoverable path;
- bilingual labels and error-code explanations;
- project/profile onboarding without exposing secrets;
- clear legacy-workspace migration/deprecation messaging.

### Compatibility

- selected-version Claude Code regression matrix;
- selected-version Codex CLI regression matrix;
- terminal/editor/authentication edge cases on real OS targets;
- future CLI-version drift handling without hardcoding internal CLI behavior.

### Operations

- explicit release promotion workflow;
- dependency/advisory triage as a separate reviewed change;
- signed-candidate retention and reproducibility documentation;
- support bundles that retain metadata but never user prompts/secrets.

## Non-goals

CC Desk will not become:

- a Provider/API-key manager;
- a bundled installer for Claude Code or Codex CLI;
- a native-config editor that competes with the CLI;
- an independent MCP runtime;
- a structured chat client that replaces the terminal.
