# Release highlights

[简体中文](RELEASE_NOTES_CN.md). Major user-facing changes only. See [CHANGELOG.md](CHANGELOG.md) for the complete engineering history.

<!-- Generated from docs/release-notes.json by node scripts/release-notes.js docs. -->

## [Unreleased]

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
