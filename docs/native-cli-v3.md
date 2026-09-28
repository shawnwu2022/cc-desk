# Native CLI v3 — authoritative architecture

> Status: code-side implementation through D27 is complete on the stacked native-CLI branch. D20 real Claude Code / Codex CLI Layer-C certification still requires an authorized target environment and must remain BLOCKED until that evidence exists.

## Product boundary

CC Desk is a desktop workspace for **Claude Code and Codex CLI**. The native path runs the real CLI process in an owned PTY and keeps the CLI responsible for its own interaction model, commands, permissions, authentication, MCP/runtime behavior, skills/agents/plugins, editors, and future CLI features.

CC Desk adds the host capabilities that a normal terminal does not provide well:

- multiple projects and concurrent tabs;
- stable project/profile/run identity;
- exact launch/recovery semantics;
- authenticated read-only native-resource projection;
- bounded ordered input and output transport;
- optional, isolated observer metadata;
- restart/recovery controls without replaying ambiguous writes.

CC Desk does **not** own Provider/API-key configuration. Provider/configuration switching belongs to the native CLI or external tools such as cc-switch. The native resource panels are projection-only.

## Two execution paths

The repository currently contains:

1. **Legacy Claude workspace** — the existing Claude-specific UI and compatibility storage.
2. **Native CLI workspace** — the authenticated Claude/Codex runtime introduced by D01–D27.

New dual-CLI work must use the native path. It must not fall back to legacy `ptySpawn`, `ptyInput`, `ptyKill`, legacy Claude roots, or deleted Provider/mutating resource APIs.

The native entry is exposed from the title bar and the welcome screen. Native startup is intentionally independent of the legacy Claude startup gate.

## Native launch identity

A native tab freezes:

```text
cli
+ projectId / selected project path
+ profileId / profileRevision
+ requestId
+ runId
+ generation
+ launch action
```

Supported launch actions are:

- new session;
- native resume picker;
- explicit native session ID;
- exact raw argv as JSON `string[]`.

Raw argv is not shell-split. An uncertain launch result keeps the original request/run identity and is recovered by querying that same request. It is never replaced by an automatic second spawn.

## Profiles and workspace storage

The native store is `cli-workspace.v1.json`.

Important rules:

- writes use an independent lock, revision/CAS validation, synchronized temporary-file replacement, and fail-closed parsing;
- `inherit`, `set(value)`, and `unset` are distinct states;
- explicit false/empty/unset values are not revived by later legacy writes;
- Codex never inherits legacy Claude secrets or settings;
- unknown schema-v1 extensions survive reads and writes;
- legacy compatibility files may still exist, but new native writes do not mutate them.

## Authenticated document and projection boundary

Native frontend calls are admitted through the authenticated document bridge. Frontend-selected paths or opaque IDs alone never authorize filesystem access.

Native resource reads:

- bind the selected CLI/profile revision and a registered project;
- use backend-held scoped filesystem authority;
- return kind-specific projection DTOs only;
- do not return raw env values, headers, credential material, arbitrary argv, or unbounded native errors.

Profile/project changes invalidate stale projection scopes.

## Input path

Native input is an ordered per-run intent queue.

Key properties:

- monotone sequence is reserved before asynchronous clipboard work;
- keyboard, IME, paste, protocol replies, and native image-paste intent cannot silently reorder;
- large input uses authenticated staged `begin -> chunk* -> commit`;
- one PTY writer owns a complete frame at a time;
- partial/unknown host writes freeze later user input;
- partial/unknown writes are never replayed automatically;
- ambiguous `onData` bytes are never guessed to be terminal protocol replies.

## Output path

Native output uses a bounded stream with explicit offsets and acknowledgements.

Key properties:

- per-run high/low-water backpressure;
- global bounded payload budget;
- FIFO budget waiter progress;
- ACK must belong to the exact owner/run/generation/stream epoch;
- ACK must land on a parsed frame boundary;
- route revocation releases budget and wakes blocked peers;
- a degraded run does not poison another run.

## Observer boundary

Observer delivery is optional metadata, not process ownership.

- observer credentials/capabilities are scoped to the exact run/document;
- only allowlisted bounded metadata reaches the owning WebView;
- prompt, response, env, credential, and raw error bodies are excluded;
- observer failure cannot kill, restart, or implicitly retry a CLI;
- Codex/raw/shell launches never receive the Claude observer overlay.

## Security and diagnostics

Native UI surfaces are interpolation-only: no `v-html` / `innerHTML` execution path.

Public diagnostics use a fixed safe-code allowlist. Arbitrary exception text, serde errors, secret-looking codes, paths, env values, and user payloads are not reflected into the native workbench.

## Windows runtime

Windows uses the verified app-local ConPTY runtime. The installed-runtime certification for the tested Windows Server 2022 target verifies reinstall, relocation, missing-runtime and corrupt-runtime fail-closed behavior.

That evidence is target-specific; it is not a universal certification of every Windows build.

## Verification layers

Evidence is intentionally split:

- **Layer A** — pure/frontend/unit contracts.
- **Layer B** — host/runtime integration on CI targets.
- **Layer C** — real installed Claude Code / Codex CLI behavior using an authorized test account.

Layer A/B cannot be promoted to Layer C.

## Current code-side status

Completed and CI-verified:

- D01–D19 native foundation, authenticated scope, transport and terminal protocol work;
- D21 installed/runtime bridge certification on the tested Windows target;
- D22–D24 dual-CLI product integration and recovery workspace;
- D25 mixed-version rollback safety;
- D26 security/log/DOM/IPC boundaries;
- D27 low-resource fairness and fault stress.

D20's harness and target-machine command are implemented and tested, but real product certification remains **BLOCKED** until an authorized target environment supplies real Claude Code and Codex CLI evidence.

## Release boundary

The repository currently builds **signed candidate packages only**. `scripts/release-policy.mjs` returns false for publishing and the release workflow has no GitHub Release publish path.

Do not describe a candidate build as a published release, and do not re-enable publishing merely because code-side CI is green. Real-CLI evidence and an explicit promotion decision are separate gates.
