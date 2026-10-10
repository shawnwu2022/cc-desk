# Changelog

## [0.18.5] - 2026-10-10

- Add local animated status icons with specific activity glyphs, passive accessible labels and static reduced-motion rendering; preserve exact-owner status and attention evidence.
- Freeze session ordering from an activity snapshot on entry/explicit refresh. Background activity and selection do not reorder existing rows; closed history requires explicit Resume, and pending exact-source resumes coalesce safely.
- Read narrowly admitted official Claude/Codex CLI history without writing CLI-owned data; expose the dynamic official release directory with exact source and signed package admission.
- Add ordinary signed historical installation with independent complete backup and FreshSettings. The receipt proves installer handoff only, never installation completion; strict reviewed roundtrip and the upgrade-only updater retain their contracts.
- Verify actual process/user admission before disarming ordinary installer cleanup, retain unknown-outcome no-replay guards, and bound test execution with exact-source shard aggregation and disposable-child cleanup.

Validation preserves all original assertions and 35 original ignored tests. Hosted coverage separately discloses 18 Job-free and four unelevated integrations as unverified when their observed host requirements are unavailable. Real installed Windows interaction, native historical installation/return acceptance and arbitrary cross-version data compatibility are not certified. The user's specific Claude history cause was not directly established. CI performance research and its proposed cache/scheduling changes are separate from this release.

## [0.18.4] - 2026-10-10 (unreleased candidate)

- Keep session rows ordered by the last confirmed opening time; switching an already open terminal or receiving background output/status does not move rows or change the selected terminal
- Persist accepted new/reopened/restored ordering independently of activity, retaining project pin/alphabetical order and archive grouping
- Join repeated closes of the exact same attempt without a second stop or an automatic retry; preserve remaining-terminal selection, replacement guards and unconfirmed-stop refusal
- Show the update proxy endpoint and port while masking credentials, and add a bounded official-manifest proxy test with safe results/timing and stale-completion protection

This prepares a repair candidate, not a public release. Real installed Windows interaction and real CLI acceptance remain separate; existing release/source/signature gates stay required.

## [0.18.3] - 2026-10-09 (unreleased candidate)

- Keep pinned project groups first, then sort by displayed project name; session activity no longer changes project order, while recent sessions keep their ordering inside each project
- Resume the selected closed history directly and close the exact owned terminal without a second confirmation; preserve source/configuration/admission checks and cancellation on replacement or navigation
- Move Archive to a trailing quick button for ended/failed rows, remove the duplicate menu entry and keep existing running/unknown safeguards
- Reset Native terminal parser state safely on the first restart so old VT modes, partial control sequences, UTF-8 fragments and queued protocol replies cannot contaminate the new run; activation, recovery, theme changes and hidden terminals retain their content
- Restore historical working/idle/permission/completed/error/closed status distinctions, detail vocabulary, collapsed project counts and the global error/permission badge; share exact-owned window attention and acknowledge only visible, selected, focused non-error causes
- Show separate recent Native Claude event notices and unread reply-end receipts from the existing authenticated optional observer; bind and acknowledge the exact attempt, deduplicate bounded IDs, and request window attention without claiming current completion

Live Native completion/current activity still needs source-owned ordering and explicit turn evidence; observer defaults and unordered-event rejection stay unchanged. See `docs/session-status-semantics.md`. This candidate is not a public release; native platform/rendering and real CLI acceptance remain separate.

## [0.18.2] - 2026-10-09

- Add an optional HTTP/HTTPS proxy for update checks and downloads, separate from CLI launch environment settings; blank uses inherited process/system proxy behavior
- Show fixed update failure codes and stages for network, timeout, manifest, platform, source and signature failures, with upstream updater log details redacted
- Admit official signed releases through their exact tag/source, existing trust key and uploaded asset digests; retain the update in the backend for one confirmed installation after all session owners are quiescent
- Recognize image MIME metadata in clipboard file items for both terminal runtimes; route a single native CLI image-paste shortcut without reading or saving image data

Version preparation does not publish a release. Actual Windows updater installation/restart and real Claude/Codex image-paste acceptance remain separate platform checks.

## [0.18.1] - 2026-10-05 (unreleased candidate)

This entry summarizes changes since the last public release, 0.17.7, including the unreleased 0.18.0 test build. Version preparation does not mean a public release has been published.

### Workspace and CLI support
- Run Claude Code and Codex CLI side by side in one project/session tree, with independent launch configurations and registered projects; preserve existing Claude history through its compatibility adapter
- Replace separate workspace surfaces with one responsive Workspace, Projects and Settings shell, keeping terminal hosts and scrollback mounted across navigation
- Add quick new-session menus, advanced launch options, configuration editing, installed-program discovery and explicit executable confirmation
- Add a searchable cross-project history chooser and direct confirmation for the exact selected historical session; explicit resume of an ended open Claude session retains its original identity
- Add project registration, pinning, display names, hiding and removal with open-session safeguards; project removal preserves project files and CLI history
- Present instructions, settings, MCP, skills, agents and plugins as scoped, read-only resource summaries; provider, credentials and native CLI configuration remain outside Desk's management boundary

### Settings and interaction
- Unify General, Appearance, Terminal, Launch configurations, Shortcuts, Update and About settings in English and Chinese
- Add independent GUI appearance and shared terminal preferences, 16 terminal themes, configurable keyboard shortcuts and allowlisted About diagnostics
- Use shared accessible menus, dialogs, tooltips and notifications, with CLI application marks, circular session-status icons, keyboard navigation and responsive layouts
- Make the trailing × button the sole row/menu Close entry for open sessions, retaining the existing shortcut and exact-owner confirmation; remove duplicate Close/Stop menu entries
- Explain that Close terminates the owned process and clears its terminal display, distinguish saved CLI history from unsaved content, and select another open session or empty-workspace guidance afterward
- Make workspace source warnings dismissible while retaining bounded diagnostic details; improve native titlebar drag targets and centered empty-workspace guidance

### Reliability and security
- Authenticate native document, launch, terminal and resource access; freeze configuration/project/request/run identities and reject stale completions without falling back to the legacy PTY path
- Add ordered staged input, complete paste framing, evidence-based clipboard/IME handling, bounded output acknowledgements/backpressure and exact-owner process cleanup
- Keep uncertain launches and partial/unknown input or storage writes fail-closed, with read-only recovery instead of automatic replay
- Isolate optional observer metadata from process control and redact paths, credentials, prompt/output bodies and raw errors from public diagnostics
- Fix reactive-proxy launch validation, first-time profile revision handling, Windows inherited environment aliases, bounded large-transcript metadata reads and subagent-history filtering
- Add mixed-version storage protection, low-resource/fault coverage and reproducible Rust toolchain pinning for ordinary CI and Windows test packages

- Reduce all frontend log messages to a UTF-8 byte-count summary at the backend entry point; latch transport backpressure immediately at the high watermark
- Correlate native input receipts with frozen request identities and parser-batch mode epochs; show a fixed input-paused diagnostic without replaying pending input

### Historical versions and release status
- Add the official historical-release catalogue and signed-package download/verification, plus guarded Windows recovery foundations and an isolated recovery-manager interface
- Historical-version installation, switching and return-to-previous-version remain unavailable to ordinary users: the production roundtrip gate stays disabled pending real Windows install/start/return acceptance
- Keep the existing 0.18.0 roundtrip acceptance inputs pinned to their original version; they do not certify 0.18.1
- Retain signed-candidate-only automation and disabled public Release/updater promotion; automatic update installation remains unavailable without a trusted promotion contract
- Real Claude Code / Codex CLI Layer-C certification remains pending an authorized target environment. Host/unit CI and reviewed browser screenshots do not replace real CLI, native-platform or roundtrip acceptance

## [0.18.0] - 2026-10-01 (unreleased test build)

### Features
- Add the Native CLI workspace for Claude Code and Codex CLI with independent profiles, registered projects, native new/resume/raw launch entry, authenticated terminal control, and read-only native resource projections

### Reliability
- Add exact request/run/generation recovery, ordered staged input, bounded output ACK/backpressure, observer isolation, mixed-version rollback protection, and low-resource/fault stress coverage
- Keep ambiguous launch and partial/unknown input writes fail-closed without automatic replay

### Security
- Keep native UI behind the authenticated document bridge, remove legacy PTY fallback, redact native diagnostics, and lock native resource panels to projection-only behavior

### Changed
- Unify Claude Code and Codex CLI in one project/session workspace; preserve existing Claude history through its runtime adapter
- Localize Native CLI workspace controls in English and Chinese and expose both Claude Code and Codex documentation from About
- Align product, architecture, persistence, roadmap, and release documentation with the dual-CLI boundary
- Keep release automation candidate-only; public publishing remains disabled pending an explicit promotion design

### Fixes
- Copy validated native launch actions without cloning reactive store proxies, fixing new Claude and Codex session startup
- Refresh the shared workspace revision before creating a missing safe CLI profile, fixing sequential first-time setup of both CLIs
- Hand off selection after closing an active session and acknowledge failed action requests without discarding newer requests

### User feedback
- Use explicit native titlebar drag targets, corresponding CLI application marks and circular session status icons
- Remove project expand/collapse tooltip bubbles and center the empty workspace guidance

### Verification
- Record D22-D27 execution evidence in the repository
- Keep D20 real Claude Code / Codex CLI Layer-C certification BLOCKED until an authorized target environment supplies real evidence

## [0.17.7] - 2026-09-20

### Fixed
- Preserve complete pasted prompts on affected Windows systems by bundling a verified app-local Microsoft ConPTY runtime
- Stop startup instead of silently falling back when the bundled console runtime is missing or invalid

### Tests
- Add real Claude prompt-integrity coverage for large plaintext, JSON, consecutive pastes, and direct and production-shell launches
- Add installer, relocation, and fail-closed validation for the bundled runtime

## [0.17.6] - 2026-09-11

### Fixed
- Generate updater manifests from the resolved release tag instead of the triggering branch
- Reject invalid updater manifest tags before publishing release assets
- Mark each stable GitHub Release as the latest update source

### Tests
- Add CLI regression coverage for main-branch release builds

## [0.17.2] - 2026-08-30

### Fixed
- Pace large PTY writes on Windows to prevent ConPTY input loss during sustained paste bursts

### Tests
- Allow slow Windows runners enough time to verify 128 KiB paste transport integrity

## [0.17.1] - 2026-08-30

### Fixed
- Fix native paste interception while terminal elements are still initializing
- Prevent valid multiline JSON from being truncated through Windows ConPTY
- Preserve non-JSON multiline paste content and avoid duplicate paste dispatch

### Tests
- Add transport and Claude CLI paste regression coverage with shared fixtures

## [0.17.0] - 2026-08-30

### Features
- Add image paste forwarding when the clipboard contains no text, using the Claude CLI platform shortcut
- Add a shared design token system for consistent light and dark themes

### Fixed
- Normalize pasted annotation punctuation for consistent display
- Keep nested sidebar controls keyboard-accessible without toggling their parent item

### Changed
- Document the CC Desk visual system and product direction
- Add contrast and sidebar keyboard regression coverage

## [0.16.0] - 2026-08-17

### Features
- Add permanent deletion for archived sessions across projects
- Add persistent session-name indexing to speed up startup and history loading

### Fixed
- Fix long-paste rendering so carriage returns no longer hide earlier text
- Preserve project visibility and error details when session scans encounter invalid paths or I/O failures
- Keep cache state consistent after forced reloads fail

## [0.15.0] - 2026-07-21

### Features
- Integrate focus queue states directly into the session list
- Add per-session attention indicators and project-level status summaries

### Changed
- Remove the separate focus queue panel to streamline session navigation

## [0.14.0] - 2026-07-20

### Features
- Rebrand the independently maintained project as CC Desk
- Add a coral radial application icon with a subtle Chengdu bamboo detail
- Add multi-project navigation, project aliases, focus queues, and safer concurrent state updates
- Add GitHub-based signed updater manifests for Windows, macOS, and Linux

### Changed
- Document the original CC-Box source and preserve MIT attribution
- Keep `~/.cc-box`, `CC_BOX_*`, hook headers, and existing terminal theme IDs for upgrade compatibility
- Move application updates and release metadata to the independent CC Desk GitHub repository

## [0.13.1] - 2026-07-03

### Features
- Drop files inside the current project now insert relative paths into the terminal

### Fixed
- Disabling a user-scope plugin now hides its skills/agents/mcp from the sidebar panels immediately

## [0.13.0] - 2026-07-02

### Features
- Add user-level enable/disable toggle for skills / agents / mcp / plugins in sidebar (move to ~/.cc-box/disabled/ for skill/agent/mcp, claude plugin enable/disable for plugin)
- Add ToggleSwitch component with disabled state greyscale rendering
- Add multi-window safety: atomic file ops + conflict detection, optimistic update with rollback on failure

### Fixed
- Fix plugins panel filtering out disabled plugins, making them impossible to re-enable from UI
- Fix plugin toggle alignment in plugin header

## [0.12.8] - 2026-06-27

### Fixed
- Fix WebGL glyph corruption after long sessions by dispose + reload entire WebGL addon every 5 minutes (xterm.js designed renderer switch path, buffer untouched)
- v0.12.7 resize(rows-1)+resize(rows) was unreliable: xterm.js Buffer.resize is not lossless on row round-trip, caused duplicate last two lines
- Fix potential black screen on GPU context loss: onContextLoss now triggers full reload instead of dispose-only

## [0.12.7] - 2026-06-27

### Fixed
- Fix WebGL glyph corruption after long sessions: trigger real term.resize (rows-1 then restore) every 5 minutes to force full renderer rebuild. This matches the user-verified resize recovery path (clearTextureAtlas + refresh was unreliable and caused page distortion).
- Restore WebGL renderer for table/box-drawing continuity (revert v0.12.4 DOM renderer that showed visible cell gaps)
- Unify terminal disposal via disposeTerminal helper to prevent timer leaks

## [0.12.6] - 2026-06-26

### Fixed
- Fix WebGL glyph corruption after long sessions: combine clearTextureAtlas + term.refresh for full redraw (clearTextureAtlas alone caused page distortion during 5-min refresh)
- Restore WebGL renderer for table/box-drawing continuity (revert v0.12.4 removal that caused visible cell gaps in DOM renderer)
- Unify terminal disposal via disposeTerminal helper to prevent atlas timer leaks

## [0.12.5] - 2026-06-26

### Fixed
- Fix long-session glyph corruption (boxes with random ASCII letters) by periodically clearing WebGL texture atlas every 5 minutes, working around @xterm/addon-webgl@0.19.0 race condition bug (xtermjs/xterm.js#4325)
- Unify terminal disposal via disposeTerminal helper to prevent atlas timer leaks across tab close / restart / unmount

### Changed
- Restore WebGL renderer for better table/box-drawing continuity (DOM renderer showed visible gaps between cells)

## [0.12.5] - 2026-06-26

### Fixed
- Fix long-session glyph corruption (boxes with random ASCII letters) by periodically clearing WebGL texture atlas every 5 minutes, working around @xterm/addon-webgl@0.19.0 race condition bug (xtermjs/xterm.js#4325)
- Unify terminal disposal via disposeTerminal helper to prevent atlas timer leaks across tab close / restart / unmount

## [0.12.4] - 2026-06-25

### Fixed
- Fix terminal mojibake on long sessions via stateful PtyDecoder (UTF-8 priority + GBK per-segment scan)
- Fix multi-byte character corruption across PTY read boundaries (incomplete UTF-8 / GBK lead bytes)
- Fix xterm.js mixed CJK/emoji rendering: enable Unicode 11 width table, add emoji font fallback
- Fix terminal bottom row clipping caused by non-integer line height (fit precision)

### Changed
- ProviderCard always shows activate button (reactivate when already active) to reapply modified config
- Simplify read_output_loop from ~100 to ~50 lines using PtyDecoder
- Remove obsolete utf8_complete_boundary and utf8_seq_len helpers

## [0.12.4] - 2026-06-25

### Fixed
- Fix terminal mojibake on long sessions via stateful PtyDecoder (UTF-8 priority + GBK per-segment scan)
- Fix multi-byte character corruption across PTY read boundaries (incomplete UTF-8 / GBK lead bytes)
- Fix xterm.js mixed CJK/emoji rendering: enable Unicode 11 width table, add emoji font fallback
- Fix terminal bottom row clipping caused by non-integer line height (fit precision)

### Changed
- ProviderCard always shows activate button (reactivate when already active) to reapply modified config
- Simplify read_output_loop from ~100 to ~50 lines using PtyDecoder
- Remove obsolete utf8_complete_boundary and utf8_seq_len helpers

## [0.12.4] - 2026-06-25

### Fixed
- Fix terminal mojibake on long sessions: replace whole-buffer GBK fallback with greedy per-character scan (UTF-8 priority + GBK per-segment), so mixed UTF-8/GBK output no longer corrupts UTF-8 content
- Fix multi-byte character corruption across PTY read boundaries via stateful PtyDecoder
- Fix utf8_complete_boundary missing mid-buffer invalid bytes that triggered spurious GBK fallback

### Changed
- Introduce stateful PtyDecoder replacing carry buffer + boundary + decode_output triplet
- Rewrite decode_output with greedy scan: ASCII fast path, UTF-8 multibyte priority, GBK double-byte fallback, U+FFFD last resort
- Simplify read_output_loop from ~100 to ~50 lines
- Remove obsolete utf8_complete_boundary and utf8_seq_len helpers

## [0.12.3] - 2026-06-25

### Fixed
- Fix terminal mojibake on mixed UTF-8/GBK output: replace whole-buffer GBK fallback with greedy scan (UTF-8 multibyte priority, GBK double-byte fallback per segment, U+FFFD last resort)
- Fix multi-byte character corruption across PTY read boundaries (incomplete UTF-8 sequences and GBK lead bytes now buffered to next read)
- Fix utf8_complete_boundary missing mid-buffer invalid bytes by replacing with stateful PtyDecoder::find_safe_boundary

### Changed
- Introduce stateful PtyDecoder replacing carry + utf8_complete_boundary + decode_output triplet
- Rewrite decode_output with platform-unified greedy scan (remove #[cfg(target_os = "windows")] branch)
- Remove unused utf8_complete_boundary and utf8_seq_len helpers
- Simplify read_output_loop from ~100 lines to ~50 lines

### Tests
- Add 17 PtyDecoder tests covering cross-read UTF-8/GBK, isolated invalid bytes, flush, deadlock prevention, performance baseline (10KB < 100ms)
- Add 5 decode_output tests covering UTF-8+GBK mixing, isolated invalid bytes, 4-byte emoji, all-platform GBK

## [0.12.2] - 2026-06-22

### Fixed
- Fix terminal mojibake: PTY output now decodes via UTF-8 with GBK fallback, resolving black-block (U+FFFD) garble from Windows Chinese subprocesses (cmd.exe, git)

## [0.12.1] - 2026-06-17

### Fixed
- Fix restart tab failing with "Could not dispose an addon that has not been loaded" error when terminal element wait timed out before term.open()

## [0.12.1] - 2026-06-17

### Fixed
- Fix restart tab failing with "Could not dispose an addon that has not been loaded" error when terminal element wait timed out before term.open()

## [0.12.1] - 2026-06-17

### Fixed
- Fix restart tab failing with "Could not dispose an addon that has not been loaded" error when terminal element wait timed out before term.open()

## [0.12.1] - 2026-06-17

### Fixed
- Fix restart tab failing with "Could not dispose an addon that has not been loaded" error when terminal element wait timed out before term.open()

## [0.12.0] - 2026-06-17

### Features
- Add Claude CLI version history: list all available versions in Settings > Update with one-click install to ~/.local/bin/
- Support canceling in-progress Claude CLI downloads with partial-file cleanup
- Reuse local cached downloads when file size matches OSS record
- Switch startup check to local-only: no HTTP request to latest.json, just read installed version
- Maintain deps/claude/versions.json in OSS via download-deps.js with full version history
- Add rebuild-claude-versions.js to reconstruct versions.json from local releases
- Show Reinstall button for in-use version, Install for others

### Changed
- Sidebar update badge no longer driven by Claude CLI updates (CC-Box app updates only)
- Rename Download to Install and Installed to In Use in Claude CLI card
- Auto-detect running Claude process before install and prompt user to terminate

## [0.11.1] - 2026-06-16

### Fixed
- Fix terminal rendering glitches (floating characters, ghost overlap, misaligned CJK text) by enabling WebGL renderer (@xterm/addon-webgl)
- Restore per-platform font fallback: declare monospace CJK fonts on Windows/Linux to prevent cell-width miscalculation
- Remove padding interference on .xterm element that caused FitAddon column count errors

## [0.11.0] - 2026-06-12

### Features
- Add dark theme support with warm color palette
- Theme syncs across GUI, terminal (xterm.js), and CodeMirror editors
- Theme preference persists to config file
- SVG icons auto-invert colors in dark mode
- Enable dark theme option in Settings > Appearance

### Fixed
- Fix hardcoded colors in SVG icons (agents, mcp) to support theming
- Fix session panel action buttons text color in dark mode
- Fix empty-state button text color consistency

## [0.11.0] - 2026-06-12

### Features
- Add dark theme support with warm color palette
- Theme syncs across GUI, terminal (xterm.js), and CodeMirror editors
- Theme preference persists to config file
- SVG icons auto-invert colors in dark mode
- Enable dark theme option in Settings > Appearance

### Fixed
- Fix hardcoded colors in SVG icons (agents, mcp) to support theming
- Fix session panel action buttons text color in dark mode
- Fix empty-state button text color consistency

## [0.10.9] - 2026-06-10

### Features
- Add closeAllTabs/closeOtherTabs buttons to session panel
- Add plugin scope MCP support with env var expansion

### Improvements
- Refactor MCP loading from subprocess to direct JSON file reading
- Faster MCP panel loading without spawning claude process

### Fixed
- Fix MCP stdio client params null issue
- Fix MCP response id matching loop
- Add env injection support for stdio MCP servers

## [0.10.8] - 2026-06-03

### Fixed
- Fix session status staying working when Claude waits for tool permission (handle permission_prompt notification)
- Fix pending state incorrectly re-triggering when user switches tabs after work completes
- Fix recap/auto-compact after Stop incorrectly restoring working state (add turnEnded guard)
- Fix wrong notification_type mapping: add both permission_prompt and worker_permission_prompt
- Fix PreCompact/PostCompact events not registered in hooks.json
- Fix missing event data extraction for tool_name, agent_id, notification_type etc.
- Fix macOS build error in platform.rs (temporary value lifetime)
- Bump plugin version to 1.1.0 with PreCompact/PostCompact support

## [0.10.8] - 2026-06-03

### Fixed
- Fix session status staying working when Claude waits for tool permission (handle permission_prompt notification)
- Fix pending state incorrectly re-triggering when user switches tabs after work completes
- Fix recap/auto-compact after Stop incorrectly restoring working state (add turnEnded guard)
- Fix wrong notification_type mapping: add both permission_prompt and worker_permission_prompt
- Fix PreCompact/PostCompact events not registered in hooks.json
- Fix missing event data extraction for tool_name, agent_id, notification_type etc.
- Fix macOS build error in platform.rs (temporary value lifetime)
- Bump plugin version to 1.1.0 with PreCompact/PostCompact support

## [0.10.8] - 2026-06-03

### Fixed
- Fix session status staying working when Claude waits for tool permission (handle permission_prompt notification)
- Fix pending state incorrectly re-triggering when user switches tabs after work completes
- Fix recap/auto-compact after Stop incorrectly restoring working state (add turnEnded guard)
- Fix wrong notification_type mapping: permission_prompt did not exist in CLI source
- Fix PreCompact/PostCompact events not registered in hooks.json
- Fix missing event data extraction for tool_name, agent_id, notification_type etc.
- Bump plugin version to 1.1.0 with PreCompact/PostCompact support

## [0.10.7] - 2026-06-03

### Fixed
- Fix session status monitoring: only idle_prompt notification ends turn, not all notifications
- Fix recap after Stop: add turnEnded guard to prevent recap/internal ops from restoring working state
- Fix pending state not cleared when user is watching the tab
- Fix permission_prompt/worker_permission_prompt notifications: now correctly set pending while waiting for user approval
- Fix notification_type mapping to match actual Claude Code CLI values

### Features
- Add PreCompact/PostCompact hook event registration and monitoring
- Add structured data extraction for all hook events (tool_name, agent_id, notification_type, etc.)
- Add 37 Rust tests and 31 TypeScript tests for hook event processing and status monitoring

## [0.10.6] - 2026-05-23

### Fixed
- Fix macOS terminal blank content caused by non-monospace CJK font in xterm fontFamily
- Fix macOS update relaunch blocked by ACL (add process plugin permission)
- Fix macOS ARM64 auto-update platform key (darwin-aarch64)

## [0.10.5] - 2026-05-23

### Fixed
- Fix macOS update relaunch blocked by ACL (add process plugin permission)
- Fix macOS ARM64 platform key in auto-update (darwin-aarch64 instead of darwin-x86_64)

## [0.10.4] - 2026-05-23

### Fixed
- Fix npm-installed Claude CLI not detected on Windows (use cmd /C to support .cmd files)
- Fix macOS Apple Silicon detected as x64 (use sysctl instead of HOSTTYPE env var)
- Fix Linux ARM64 detected as x64 (use uname -m instead of HOSTTYPE env var)
- Fix installed Claude not having highest PATH priority after update
- Implement persistent PATH on macOS/Linux (write to ~/.zshenv or ~/.bashrc)

## [0.10.3] - 2026-05-21

### Fixed
- Add CJK font fallback (Microsoft YaHei, PingFang SC, Noto Sans CJK SC) for proper Chinese character rendering in terminal

## [0.10.2] - 2026-05-20

### Fixed
- Fix duplicated arrow and plus symbols in back and add-variable buttons

### Improved
- Add dedicated Custom Provider button in preset panel category row
- Add i18n support for Custom Provider label

## [0.10.1] - 2026-05-19

### Features
- Add Claude CLI update check from OSS in settings
- Auto-check Claude CLI updates on startup with badge notification
- Detect running Claude processes before update with confirmation dialog
- Kill all PTY tabs and Claude processes before updating

## [0.10.0] - 2026-05-18

### Features
- Add Windows Explorer right-click context menu: "Open with CC-Box" on folders and "Open CC-Box Here" on directory background
- Support opening project from CLI argument (cross-platform)
- Distinguish existing vs new project when opening from context menu

### Fixed
- Fix NSIS installer hook macro names for Tauri 2 compatibility

## [0.9.3] - 2026-05-18

### Fixed
- Fix working status not restoring after permission grant or plan confirmation
- Add missing always-on-top shortcut in settings shortcuts section
- Remove unused ShortcutsModal component

## [0.9.2] - 2026-05-17

### Features
- Add always-on-top toggle with pin button in terminal header (Ctrl+Shift+T / Cmd+Shift+T)

## [0.9.1] - 2026-05-16

### Fixed
- Fix session resume from home page not opening the correct session due to watcher race condition
- Fix active tab switching to wrong tab when returning to the same project
- Fix history sessions not refreshing after new session starts

### Improved
- Refactor history sessions to per-project caching for faster project switching

## [0.9.0] - 2026-05-15

### Features
- Add one-click update with custom confirmation dialog when active PTs detected
- Add manual download button opening GitHub Releases page
- Add i18n support with Chinese/English language switching
- Add useTimeFormat composable for localized relative time display

### Fixed
- Fix session name using last message instead of first user message
- Fix history session list reloading on resume (unnecessary full reload)
- Fix UI lag when closing tabs by deleting tab before async PTY kill

## [0.7.0] - 2026-05-14

### Fixed
- Add tests for startup environment variables (frontend 5, backend 6)
- Fix Windows CI: configure MSVC linker for windows-2022 runner

## [0.6.5] - 2026-05-14

### Fixed
- Add tests for startup environment variables (frontend 5, backend 6)

## [0.6.5] - 2026-05-14

### Fixed
- Add tests for startup environment variables (frontend 5, backend 6)

## [0.6.4] - 2026-05-12

### Fixed
- Preserve user startup options (skipPermissions, customArgs) when switching projects

### Changed
- Change Claude CLI auto-install path to standard .local/bin directory

## [0.6.3] - 2026-05-12

### Fixed
- Fix update info sync between sidebar and update store on startup
- Settings panel now shows update info immediately without re-check

## [0.6.2] - 2026-05-12

### Fixed
- Fix pending status not showing on home page Recent Sessions
- Fix pending incorrectly cleared when user is on home page
- Fix pending badge showing for all projects instead of current project only
- Show working/pending status indicators on home page session list
- Improve update check error display with specific message

## [0.6.1] - 2026-05-11

### Features
- Show running status dot for active sessions in Recent Sessions
- Show running session count on projects
- Merge active tabs into Recent Sessions list

### Fixed
- Fix resume session not opening when returning to same project
- Fix clicking already-running session creating duplicate PTY
- Fix historySessions duplicate keys Vue warning
- Fix readonly computed assignment error in TerminalView

## [0.6.0] - 2026-05-11

### Features
- Add auto-install system for Claude CLI and Git portable from OSS
- Add dependency download script for OSS distribution
- Add install progress UI with cancel support
- Unified dependency management for better first-run experience

### Changed
- Improve startup checks with install detection
- Add installer module to handle downloads and PATH setup

## [0.5.2] - 2026-05-10

### Features
- Add automated release script with full workflow
- Switch update system to Alibaba Cloud OSS for better China access
- Add download cancellation support
- Unify version management from package.json

### Changed
- Remove GitHub Actions workflow (migrated to local script)
- Improve update UI with manual download and cancel options

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
