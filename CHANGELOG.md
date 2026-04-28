# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.4] - 2025-04-29

### Fixed

- Terminal copy not working (added Ctrl+C/Ctrl+Shift+C handling)
- Console window flashing on Windows (added CREATE_NO_WINDOW flag)
- Alt+N/Alt+R shortcuts not triggering new/restart session
- Window snap shortcuts (Ctrl+Shift+Arrow) not working
- Sidebar data (skills/agents/mcp/plugins) not loading on startup
- Event listeners not set when cwd empty in TerminalView

### Added

- Alt+N shortcut for new session (only in terminal view)
- Alt+R shortcut for restart session (only in terminal view)
- Shortcut hints displayed under New/Restart buttons
- Spawn new app instance (Ctrl+Shift+N) instead of multi-window
- Preload sidebar data on entering terminal view

### Changed

- Sidebar panels now use shared store data (pre-loaded on startup)
- Keyboard event key comparison now uses lowercase for consistency
- Updated interaction.md with new shortcuts documentation

## [0.2.3] - 2025-04-28

### Added

- Keyboard shortcuts for window management (snap left/right half screen)
- Window snap buttons in terminal header

### Fixed

- Terminal instances destroyed on view switch
- Focus issues when switching between views
- Keyboard shortcuts interfering with terminal input

### Changed

- Refactored keyboard shortcuts into dedicated composable
- Improved focus recovery mechanism

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