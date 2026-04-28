# Release Notes 模板

发布新版本时，使用以下模板格式：

## GitHub Release Notes

```
## v0.2.4

### Bug Fixes

- Fix terminal copy not working (Ctrl+C/Ctrl+Shift+C)
- Fix console window flashing on Windows (CREATE_NO_WINDOW)
- Fix Alt+N/Alt+R shortcuts not triggering new/restart session
- Fix window snap shortcuts (Ctrl+Shift+Arrow) not working
- Fix sidebar data not loading on startup

### Features

- Add Alt+N shortcut for new session
- Add Alt+R shortcut for restart session
- Add shortcut hints under session buttons
- Spawn new app instance (Ctrl+Shift+N) instead of multi-window
- Preload sidebar data on startup

### Downloads

| Platform | File |
|----------|------|
| Windows | `CC-Box_0.2.4_x64-setup.exe` |
| macOS | `CC-Box_0.2.4_universal.dmg` |
| Linux | `CC-Box_0.2.4_amd64.deb` / `CC-Box_0.2.4_amd64.AppImage` |

---

**Full Changelog**: https://github.com/orczh-hj/cc-box/compare/v0.2.3...v0.2.4
```

## CHANGELOG.md 格式

```markdown
## [0.2.4] - 2025-04-29

### Fixed

- Terminal copy not working (added Ctrl+C/Ctrl+Shift+C handling)
- Console window flashing on Windows (added CREATE_NO_WINDOW flag)
- Alt+N/Alt+R shortcuts not triggering new/restart session
- Window snap shortcuts (Ctrl+Shift+Arrow) not working
- Sidebar data not loading on startup

### Added

- Alt+N shortcut for new session
- Alt+R shortcut for restart session
- Shortcut hints under session buttons
- Spawn new app instance (Ctrl+Shift+N)
- Preload sidebar data on startup

### Changed

- Sidebar panels now use shared store data
- Keyboard key comparison uses lowercase
```

## Commit 格式

```
Release v0.2.4

Bug fixes:
- Fix terminal copy
- Fix console window flash

Features:
- Add Alt+N shortcut
- Add Alt+R shortcut

Author <Chen Zihan>: orczh_hj@163.com
```