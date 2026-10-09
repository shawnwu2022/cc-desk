# CC Desk vision

## One-line vision

A native desktop workspace for **Claude Code and Codex CLI** that makes multi-project and multi-session work manageable without replacing the terminal-native experience.

## Who it is for

Developers who already rely on Claude Code, Codex CLI, or both and need several sessions running in parallel across multiple projects.

## What becomes easier

1. **Parallel sessions** — keep multiple real CLI sessions alive and switch without losing terminal state.
2. **Project/run identity** — know exactly which project, profile, run and generation a tab owns.
3. **Safe recovery** — recover uncertain launch state without creating a duplicate process.
4. **Read-only visibility** — inspect useful native resources through scoped backend projections.
5. **Host reliability** — preserve ordering, backpressure, process ownership, and safe diagnostics around the real CLI.

## Division of responsibility

| System | Responsibility |
|---|---|
| Claude Code / Codex CLI | conversation, commands, permissions, authentication, MCP/runtime behavior, extensions, editors |
| cc-switch or other external config tools | Provider/API-key/configuration switching when the user chooses to use them |
| CC Desk | desktop projects, tabs, owned PTYs, transport, recovery, scoped read projections, optional observer metadata |

CC Desk does not need cc-switch to run and does not read cc-switch internals.

## Principles

- **CLI-first** — do not reimplement interaction that the CLI already owns.
- **Exact identity** — never guess which run/session/write/ACK an event belongs to.
- **No hidden replay** — ambiguity is surfaced and recovered explicitly.
- **Read-only by default** — native CLI resources are projections, not a second configuration authority.
- **Fail closed** — missing authorization, stale revisions, unknown schemas, invalid scopes and unverifiable real-CLI evidence do not silently fall back.
- **Reversible** — users can always return to the normal CLI.

## Evidence discipline

Host tests prove host behavior only.

Real Claude Code / Codex CLI parity is a separate Layer-C certification problem. D20 remains BLOCKED until an explicitly authorized target environment produces the required evidence.

## End state

CC Desk should feel like a dependable multi-session shell around the real CLIs: more visibility and control at the desktop level, without changing what the CLI itself means.
