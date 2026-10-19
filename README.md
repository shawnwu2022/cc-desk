# CC Desk

CC Desk is a desktop workspace for **Claude Code, Codex CLI and opencode** built with Tauri 2, Vue 3, xterm.js, and Rust.

It keeps the real CLI in charge of the interactive session and adds the host capabilities that become awkward in a normal terminal: multi-project navigation, multiple concurrent sessions, stable run identity, recovery, bounded terminal transport, and read-only native-resource projection.

> Native CLI v3 is currently code-complete through D27 on the development stack. Real Claude Code / Codex CLI Layer-C certification (D20) still requires an authorized target environment. Do not treat host CI as real-CLI certification.

## What CC Desk does

- Run real **Claude Code** or **Codex CLI** processes in owned PTYs.
- Keep multiple projects and sessions open in one desktop window.
- Start a new session, open the CLI-native resume picker, resume a known native session ID, or launch exact raw argv.
- Preserve exact argv boundaries; raw native argv is JSON `string[]`, not shell-split text.
- Keep input ordered across keyboard, IME, paste, terminal replies, and native image-paste intent.
- Apply bounded output backpressure with exact run/generation/stream ownership.
- Project native history/resources through an authenticated, read-only backend scope.
- Keep Claude observer metadata optional and isolated from process ownership.

## What CC Desk does not own

CC Desk does **not** own Provider/API-key switching or native CLI configuration mutation.

- Provider/configuration switching belongs to Claude Code, Codex CLI, or external tools such as cc-switch.
- Skills, agents, MCP servers, plugins, history, and related native resources are projected read-only in the current project/session context.
- Observer failure never kills or restarts a CLI.
- A launch or input result that is ambiguous is never silently replayed.

## Unified workspace

The three main destinations are **Workspace**, **Projects** and **Settings**. One
project/session tree mixes Claude Code, Codex CLI and opencode sessions below each project.
The contextual resource drawer shows instructions, settings, MCP, skills, agents
and plugins as structured read-only content.

New and Restore use shared dialogs. Settings include launch configurations,
terminal preferences with a static preview, configurable application shortcuts,
updates and safe diagnostic copying. GUI and terminal themes are independent.
Switching views or colors preserves running sessions and terminal scrollback.

The unified workspace supports:

| Capability | Claude Code | Codex CLI | opencode |
|---|---:|---:|---:|
| New session | Yes | Yes | Yes |
| Native resume picker | Yes | Yes | No |
| Known session-ID resume | Yes | Yes | Yes |
| Exact raw argv | Yes | Yes | Yes |
| Registered project selection | Yes | Yes | Yes |
| Independent launch configuration selection | Yes | Yes | Yes |
| Read-only native resource projection | Yes | Yes | History only |
| Ordered staged input | Yes | Yes | Yes |
| Bounded output + ACK | Yes | Yes | Yes |
| Optional Claude observer overlay | Yes | No | No |

opencode history is read directly from its local SQLite store (read-only). CLI-side picker/rename/unarchive are unsupported and their UI entries are hidden, not degraded. Creating an opencode launch configuration is a one-way upgrade for older app versions.

Existing Claude history and sessions remain supported through the Legacy adapter in the same shell. New Claude/Codex sessions use the authenticated Native adapter. There is no separate Native product page or Legacy startup route, and Native operations never fall back to Legacy PTY APIs.

### Native session rename

Claude GUI rename uses the pinned public Agent SDK's local metadata API while keeping the full CLI TUI. It requires **Node.js 22.13+ on the launch configuration PATH**, a writable configuration root supporting same-volume hard links, and a plain JSONL transcript. Standalone Claude installations do not necessarily include Node. CC Desk embeds the SDK helper and includes its license notices, but does not bundle or install Node. If necessary, install/update Node, make it available to that launch configuration, and restart CC Desk; alternatively use `/rename` directly in the CLI (resume a closed Claude session first). Missing prerequisites affect GUI rename only; the existing CLI can still run.

Codex GUI rename uses the official app-server API. Opened resumed tabs explicitly target their retained source session, rather than assuming which thread the TUI currently shows. Both paths require exact source identity and verified native readback; failed or unknown writes do not save a display alias or replay automatically. Real idle Claude 2.1.296 rename/resume has been checked in a private fixture. Busy writer concurrency, incident-version 2.1.301 and installed WebView2 remain unverified.

## Safety properties

The native path is intentionally fail-closed:

- profile/workspace writes use revision checks and atomic replacement;
- explicit `unset` and false/empty values are not revived by legacy state;
- Codex never inherits legacy Claude secrets;
- native frontend commands go through the authenticated document bridge;
- native UI uses interpolation only—no `v-html` or `innerHTML` sink;
- public errors are fixed safe codes rather than arbitrary native exception text;
- partial/unknown input writes are not replayed automatically;
- wrong-owner or stale-generation output ACKs are rejected.

See [docs/native-cli-v3.md](docs/native-cli-v3.md) for the authoritative architecture.

## Verification status

The latest completed code-side stack through D27 has green host evidence for frontend, Rust, Windows loader, output transport, observer isolation, rollback/security boundaries, and low-resource stress.

D20 remains a separate gate. Its harness and target-machine command are implemented, but a PASS requires real Claude Code and real Codex CLI execution with an explicitly authorized test account.

Synthetic fixtures, model echo, screen scraping, or host writer receipts cannot substitute for that evidence.

## Release status

Releases promote one signed platform set from **protected main** through an explicit promotion gate:

- `scripts/release-policy.mjs` gates publication on required CI success, a three-way consistent stable version, and a coverage artifact bound to that exact source/CI run; any anomaly fails closed;
- `.github/workflows/release.yml` builds signed candidates, rechecks the exact main source, CI and coverage, then publishes the GitHub Release and the `latest.json` updater manifest only after the gate passes; an existing conflicting draft goes through the reviewed draft recovery transaction;
- ordinary development PRs run verification only: no version bump, tag or release.

A green code-side CI run alone is not an authorization to publish, and the gate must not be bypassed via Gitee/OSS or other channels.

## Building from source

### Requirements

- Node.js 22.13+
- Rust stable
- platform dependencies required by Tauri 2
- the CLI(s) you intend to run installed and authenticated on the target machine
- Windows: MSVC build tools / Windows SDK; the application packages its verified private ConPTY runtime

### Commands

Install dependencies and build the frontend:

```bash
npm ci
npm run typecheck
npm run build
```

During iteration, select the checks that cover the changed behavior:

| Scope | Command |
| --- | --- |
| Affected tests | `npm test -- <test file or directory>` |
| Ordinary Node/DOM tests | `npm run test:fast` |
| Real subprocess/script/HTTP contracts | `npm run test:process` |
| Full stress workload | `npm run test:stress` |
| Production startup under the actual CSP | `npm run test:production` |

For complete Vitest and Rust checks, use the following separately from the iteration loop. Before direct Cargo commands, run `npm run prepare:session-sdk`; on Windows also run `node scripts/prepare-conpty.mjs` from the repository root.

```bash
npm run test:ci

cd src-tauri
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

`npm test`, `test:all` and `test:ci` cover all four Vitest groups. Independent Node policy, Rust, production startup, visual and platform gates remain separate. `test:rust:lib` still contains real process integration tests; it is not a fast suite. See the [test guide](docs/测试编写原则.md) and [CI performance record](docs/ci-performance.md).

Install the production-startup browser with `npx playwright install --with-deps chromium` before `npm run test:production`. This required Frontend check builds the normal production entry and applies the production CSP to English/Chinese startup and navigation; its IPC fixture does not certify an installed WebView2 or real CLI. `npm run typecheck` generates shared configuration declarations into the ignored cache before checking the application and tests.

Run the desktop app with:

```bash
npm run tauri:dev
```

## Project origin

CC Desk began as a fork of `orczh-hj/cc-box` and is now maintained independently. The original MIT attribution is preserved in [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md).

Legacy identifiers such as `~/.cc-box/` and some `CC_BOX_*` names remain where changing them would break compatibility. They are compatibility names, not product ownership boundaries.

## Documentation

- [Shared agent standard](AGENTS.md) and [tool entry points](docs/agent-guides.md)
- [Detailed project contracts](docs/project-contracts.md)
- [Test guide](docs/测试编写原则.md) and [CI performance](docs/ci-performance.md)
- [Native CLI v3 architecture](docs/native-cli-v3.md)
- [Native CLI execution ledger](docs/superpowers/execution/)
- [Real CLI certification command](docs/testing/d20-real-cli-command.md)
- [Terminal integration](docs/terminal-integration.md)
- [Data persistence](docs/data-persistence.md)
- [Release process](docs/release-process.md)
- [Roadmap](docs/roadmap.md)

## License

MIT.
