<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="CC Desk" width="80" height="80">
</p>

<h1 align="center">CC Desk</h1>

<p align="center">
  <strong>A native desktop workspace for Claude Code and Codex CLI — multi-project, multi-session management</strong><br>
  One window. Real CLIs. Isolated projects and terminal tabs.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue" alt="Platform">
  <img src="https://img.shields.io/badge/Tauri-2.x-orange" alt="Tauri">
  <img src="https://img.shields.io/badge/Vue-3-green" alt="Vue">
  <img src="https://img.shields.io/badge/License-MIT-yellow" alt="License">
</p>

---

English | [简体中文](README_CN.md)

---

## Why CC Desk?

Claude Code and Codex CLI are strongest when they keep ownership of their native terminal UX. CC Desk adds a desktop workspace around those real CLIs: explicit project registration, per-CLI profiles, multiple isolated terminal tabs, native resume entry, and source-scoped resource panels.

The native path does **not** replace either CLI with an SDK or app-server protocol. CLI authentication, model selection, permission prompts, slash commands, editors, and other native behavior remain owned by the installed CLI.

Compatibility is evidence-based. Unknown CLI versions may still launch, but they are not automatically labeled certified; certified target combinations must pass the repository's machine-verifiable acceptance gate.

---

## Screenshots

<p align="center">
  <img src="screenshots/projectselect.png" alt="Project Selection" width="400">
  <img src="screenshots/project.png" alt="Session Management" width="400">
</p>

---

## Highlights

### Multi-Project Management

Browse all your projects in one place. See which projects have active sessions, launch a new session with one click, and switch between projects instantly. No more `cd` between directories or managing multiple terminal windows.

### Multi-Session in One Window

Run Claude Code and Codex CLI side by side. Each native tab freezes its CLI/profile/project/run identity, so input, output, restart state, and recovery do not cross between sibling tabs.

### Quick Launch with Presets

Use per-CLI profiles and explicit project registration. Start a new CLI, use its native resume picker, resume a known session ID, or pass an exact raw argv array without shell re-parsing.

### Provider Management

Manage Provider presets directly or import existing entries from cc-switch. Activating a Provider is an explicit action that merges the selected env/model fields into `~/.claude/settings.json`; unrelated Claude settings are preserved.

### Sidebar Panels

A side drawer with contextual panels — no overlay, no focus stealing. Native projections are bound to the selected CLI profile and registered project root, and unavailable capabilities stay explicitly unavailable instead of falling back to another CLI's data.

Legacy Claude panels remain available where supported; Codex projections are kept separate from Claude configuration/history roots.

### Native Terminal, Zero Compromise

The app runs the real Claude Code or Codex CLI binary through an owned pseudo-terminal. The host preserves raw argv, ordered input, terminal protocol replies, bounded output flow control, resize, native clipboard/IME paths, and process ownership without silently switching to a different executable or legacy PTY route.

---

## Prerequisites

Install and authenticate the CLI(s) you intend to use:

- **[Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code)**
- **[Codex CLI](https://developers.openai.com/codex/cli/reference)**
- **Windows / source builds**: follow the relevant CLI requirements plus the Rust/MSVC prerequisites listed below.

---

## Quick Start

### 1. Download & Install

Head to the [**Releases**](https://github.com/shawnwu2022/cc-desk/releases) page and grab the installer for your platform:

| Platform | File |
|----------|------|
| **Windows** | `.exe` (NSIS installer) |
| **macOS** | `.dmg` (Apple silicon) |
| **Linux** | `.AppImage` |

### 2. Launch & Go

1. Open the app and enter **Native CLI**
2. Register or select a project directory
3. Select or explicitly create a Claude Code or Codex CLI profile
4. Start **New**, native **Resume**, a known session ID, or an exact raw argv launch
5. Open sibling tabs as needed; each run remains independently owned

---

## Building from Source

<details>
<summary>Click to expand</summary>

### Prerequisites

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://www.rust-lang.org/tools/install) stable toolchain (MSVC on Windows)
- The Claude Code and/or Codex CLI binaries needed for the scenarios you want to test
- **Windows builds**: Microsoft C++ Build Tools and Windows SDK; follow each CLI's own Windows runtime requirements

### Setup

```bash
git clone https://github.com/shawnwu2022/cc-desk.git
cd cc-desk
npm install
```

### Development

```bash
npm run tauri:dev     # Start dev mode with hot reload
```

### Build

```bash
npm run tauri:build   # Build for current platform

# Or platform-specific:
npm run build:win     # Windows (x86_64-pc-windows-msvc)
npm run build:mac     # macOS (universal)
npm run build:linux   # Linux (x86_64)
```

Output goes to `src-tauri/target/release/bundle/`.

</details>

---

## FAQ

<details>
<summary><strong>Does this modify my native CLI config or history?</strong></summary>

The native dual-CLI workspace keeps its own profile/project state in CC Desk storage and treats native CLI history/config projections as read-only. Existing legacy Claude GUI compatibility remains isolated. The deliberate exception is the pre-existing **Provider activation** action for Claude, which explicitly merges selected env/model fields into `~/.claude/settings.json`; it is not applied to Codex.
</details>

<details>
<summary><strong>Can I use all CLI features?</strong></summary>

CC Desk deliberately avoids a feature allowlist and forwards native terminal behavior through the real CLI. That is different from claiming every CLI/OS/version combination is certified. Release certification is target-specific and must be backed by the acceptance evidence gate; missing or blocked real-CLI evidence is not reported as PASS.
</details>

<details>
<summary><strong>What's the performance like?</strong></summary>

Built with Tauri 2 (Rust backend), the app is ~10 MB installed and uses minimal RAM. The terminal renders via xterm.js, matching native terminal performance.
</details>

<details>
<summary><strong>What happens when Claude Code or Codex CLI updates?</strong></summary>

Unknown versions are not blocked merely because the GUI has not seen them before, but they are also not automatically certified. The repository includes pinned/latest canaries plus target-specific acceptance evidence so maintainers can detect changes and re-certify without restarting or rewriting already-running user sessions.
</details>

---

## Project Origin

CC Desk began as a fork of [orczh-hj/cc-box](https://github.com/orczh-hj/cc-box). It has since evolved independently with a different product direction and is not presented as an official successor to the upstream project.

The original MIT license and copyright notice are preserved in [LICENSE](LICENSE). Additional attribution details are recorded in [NOTICE.md](NOTICE.md).

Because CC Desk uses a new application identifier, it installs independently from CC-Box. Existing settings are still reused from `~/.cc-box/`; uninstall the old application separately after confirming the new installation works.

---

## Tech Stack

Tauri 2 (Rust) + Vue 3 + TypeScript + xterm.js + portable-pty

---

## License

[MIT](LICENSE)

## Community

- [Contributing](CONTRIBUTING.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)
- [Security Policy](SECURITY.md)
- [Support](SUPPORT.md)
- [Governance](GOVERNANCE.md)
- [Issues](https://github.com/shawnwu2022/cc-desk/issues)
- [Discussions](https://github.com/shawnwu2022/cc-desk/discussions)