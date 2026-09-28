<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="CC Desk" width="80" height="80">
</p>

<h1 align="center">CC Desk</h1>

<p align="center">
  <strong>A native desktop workspace for Claude Code and Codex CLI — multi-project, multi-session management</strong><br>
  One window. Real CLIs. Isolated projects, profiles, and terminal tabs.
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

Claude Code and Codex CLI are strongest when their native terminal behavior remains intact. CC Desk adds a desktop workspace around those real binaries so you can manage **multiple projects**, **multiple CLI profiles**, and **independent terminal tabs** without replacing either CLI with an SDK or provider runtime.

The Native CLI workbench supports Claude Code and Codex CLI independently. A Codex-only setup does not require Claude configuration. Existing Claude-focused project/session panels remain available as legacy/read-only enhancements where applicable.

**The terminal stays authoritative; the desktop UI manages workspace, launch, recovery, and optional projections.**

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

Open independent Claude Code and Codex CLI terminal tabs in the same workspace. Each native run keeps its own CLI/profile/project identity, input/output stream, process lifecycle, and restart generation.

### Quick Launch with Presets

Use per-CLI profiles and explicit New, native resume picker, known session ID, or raw argv launches. CC Desk does not silently inject model, approval, sandbox, or permission defaults into new native profiles.

### Claude Provider Management (legacy tooling)

The existing Claude-focused Provider presets remain available for the legacy Claude workflow. They are not a Codex provider/credential layer and are not applied to Codex native profiles.

### Sidebar Panels

A side drawer with contextual panels — no overlay, no focus stealing:

- **Sessions** — Browse, search, and switch between all sessions. Status indicators show running/thinking/waiting states.
- **MCP Servers** — Inspect connected MCP servers, browse available tools and their input schemas
- **Skills & Agents** — Quick access to your Claude Code skills and agent configurations
- **Plugins** — View installed plugins and their components

### Native Terminal, Zero Compromise

The Native CLI workbench runs the user's real Claude Code or Codex CLI binary through the application-owned PTY and xterm host. Slash commands, native prompts, terminal editors, raw keyboard/protocol traffic, resize, clipboard and recovery stay on the CLI path rather than being reimplemented as a chat UI.

---

## Prerequisites

Install and authenticate at least one CLI you intend to use:

- **[Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code)**
- **[Codex CLI](https://developers.openai.com/codex/cli/)**

The Native CLI workbench does not require Git Bash for Codex-only use. Git Bash remains relevant only to legacy/explicit shell launch modes.

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
2. Add or select a project directory
3. Create/select a Claude Code or Codex CLI profile
4. Start New / Resume / Raw sessions; each native tab runs independently

---

## Building from Source

<details>
<summary>Click to expand</summary>

### Prerequisites

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://www.rust-lang.org/tools/install) stable toolchain (MSVC on Windows)
- Optional for runtime testing: [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) and/or [Codex CLI](https://developers.openai.com/codex/cli/)
- **Windows build only**: Microsoft C++ Build Tools and Windows SDK
- Git for Windows is only needed when testing explicit/legacy Git Bash launch paths

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
<summary><strong>Does this modify my Claude Code config?</strong></summary>

CC Desk treats Claude session and history data as read-only. Most GUI settings stay in the legacy-compatible `~/.cc-box/` directory. The only deliberate native-config write is Provider activation, which merges the selected env/model fields into `~/.claude/settings.json` after an explicit user action. Existing unrelated settings are preserved.
</details>

<details>
<summary><strong>Can I use all CLI features?</strong></summary>

Yes. Slash commands, keyboard shortcuts, model switching, permission prompts — everything passes through to the real CLI transparently.
</details>

<details>
<summary><strong>What's the performance like?</strong></summary>

Built with Tauri 2 (Rust backend), the app is ~10 MB installed and uses minimal RAM. The terminal renders via xterm.js, matching native terminal performance.
</details>

<details>
<summary><strong>Will it break when Claude Code updates?</strong></summary>

The app runs the CLI binary directly — it doesn't depend on any internal API. As long as the CLI is on your PATH, it works with any version.
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