# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.7] - 2026-04-30

### Fixed

- Claude CLI not found on macOS/Linux when launched from GUI (Finder/Dock/desktop)
- GUI apps don't inherit terminal PATH, now refreshes PATH from login shell

## [0.2.5] - 2026-04-29

### Fixed

- Projects list not loading when content height is too short to trigger scroll
- Claude CLI not launching on Mac with npm installation (cli.js symlink detection)
- Claude CLI not launching when path contains spaces (Windows)
- Node.js script detection for various installation methods (exe, npm shim, cli.js)

### Added

- "Load More Projects" button for manual loading trigger
- Claude launcher type detection and caching (direct/node)
- Claude launcher type saved to config for faster startup

### Changed

- Default terminal font size changed to 12px (from 14px/10px inconsistency)
- Improved Claude CLI startup detection for multiple installation types
- Updated terminal integration and startup checks documentation

## [0.2.4] - 2026-04-29

### Added

- Alt+N/Alt+R shortcuts for new/restart session (terminal view only)
- Shortcut hints on session buttons (Alt+N, Alt+R)
- Sidebar data preload on startup (skills, agents, MCP servers, plugins)
- Spawn new app instance instead of multi-window (Ctrl+Shift+N)

### Fixed

- Terminal copy not working (Ctrl+C with selection, Ctrl+Shift+C)
- Console window flash on Windows (CREATE_NO_WINDOW flag)
- New/restart session shortcuts not triggering (event listener timing)
- Window snap shortcuts not working (arrow key lowercase)
- Sidebar data not loading on app startup

### Changed

- Refactored sidebar store to support preloaded data
- Panel components now use centralized sidebar store
- Updated keyboard shortcuts documentation

## [0.2.3] - 2026-04-28

### Added

- Keyboard shortcuts reference in docs/interaction.md
- Session rename functionality in sidebar
- Empty state UI for sessions panel

### Fixed

- Terminal instances destroyed on view switch
- Focus issues after window restoration
- Keyboard shortcut interference between views

### Changed

- Refactored keyboard shortcuts handling (capture phase)
- Improved PTY lifecycle management
- Window title updates based on project folder

## [0.2.1] - 2025-04-27

### Added

- Global settings overlay accessible from any view (welcome, projects, terminal)
- Settings button in ProjectSelectView header (next to "Projects" title)
- Use app icon in About section instead of placeholder text

### Changed

- `Ctrl+,` shortcut now opens settings from any view
- Menu bar Settings/Shortcuts works in all views (not just terminal)
- Settings panel now displayed as global overlay instead of inline in terminal view

## [0.2.0] - 2025-04-27

### Added

- Settings panel with appearance, shortcuts, startup, and about sections
- Update check functionality (check GitHub Releases for new versions)
- Clipboard-manager plugin for paste support (Ctrl+V)
- Window snap buttons (snap to left/right half of screen)
- Custom app icons (claude-color design)
- `.idea/` to gitignore for JetBrains IDE users

### Fixed

- Window decorations missing in dev mode (added `decorations: true` to config)
- Sidebar toggle not working when settings panel is open
- Removed unused `-webkit-app-region` CSS (for borderless window)

### Changed

- Right-click disabled in production build for cleaner UX
- UI color system refinements (artisan terminal theme)
- Improved sidebar panel toggle logic

## [0.1.0] - 2025-04-24

### Added

- Initial release
- Multi-terminal support with xterm.js + portable-pty
- Sidebar panels: Sessions, Skills, Agents, MCP Servers, Plugins
- Project quick launch with per-project options
- Native terminal experience (runs real Claude CLI)
- Cross-platform builds (Windows, macOS, Linux)
- CI/CD with GitHub Actions