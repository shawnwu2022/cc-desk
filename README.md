<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="Claude Code GUI" width="80" height="80">
</p>

<h1 align="center">Claude Code GUI</h1>

<p align="center">
  <strong>A desktop manager for power users of <a href="https://docs.anthropic.com/en/docs/claude-code">Claude Code</a></strong><br>
  One window. Multiple sessions. Everything you wish the CLI could show you.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue" alt="Platform">
  <img src="https://img.shields.io/badge/Tauri-2.x-orange" alt="Tauri">
  <img src="https://img.shields.io/badge/Vue-3-green" alt="Vue">
  <img src="https://img.shields.io/badge/License-MIT-yellow" alt="License">
</p>

---

## Why Claude Code GUI?

Claude Code's CLI is great for single-session work. But if you're juggling **multiple projects**, **running several agents in parallel**, or want to **see token usage and costs at a glance** — the terminal alone falls short.

Claude Code GUI doesn't replace the CLI. It wraps it with a native terminal experience and adds the things the CLI can't do well: multi-session orchestration, information dashboards, and quick project switching.

**Think of it as iTerm2/Warp, but purpose-built for Claude Code.**

---

## Highlights

### Multi-Session in One Window

Open as many Claude Code sessions as you need — each runs independently in its own terminal tab. Switch between them instantly, output is preserved when you switch back. No more juggling terminal windows.

### Project Quick Launch

Browse your favorite projects and launch a session with one click. Set per-project startup options like `--continue`, `--model`, or custom flags. No need to `cd` and type the same arguments every time.

### Sidebar Panels

A side drawer with contextual panels — no overlay, no focus stealing:

- **Sessions** — Browse, search, and switch between all sessions (with scroll-to-load for large histories)
- **MCP Servers** — Inspect connected MCP servers, browse available tools and their schemas
- **Skills & Agents** — Quick access to your Claude Code skills and agent configurations
- **Plugins** — View installed plugins

### Native Terminal, Zero Compromise

The app runs the real Claude CLI binary through a pseudo-terminal. Everything works exactly as in your terminal — slash commands, keyboard shortcuts, streaming output, colors, and interactive prompts.

### Settings That Make Sense

Adjust font size in real time, set default startup flags (`--continue`, `--skip-permissions`, custom args), and configure once for all future sessions.

---

## Quick Start

### 1. Make sure Claude Code is installed

```bash
# If you haven't installed Claude Code yet
npm install -g @anthropic-ai/claude-code
claude        # Run once to authenticate
```

### 2. Download & Install

Head to the [**Releases**](https://github.com/orczh/claude-tauri-gui/releases) page and grab the installer for your platform:

| Platform | File |
|----------|------|
| **Windows** | `.exe` (NSIS installer) or `.msi` |
| **macOS** | `.dmg` (universal binary) |
| **Linux** | `.deb` or `.AppImage` |

### 3. Launch & Go

1. Open the app
2. Click **Open New Project** and select a directory
3. A Claude Code session starts — just type as you would in the terminal
4. Open more sessions from the sidebar, each runs independently

---

## Building from Source

<details>
<summary>Click to expand</summary>

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://www.rust-lang.org/tools/install) stable toolchain
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated
- **Windows only**: MinGW-w64 (`C:\ProgramData\mingw64\mingw64\bin` in PATH)

### Setup

```bash
git clone https://github.com/orczh/claude-tauri-gui.git
cd claude-tauri-gui
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
npm run build:win     # Windows (x86_64-pc-windows-gnu)
npm run build:mac     # macOS (universal)
npm run build:linux   # Linux (x86_64)
```

Output goes to `src-tauri/target/release/bundle/`.

</details>

---

## FAQ

<details>
<summary><strong>Does this modify my Claude Code config?</strong></summary>

No. The app only reads native Claude Code files. All GUI-specific settings are stored separately in `~/.claude-gui/`. You can go back to the CLI at any time.
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

## Tech Stack

Tauri 2 (Rust) + Vue 3 + TypeScript + xterm.js + portable-pty

---

## License

[MIT](LICENSE)
