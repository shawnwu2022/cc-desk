# Contributing to CC Desk

Thanks for your interest in contributing! This document outlines how to get started.

## Development Setup

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://www.rust-lang.org/tools/install) stable toolchain
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated
- **Windows only**: MinGW-w64 (`C:\ProgramData\mingw64\mingw64\bin` in PATH)

### Getting Started

```bash
git clone https://github.com/shawnwu2022/cc-desk.git
cd cc-desk
npm install
npm run tauri:dev
```

## Project Structure

```
cc-desk/
├── src-tauri/          # Rust backend (PTY, IPC commands)
│   ├── src/
│   │   ├── lib.rs      # App initialization
│   │   ├── pty.rs      # PTY management
│   │   ├── commands.rs # Tauri IPC commands
│   │   └── store.rs    # Claude Code data reading
│   └── tauri.conf.json # Tauri configuration
├── src/                # Vue 3 frontend
│   ├── components/     # UI components
│   ├── stores/         # Pinia state management
│   ├── api/tauri.ts    # Tauri API wrappers
│   └── styles/         # CSS variables & global styles
├── docs/               # Detailed documentation
└── CLAUDE.md           # Project instructions for Claude Code
```

## Code Style

- **Rust**: Follow standard Rust conventions (`cargo fmt`, `cargo clippy`)
- **Vue/TypeScript**: Use composition API, follow existing patterns
- **CSS**: Use CSS variables from `global.css`, avoid hardcoded colors
- **Commits**: Use descriptive messages, reference issues when applicable

## Pull Request Process

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/your-feature`)
3. Make your changes
4. Test with `npm run tauri:dev`
5. Commit and push (`git push origin feature/your-feature`)
6. Open a Pull Request with a clear description

## Reporting Issues

Use [GitHub Issues](https://github.com/shawnwu2022/cc-desk/issues) for:

- Bug reports (include OS, version, steps to reproduce)
- Feature requests
- Questions about usage

## License

By contributing, you agree that your contributions will be licensed under the MIT License.