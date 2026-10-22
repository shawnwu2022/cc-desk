# Release highlights

[简体中文](RELEASE_NOTES_CN.md). Major user-facing changes only. See [CHANGELOG.md](CHANGELOG.md) for the complete engineering history.

<!-- Generated from docs/release-notes.json by node scripts/release-notes.js docs. -->

## [0.24.0-rc.4]

Fixed

- Start fresh settings with only Claude Code enabled, while preserving every saved tool preference. Restore compact icon-only tool switches with reliable enabled/disabled brightness.
- Apply OpenCode support changes to session history immediately, without refreshing or stopping open terminals.
- Rename eligible OpenCode conversations through the official metadata API and retain their titles after restart. Hide rename actions when the source cannot be safely verified.
- Show OpenCode activity through the shared session badges, distinguishing working, idle, permissions, questions, errors and unknown state. Preserve identity and status across delayed events and clean up disposed observers.

Important

- This is a prerelease test package; the stable updater channel remains unchanged.
- Optional OpenCode activity observation is enabled only for the examined official CLI 1.19.0 contract. Unrecognized versions skip this enhancement without adding a launch requirement; compatibility with every future CLI is not guaranteed.
- Isolated official CLI metadata/plugin checks did not call a paid model. Account-backed model turns and installed Windows WebView acceptance have not been certified for this source.

## [0.24.0-rc.3]

Unpublished candidate

- This candidate was not released because its main CI did not pass. Its provider settings and OpenCode fixes continue in 0.24.0-rc.4.

## [0.24.0-rc.2]

Test release

- Add OpenCode as a third native CLI with owned terminal launches, exact-ID resume and project-scoped SQLite history.
- Improve Claude Code session identity after delayed hooks, Codex command-wrapper compatibility, active session filtering and focused keyboard shortcuts.
- Keep optional OpenCode session preparation and history safe when the CLI, database schema or capability probe is unavailable.

Important

- This is a prerelease test package. Download and run it explicitly; the stable updater channel remains on the latest stable release.
- OpenCode integration was checked against official CLI 1.19.0 without paid model calls. Account-backed create/resume and installed Windows WebView acceptance remain separate checks. Older or future CLI versions may lose optional history or title features while basic terminal launch remains available.

## [0.23.5]

Fixed

- Restore new and resumed Codex sessions launched through Windows npm command wrappers.
- Keep automatic Codex titles and a single history entry when using the supported official npm wrapper.
- Apply launch-configuration changes without restarting CC Desk.

Important

- Codex title synchronization supports the official npm 0.163.0 wrapper at its direct path, with NODE_OPTIONS unset or empty. Other wrappers or Node options can still launch, but title synchronization and history deduplication may be unavailable.
- Historical-version rollback failures remain under investigation.

## [0.23.4]

Added

- Quiet background update checks, with a notification in Settings when an update is available. You still choose when to install.

Improved

- New configurations start with Claude Code and its activity updates enabled. Existing CLI and observation preferences are preserved.

Fixed

- Restore Claude session activity updates and show Rename only for conversations that can be renamed.
- Show Codex automatic conversation titles and keep one session entry after refreshing.
- Keep dismissed history warnings consistent across the session list, notices and details.
- Let F2 follow session-list focus, leaving terminal function keys to the CLI.

Important

- Claude GUI rename requires Node.js 22.13 or newer on the launch configuration's PATH and readable local history. Codex title updates require compatible CLI support and may be unavailable when CLI hooks are disabled.
- Historical-version switching supports the approved Windows releases. Shared CLI settings, conversation history and project files are not rolled back; back up important data before switching.

## [0.23.3]

Fixed

- Keep the terminal's last row visible when resizing the window or changing fonts.
- Improve history refresh and recovery notices, and restore rename availability for readable conversations.
- Use the CC Desk application icon in the Windows installer.

Important

- Unreadable or damaged history sources can still leave the list incomplete. Resolve the file-access problem, then retry the affected source.
- Historical-version switching does not roll back shared CLI settings, conversation history or project files; back up important data before switching.
