# CC Desk

CC Desk is a desktop workspace for **Claude Code and Codex CLI** built with Tauri 2, Vue 3, xterm.js, and Rust.

It keeps the real CLI in charge of the interactive session and adds the host capabilities that become awkward in a normal terminal: multi-project navigation, multiple concurrent tabs, stable run identity, recovery, bounded terminal transport, and read-only native-resource projection.

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
- Skills, agents, MCP servers, plugins, history, and related native resources are projected read-only by the new native workspace.
- Observer failure never kills or restarts a CLI.
- A launch or input result that is ambiguous is never silently replayed.

## Native CLI workspace

The title bar and welcome screen expose the **Native CLI** workspace.

The native workspace supports:

| Capability | Claude Code | Codex CLI |
|---|---:|---:|
| New session | Yes | Yes |
| Native resume picker | Yes | Yes |
| Known session-ID resume | Yes | Yes |
| Exact raw argv | Yes | Yes |
| Registered project selection | Yes | Yes |
| Independent profile selection | Yes | Yes |
| Read-only native resource projection | Yes | Yes |
| Ordered staged input | Yes | Yes |
| Bounded output + ACK | Yes | Yes |
| Optional Claude observer overlay | Yes | No |

The repository still contains the legacy Claude workspace for compatibility. New dual-CLI work must use the authenticated native path and must not fall back to legacy PTY APIs.

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

The repository currently builds **signed candidate packages only**.

Publishing is deliberately disabled:

- `scripts/release-policy.mjs` returns false;
- `.github/workflows/release.yml` builds and uploads candidate artifacts;
- it does not publish a GitHub Release or updater manifest.

A green code-side CI run is not an authorization to publish.

## Building from source

### Requirements

- Node.js 20+
- Rust stable
- platform dependencies required by Tauri 2
- the CLI(s) you intend to run installed and authenticated on the target machine
- Windows: MSVC build tools / Windows SDK; the application packages its verified private ConPTY runtime

### Commands

```bash
npm ci
npm run typecheck
npm run test:ci
npm run build

cd src-tauri
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

Run the desktop app with:

```bash
npm run tauri:dev
```

## Project origin

CC Desk began as a fork of `orczh-hj/cc-box` and is now maintained independently. The original MIT attribution is preserved in [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md).

Legacy identifiers such as `~/.cc-box/` and some `CC_BOX_*` names remain where changing them would break compatibility. They are compatibility names, not product ownership boundaries.

## Documentation

- [Native CLI v3 architecture](docs/native-cli-v3.md)
- [Native CLI execution ledger](docs/superpowers/execution/)
- [Real CLI certification command](docs/testing/d20-real-cli-command.md)
- [Terminal integration](docs/terminal-integration.md)
- [Data persistence](docs/data-persistence.md)
- [Release process](docs/release-process.md)
- [Roadmap](docs/roadmap.md)

## License

MIT.
