# Release highlights

[简体中文](RELEASE_NOTES_CN.md). Major user-facing changes only. See [CHANGELOG.md](CHANGELOG.md) for the complete engineering history.

<!-- Generated from docs/release-notes.json by node scripts/release-notes.js docs. -->

## [0.27.0-rc.1]

Added

- Choose Stable, Test or Preview updates. Stable remains the default; returning to Stable never silently downgrades. Updates show their current stage, support bounded cancellation, and reject stale or untrusted candidates.
- Browse historical release information and manual recovery guidance. Ignore one exact candidate without hiding later versions, and review local update history or retry a failed check.
- Copy local redacted diagnostics, including bounded version probes for the selected CLI profile. Nothing is uploaded automatically.
- Permanently delete eligible archived Codex roots on Windows with a preview, two separate confirmations and explicit irreversible-risk warnings. The native root and its execution-time subtree are the scope; the preview does not claim a complete frozen descendant list. Active or unproven sources are refused. Unknown outcomes are checked before any retry.

Fixed

- Restore native Ctrl+V and paste handling in structured Windows Codex sessions without forwarding clipboard text or replaying uncertain input.
- Validate compressed Codex history when the native API reports a missing logical .jsonl path, while preserving source and session identity. Metadata rename requests explicit login and makes no model call.
- Keep invalidated delete previews queryable before dispatch. Refresh the original source after a delete is acknowledged, including through read-only reconciliation after switching targets, and preserve unrelated history rows.

Important

- This is a prerelease. Stable Latest remains unchanged. Claude and OpenCode permanent deletion remain disabled because their source or writer protection is not yet sufficient.
- Isolated product-backend deletion of one synthetic original was verified with Codex 0.167. The permanent-delete document IPC and visible WebView path remain unverified after a blocked test-instance lifecycle. Full descendants and partial failures, Unix, installed application behavior, D20/native All, real AI naming and signed upgrade/return roundtrips remain unverified. These limits are not certified by updater signatures; Windows Authenticode is also separate.

## [0.26.0]

Fixed

- Retain pending Codex identity until an authenticated first event arrives, and preserve exact-ID title/history refresh when a later event reports a previous identity. Never merge sessions by matching titles or timestamps.
- Accept one leading UTF-8 BOM at the Codex Hook reporter input boundary after the original raw 64 KiB limit. Preserve JSON/metadata validation, authentication and privacy filtering.
- Adapt supported Windows Codex launch routes and terminal capability, and retain the OpenCode observer without an auxiliary version probe.
- Read migrated OpenCode v2 history with nullable titles through the selected CLI database source. Create and rename supported sessions through authenticated official metadata APIs with exact readback, and refresh eligible history after database/WAL changes while respecting pure mode.

Important

- This stable release promotes the RC1-RC3 fixes to Latest and the stable updater as 0.26.0. The separately developed 0.27 update/recovery features and remote/mobile work are not included.
- Windows Hook regression uses synthetic events through the actual PowerShell wrapper, production reporter and authenticated local intake. The observed BOM prefix is supplied in the synthetic input on this development host; hosted test subprocesses use system PowerShell Modules only.
- Full installed Windows WebView, the original live duplicate-row case, account-backed model turns, D20 Layer-C, original unfiltered native All and real AI automatic naming remain unverified. Updater signatures do not certify Windows Authenticode or real installation/return acceptance. No universal duplicate-row fix or future CLI compatibility is claimed.

## [0.26.0-rc.3]

Fixed

- Accept one leading UTF-8 BOM in Codex Hook reporter input, including the prefix observed in a Windows PowerShell transport. Keep the original raw 64 KiB input limit, JSON and metadata validation, authentication and privacy filtering.

Important

- This is a prerelease test package. Download it explicitly; stable Latest and the stable updater channel remain unchanged.
- Regression checks exercise the production reporter, actual Windows PowerShell wrapper and authenticated local intake for SessionStart, UserPromptSubmit and Stop, including space, Chinese and apostrophe paths plus direct launch. This development host does not add a BOM itself; the observed prefix is supplied in synthetic input through the real chain.
- No real user conversation, model call or installed application restart is used. The reported live duplicate-row case, full installed Windows WebView, D20 Layer-C, original unfiltered native All and real AI automatic naming remain unverified. This correction does not certify every cause of duplicate rows.

## [0.26.0-rc.2]

Fixed

- Keep Codex title and history refresh active when an authenticated turn event reports a previous session ID, while retaining the current exact identity.
- Read migrated OpenCode v2 session history, including unnamed sessions, and resolve the selected CLI's database path without silently showing an old channel database.
- Create and rename supported OpenCode v2 sessions through the authenticated official metadata API. Accept an initially absent title and require exact title readback after a single rename request.
- Refresh OpenCode v2 history after database or WAL changes for an eligible owned resumed session. Treat these changes only as refresh cues; optional observation remains disabled in pure mode.

Important

- This is a prerelease test package. Download it explicitly; the stable Latest release and stable updater channel remain unchanged.
- OpenCode 2.2.5 metadata checks use isolated local roots without paid model calls. Database refresh cues do not certify live activity badges or account-backed model turns. Full installed Windows WebView, D20 Layer-C and real AI automatic naming remain unverified.
- The earlier field report of two rows with distinct native IDs is not fully certified fixed. Session identity remains exact; titles and timestamps never merge different sessions. Unsupported or future CLI contracts may lose optional enhancements.

## [0.26.0-rc.1]

Preview release

- Keep Codex identity pending until an authenticated event arrives, so a delayed first event does not falsely report that session linking is unavailable. Preserve exact-ID titles and history projection during refresh and close; restored runs still need a new authenticated event.
- Adapt the supported Windows Codex npm and canonical native launch routes by their same-run structure and authenticated events instead of a CLI version string. Observe OpenCode in the selected run without a version probe; retry at most twice after transient loopback transport failures using the same authenticated event ID.
- Declare the interactive PTY's terminal capability to avoid an inherited TERM=dumb warning. Keep Claude Code behavior unchanged.

Important

- This is a prerelease test package. It does not become the stable Latest release or an automatic stable update.
- The earlier field report of two rows with different native IDs is not root-caused or certified fixed. Never merge sessions by matching title or time. A diagnostic build completed one new-session title, refresh, close and restore sequence; the restored run's identity event, full installed-app flow and real AI automatic naming remain unverified.
- Arbitrary custom wrappers, legacy native layouts, Unix identity enhancement and future incompatible CLI protocols are not newly certified. An unsupported identity capability remains visible rather than silently inventing an association.

## [0.25.2]

Fixed

- Refresh Codex history titles after an idle transition, including delayed naming when no Stop hook arrives.
- Preserve readable names when a Codex title index has a partially appended record or exceeds the normal read limit; retain already observed same-source titles during incomplete close-time reads.

Important

- Identity hooks and supported launcher requirements remain unchanged. Sessions missing startup identity still require a supported launch or resume.
- Cold reads cannot recover older names outside the bounded large-index tail, or names saved only in Codex SQLite after index persistence fails. Title writes beyond the bounded retry window require another refresh cue.
- The reported field trigger, complete installed-app behavior and real AI automatic naming have not been certified.

## [0.25.1]

Fixed

- Restore authenticated session identity for the audited official Windows npm Codex CLI 0.164.0 wrapper, allowing newly started sessions to follow CLI history titles and fold their exact history entry.

Important

- Start a new Codex session after updating. Existing sessions launched without identity hooks cannot acquire them by refreshing the list.
- Identity synchronization requires the supported direct-path npm 0.163.0 or 0.164.0 wrapper, enabled CLI hooks, and NODE_OPTIONS unset or empty. Native-executable and custom-launcher admission remains unchanged.
- Complete installed-app behavior and real AI automatic naming have not been certified for this patch.

## [0.25.0]

Added

- Toggle the session list with Ctrl+B on Windows/Linux or Cmd+B on macOS. Terminal input, editing fields and existing shortcut bindings keep priority.

Improved

- Update the Vue interface and native Tauri/TLS dependencies while preserving compact controls, saved settings and application permission boundaries.
- Strengthen automated regression checks and optional CI timing measurement without reducing the required test inventory.

Important

- Existing CLI model/provider settings and native session ownership remain unchanged. Optional CLI enhancements still depend on supported official interfaces.
- Account-backed model turns, D20 Layer-C, original native All and installed Windows WebView acceptance have not been certified for this source. Some Rust dependency advisories remain under review; no universal security or compatibility certification is claimed.

## [0.24.0]

Added

- Use OpenCode alongside Claude Code and Codex CLI for native new sessions, exact-session resume, project preferences and local history, while keeping CLI model and provider settings in the original tools.

Fixed

- Start fresh settings with only Claude Code enabled and retain saved tool preferences. Keep compact icon-only switches and apply OpenCode history visibility immediately without stopping open terminals.
- Rename eligible OpenCode conversations through the official metadata API, verify the persisted title and hide unavailable rename actions. Show supported activity through the shared session badges, retaining distinct idle, working, permission, question, error and unknown states.
- Read OpenCode SQLite history coherently with WAL/SHM and honor safely scoped custom databases. Reject incomplete or invalid sources without silently losing older history or replaying an uncertain session-creation request.
- Keep newer Claude SessionStart identities when delayed Stop events arrive. Retain Codex title synchronization, exact history deduplication, active-session filtering and session-list keyboard focus fixes.

Important

- This is the stable 0.24.0 release. Installation remains your choice when an update is offered.
- Saving an OpenCode profile updates the workspace document. Older CC Desk versions can reject that document with WORKSPACE_INVALID; historical-version switching does not roll back shared CLI settings, history or project files.
- Optional OpenCode activity observation uses the examined official CLI 1.19.0 contract. Unrecognized versions skip it without making it a launch requirement; compatibility with every future CLI is not guaranteed.
- Account-backed model turns, D20 Layer-C, original native All and installed Windows WebView acceptance have not been certified for this source. Existing dependency advisories are unchanged by this release preparation.

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
