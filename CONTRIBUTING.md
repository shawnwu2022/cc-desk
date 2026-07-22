# Contributing to CC Desk

Thanks for contributing to CC Desk. This document is the source of truth for the public contribution workflow.

## Before You Start

- Read the [Code of Conduct](CODE_OF_CONDUCT.md), [Security Policy](SECURITY.md), and [Governance](GOVERNANCE.md).
- Use GitHub Issues for reproducible bugs and GitHub Discussions for questions or early ideas.
- Do not include API keys, access tokens, unredacted logs, private paths, session transcripts, or Provider configuration in public content.

## Development Environment

| Requirement | Version or purpose |
|---|---|
| Node.js | 20 or later |
| Rust | stable MSVC toolchain on Windows; stable toolchain on macOS/Linux |
| Claude Code CLI | Installed and authenticated for real terminal-session testing |
| Windows build tools | Microsoft C++ Build Tools and Windows SDK |
| macOS/Linux | Platform packages required by Tauri/WebKit when building locally |

On Windows, use the MSVC Rust target. Do not use the GNU target for CI-compatible builds:

```powershell
rustup default stable-x86_64-pc-windows-msvc
```

## Setup

```bash
git clone https://github.com/shawnwu2022/cc-desk.git
cd cc-desk
npm ci
```

For local development:

```bash
npm run tauri:dev
```

## Required Verification

Run the checks relevant to your change before opening a pull request:

```bash
npm run typecheck
npm test
(
  cd src-tauri
  cargo fmt --check
  cargo clippy -- -D warnings
  cargo test
)
```

Run real PTY, installer, updater, and visual checks manually when your change affects those boundaries. Record the manual verification in the pull request.

## Pull Requests

1. Fork the repository and create a focused branch.
2. Keep commits scoped to one change; do not bundle formatting or unrelated refactors.
3. Add or update automated tests for changed behavior. Bug fixes must include a regression test when feasible.
4. Update public documentation when behavior, configuration, supported platforms, or user-visible copy changes.
5. Complete the pull request template and wait for required CI checks before requesting review.

All contributions are licensed under the [MIT License](LICENSE).