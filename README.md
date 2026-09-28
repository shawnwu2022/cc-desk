<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="CC Desk" width="80" height="80">
</p>

<h1 align="center">CC Desk</h1>

<p align="center">
  <strong>A native desktop workspace for Claude Code and Codex CLI — multi-project, multi-tab workflows</strong><br>
  One window. Multiple projects. Instant session switching.
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

Claude Code and Codex CLI are strongest when they retain ownership of their native terminal, authentication, permissions, models, and extensions. Managing **multiple projects and concurrent CLI runs** still becomes cumbersome across many terminal windows.

CC Desk is a **native desktop workspace for the real installed CLIs**. It adds project registration, per-CLI profiles, multi-tab execution, recovery state, and scoped read-only resource views without replacing the CLI with an SDK or provider proxy.

**Think of it as one desktop workbench for Claude Code and Codex CLI power users.**

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

Open independent Claude Code and Codex CLI runs in native terminal tabs. Each tab freezes its CLI/profile/project/run identity, and switching tabs does not reroute input or output.

### Quick Launch with Presets

Use per-CLI profiles plus New, native resume picker, explicit session ID, or exact raw argv. CC Desk does not inject a model/approval/sandbox policy into new native profiles.

### Legacy Claude Provider Management

The existing Claude provider panel remains available for legacy Claude workflows. Activating a Provider is an explicit Claude-only action that merges selected env/model fields into `~/.claude/settings.json`; it is not copied into Codex profiles.

### Sidebar Panels

A side drawer with contextual panels — no overlay, no focus stealing:

- **Sessions** — Browse, search, and switch between all sessions. Status indicators show running/thinking/waiting states.
- **MCP Servers** — Inspect connected MCP servers, browse available tools and their input schemas
- **Skills & Agents** — Scoped read-only views where the selected CLI/root supports them
- **Plugins** — View installed plugins and their components

### Native Terminal, Zero Compromise

The Native CLI workbench runs the real installed Claude Code or Codex CLI binary through the application-owned PTY and xterm parser. The host preserves CLI-owned interaction rather than reimplementing commands, approvals, authentication, or model routing.

---

## Prerequisites

- At least one supported CLI installed for runtime use: **Claude Code and/or Codex CLI**
- Authenticate each CLI through its own native workflow
- **Windows source builds only**: Microsoft C++ Build Tools and Windows SDK. Git for Windows is optional for legacy shell-based workflows, not a global Codex requirement.

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

1. Open the app
2. Select or add a project directory
3. Open **Native CLI**, choose or create a Claude/Codex profile, then select New/Resume/Raw
4. Open additional native tabs as needed; each run remains independently owned

---

## Building from Source

<details>
<summary>Click to expand</summary>

### Prerequisites

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://www.rust-lang.org/tools/install) stable toolchain (MSVC on Windows)
- Node.js and Rust are sufficient to build the application
- Install Claude Code and/or Codex CLI when testing native runtime behavior
- **Windows only**: Microsoft C++ Build Tools and Windows SDK

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
<summary><strong>Does this modify my CLI config?</strong></summary>

The new native workspace keeps its own profiles/project registry under CC Desk data and treats native CLI history/configuration as read-only. Codex profiles do not inherit Claude legacy env/permission settings. The existing Claude Provider activation remains an explicit legacy Claude-only write to `~/.claude/settings.json`; unrelated Claude settings are preserved.
</details>

<details>
<summary><strong>Can I use all CLI features?</strong></summary>

The native host is designed to preserve the real CLI/TUI rather than whitelist individual commands. Compatibility is certified per concrete OS/CLI/package target; an untested or newly changed CLI version is not automatically labeled certified.
</details>

<details>
<summary><strong>What's the performance like?</strong></summary>

Built with Tauri 2 (Rust backend), the app is ~10 MB installed and uses minimal RAM. The terminal renders via xterm.js, matching native terminal performance.
</details>

<details>
<summary><strong>What happens when Claude Code or Codex CLI updates?</strong></summary>

Unknown versions can still be launched when the configured executable/profile is valid, but release certification is version-specific. Pinned/stable canaries record binary identities and a changed CLI requires the relevant target combinations to be re-certified; CC Desk does not claim every future CLI version is automatically compatible.
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