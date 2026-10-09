# Native CLI v3 execution ledger

## Current continuation point

The code-side stack is complete through D27.

Authoritative frozen head before finalization:

`6bdec882b54e59b502fa42c6b13ca487fc27e2d8`

Authoritative PR for D25–D27: #32.

D20 real Claude Code / Codex CLI Layer-C certification remains BLOCKED pending an explicitly authorized real target environment.

## Completed stages

- D01–D06: native foundation, identity, profile/workspace storage.
- D07–D10: project registration, availability/recovery, platform and boundary work.
- D11: native runtime/document bridge and owned-resource foundation.
- D12: authenticated scoped native projections.
- D13: observer isolation.
- D14–D15: output transport and owned run lifecycle.
- D16–D17: ordered input queue and staged writer receipts.
- D18: clipboard/keyboard/IME arbitration.
- D19: terminal protocol provenance.
- D20: fail-closed real-CLI certification harness and target-machine command implemented; real evidence still BLOCKED.
- D21: tested Windows installed-runtime / OS-bridge certification.
- D22: dual-CLI profile/store/tab integration.
- D23: complete native launch/recovery entry.
- D24: resource/error workbench integration.
- D25: mixed-version rollback and legacy-writeback safety.
- D26: log/DOM/IPC security boundaries.
- D27: fairness, low-resource and crash/fault stress.

Per-stage records are under this directory.

## Final verified D25–D27 head

At `6bdec882b54e59b502fa42c6b13ca487fc27e2d8`:

- CI #528 / run `36332191693`: SUCCESS.
- D14 output transport core #81 / run `36332191682`: SUCCESS.
- D13 observer isolation #183 / run `36332191725`: SUCCESS.
- frontend: 84 files / 871 tests PASS.
- Rust library: 676 passed / 0 failed / 20 ignored.
- Rust main: 6 PASS.
- launch configuration: 2 PASS / 3 ignored.
- paste transport: 8 PASS.
- `cargo fmt --check`: PASS.
- strict Clippy: PASS.
- Windows application loader: PASS.

Intermediate failures are historical evidence only and are not the final state.

## Product boundary

The forward path is the authenticated Native CLI workspace for Claude Code and Codex CLI.

New native work must not:

- fall back to legacy Claude PTY APIs;
- create a second Provider/API-key authority;
- mutate native CLI resource configuration through the projection UI;
- infer protocol provenance from byte content;
- replay ambiguous input or duplicate an uncertain launch;
- expose arbitrary native errors, secrets, paths, headers or env values to the UI.

## Compatibility boundary

Legacy Claude UI/storage remains for compatibility.

The new native workspace is independently revisioned. Legacy writes cannot mutate it or revive explicit native unset/false state. Codex never inherits legacy Claude secret values.

## Evidence boundary

Three evidence layers remain distinct:

- Layer A — unit/frontend/policy.
- Layer B — host/runtime integration.
- Layer C — real installed CLI behavior.

Only D20 target execution can supply the required Layer-C evidence.

## Finalization branch

The post-D27 finalization work is intentionally limited to:

- first-class discoverability of the native dual-CLI workspace;
- authoritative documentation convergence;
- execution-ledger completion for D22–D27;
- regression gates preventing stale product boundaries from returning;
- one final PR CI run after all edits are complete.

No merge, tag, version bump, dependency bump or release is implied by finalization.
