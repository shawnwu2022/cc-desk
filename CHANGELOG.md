# Changelog

## [Unreleased]

## [0.23.4] - 2026-10-20

### Added

- Check for verified desktop updates quietly in the background and show available updates in Settings, while keeping installation user initiated
- Maintain concise English and Chinese release highlights for the application and GitHub, with English as the publication default and complete technical validation disclosure retained in a collapsed section

### Fixed

- Enable Claude Code and its observer for new configurations while preserving saved CLI and observation preferences
- Restore quoted-path Claude activity hooks and exact-source GUI rename admission, including readable conversations whose later working directory differs from their original project
- Bind Codex session hooks to the owned run, synchronize automatic conversation titles and avoid duplicate closed history rows after refresh
- Preserve dismissed history warnings consistently across session lists, notices and details, including partial source failures
- Scope F2 rename and session-list navigation to the focused surface so terminal function keys reach the CLI, and hide Rename when its exact target cannot be modified

Local validation passed the complete Vitest inventory (3,022 cases, two original skips), 66 visual-fixture cases, type checking, strict Rust Clippy, 21 desktop-updater Rust cases, 254 Node-policy cases (six platform-conditional skips) and both English/Chinese production-CSP startup cases. Focused real-xterm keyboard checks passed; isolated real Claude and Codex CLI probes exercised production hooks and the bundled Windows ConPTY without changing user profiles or sessions. These checks do not establish installed Windows WebView2, account-backed D20 certification, unfiltered native All or installation/return acceptance. Publication still requires the exact merged-main ordinary CI, authenticated coverage and all three same-run signed updater payloads.

## [0.23.3] - 2026-10-19

### Fixed

- Fit the real Native CLI terminal inside its visible host so the last row remains complete across cell-height boundaries, fractional DPR, font changes, hide/show and window resizing
- Configure the NSIS installer shell to use the approved application icon
- Preserve actionable, bounded history-source diagnostics and recover readable history without exposing raw upstream errors
- Restore exact-source native session rename admission and use one Rename action with precise target and failure messages
- Refresh only the affected CLI when support is toggled, publish readable profiles despite another source's failure, update Legacy read warnings on failure/success/disable, retain opened terminal titles, and prevent late refreshes from reviving archived or duplicate history rows
- Reload open-tab-only projects, refresh all Legacy projects on root retirement, keep disabled-source projections free of root validation, and discard stale exact diagnostics on retained cached history
- Keep disabled-source warnings hidden across full refreshes and scope diagnostic retries, archive restores and recovery guidance to the original history source
- Fold closed archive aliases only with complete physical-source proof, keep distinct opened owners, and preflight all archive aliases before per-write source ownership checks

Local validation of the final repair chain passed 2,823 fast Vitest cases, type checking and the production build. Earlier focused validation passed the six real-xterm/FitAddon Chromium layout cases at DPR 1/1.25/1.5, 20 native rename cases and 172 projection cases; an uninstalled debug NSIS bundle's seven PE icon frames matched the approved ICO. These are source/component and synthetic-history checks, not installed Windows WebView2, live Claude/Codex last-row acceptance or native installation/return proof. Final source-bound required CI, authenticated coverage and all three same-run signed updater payloads remain mandatory before publication; unfiltered native All and D20 remain separately unverified.

## [0.23.2] - 2026-10-19

### Changed

- Adopt the approved stacked-terminal CC Desk icon in the title bar, About page, browser favicon and Windows/macOS/Linux packaging, preserving the separate Claude Code and Codex CLI session marks

### Fixed

- Preserve the approved icon colors in dark mode when small PNGs are inlined by the production build

Local validation passed all 19 approved source hashes, nine transparent PNGs, seven exact ICO frames, ten exact ICNS representations, deterministic regeneration, 47 targeted component tests, type checking and 82 release-policy fixtures. Both English and Chinese production-browser cases passed under the actual CSP, including favicon bytes and light/dark titlebar/About rendering; the local run used an alternate port because a previous test server could not be stopped by the available Windows identity. Installed taskbar/Start menu, macOS Finder/Dock, Linux icon caches, OS fractional scaling, native All and D20 remain separately unverified. Exact-main CI, authenticated coverage and three same-run signed updater payloads remain mandatory before publication.

## [0.23.1] - 2026-10-19

### Fixed

- Refresh only affected session-history projections and preserve unchanged rows and arrays instead of repeatedly refreshing the complete session list
- Keep trusted source roots, titles and archive state together during reloads and read failures to prevent transient archived or duplicate session rows
- Reproject dependent archive/folding states when native-ID uniqueness or a shared root proof changes, and normalize legacy archive keys to prevent repeated rescans
- Read the latest valid Claude custom title, then AI title, then first prompt consistently across history lists, search, details and opened sessions; invalidate older derived name-cache entries
- Preserve title updates containing separators by encoding row projections structurally instead of concatenating unescaped title fields
- Follow idle Claude CLI `/rename` changes in opened Native and Legacy sessions through coalesced, owner-bound title invalidation without periodic history scans
- Keep source-proven CLI title updates working when the optional hook observer is off or unavailable, and retain complete Legacy history until every replacement page succeeds
- Replace a new session's preparation row as soon as its runtime owner is admitted, before opening-time metadata finishes saving; prevent late saves from reinserting closed sessions
- Make compact density visibly tighten shared controls, project/session rows and workspace spacing through geometry tokens while preserving terminal fonts and control hit targets
- Resolve locked npm tarballs through the official registry instead of fixed regional mirrors while preserving every dependency version and integrity hash

Local validation passed 2,864 Vitest cases with two existing skips using four workers, type checking and the production build. The original concurrent full run hit the unchanged 60-second stress timeout; the unchanged 5,000-session, 30-owner, 120-change stress inventory passed separately and in the four-worker full run. Real source-build WebView2 observations covered close/resume/archive/restore with no full catalog refresh or unrelated row replacement, and real Claude Code 2.1.296 PTY checks confirmed idle rename title notifications and metadata writes. The final post-fix WebView2 interaction/visual rerun remains unverified because desktop capture/input automation failed. Exact-main CI, authenticated coverage and all three same-run signed updater payloads remain mandatory before publication; unfiltered native All/D20 and native historical installation/return remain separately unverified.

## [0.23.0] - 2026-10-18

### Features

- Add session-row dragging to insert a single-line session reference into the active terminal without submitting the prompt

### Changed

- Preserve unchanged project-group identity so a session status update repaints only the affected project
- Coalesce runtime status and attention refreshes for affected projects while preserving complete history and archive projections

### Fixed

- Keep session-reference dragging compatible with Windows native file drops through document pointer events, with cancellation and click suppression
- Refresh restored Legacy sessions after a lost acknowledgement even when restoring the last archive removes its project key
- Escape every terminal control character in session-reference fields, including DEL and C1 controls
- Preserve ordinary click focus and keyboard menus before a session drag starts
- Retry late AI titles within a bounded window and stop on a generated title, changed owner or newer interaction

The complete Linux Chromium no-update visual gate passed all 241 cases in PR60 run 39120741597, and the Windows package installation/relocation gate passed in run 39120741531. The final local full Vitest run passed 2,803 cases with two existing Windows skips. Exact-main CI, authenticated coverage and all three same-run signed updater payloads remain mandatory before publication. Installed Windows WebView2 drag/file-drop behavior, real CLI input, unfiltered native All/D20 and native historical installation/return remain separately unverified.

## [0.22.0] - 2026-10-18

### Changed

- Group mixed-CLI projects under collapsible Claude Code / Codex CLI headers, with per-group status counts and keyboard navigation that skips collapsed rows
- Keep session creation order stable across activity, history refresh failures and restarts; use CLI creation timestamps when available and restore CLI archives before clearing Desk archive markers
- Close three creation-time boundary defects: a Codex thread's creation time no longer depends on whether the current scope registers the project owning an older rollout, a Claude transcript whose first stamped record is an `isMeta` message no longer reports a different creation time under complete versus bounded reads, and an over-long or NUL-bearing timestamp candidate is no longer published (one undecodable optional field rejected the whole history response)
- Drop ten never-imported frontend dependencies (CodeMirror stack, `vue-codemirror`, `v3-infinite-loading`, `macos-traffic-lights`, `vue-draggable-plus`) and the dead `editor-vendor` chunk rule; DESIGN.md now describes the `<textarea>` the app actually uses for multi-line editing
- Move blocking store reads (`get_projects`, `get_project_info`, `get_session_count`, `get_session_details`, `search_session_messages`, `get_app_config`, `get_project_config` and the four Agents/Skills/MCP/Plugins resource scans) onto the blocking pool instead of occupying async IPC workers
- Debounce the native terminal's `ResizeObserver` fit by 80 ms so dragging the window produces one ConPTY resize instead of one per frame; hidden terminals still skip layout work
- Reuse unchanged session-row projection objects when another row's status changes

### Fixed

- Expand the target CLI group when creating a session through the shortcut so the new row stays visible
- Forward native Codex Ctrl+V only on Windows when a bounded executable version probe identifies `codex-cli` 0.163.0 or later; retain the existing clipboard fallback for older, unidentified or unavailable executables
- Preserve one session row when an opened session replaces its history projection
- Reveal forward-slash project paths correctly in Windows Explorer

The complete Linux Chromium no-update visual gate passed all 237 cases in PR58 run 39053275403. Exact-main CI, authenticated coverage and all three same-run signed updater payloads remain required before publication. Installed WebView2, old Codex 0.162.x paste behavior, unfiltered native All/D20 and native installation/return remain separately unverified.

## [0.21.0] - 2026-10-16

### Fixed

- Preserve one physical history row across launch configurations after native rename, concurrent refresh, close, resume and application restart; retain distinct source roots, archive groups and opened process owners
- Restore original-session restart continuity from frozen source provenance instead of silently creating another conversation; reject missing, changed or ambiguous identities
- Separate close and archive hit targets, refresh titles after the first authenticated prompt, retain unread reply attention for folded projects, and restore project-wide close with exact-owner confirmation
- Include user-visible Codex code-review main threads while retaining background-agent filters and exact project association
- Explain observer installation/availability and permission/failure reasons without inventing activity, preserve explicit observer choices, and keep closed rows free of status badges

### Features

- Add verified native Claude rename through pinned official Agent SDK 0.3.301 and Codex rename through its official app-server metadata API, preserving the full interactive TUI and unsubmitted drafts; write once, read back, and never replay unknown outcomes
- Add transactional session-index rebuild with generation publication, write barriers and recoverable unknown-result checks instead of deleting history or configuration
- Show explicit resumed-source or historical-record rename targets and actionable runtime prerequisite errors; refresh exact physical projections and clear only still-matching aliases after native success

Claude GUI rename requires an existing Node.js 22.13+ on the launch configuration PATH, a writable configuration root supporting same-volume hard links, and plain JSONL history. The helper is embedded in the binary with SDK license notices; Node is neither bundled nor automatically installed. Missing prerequisites affect GUI rename only; the existing CLI remains usable and `/rename` in its terminal is the manual fallback. Real idle Claude 2.1.296 rename/resume was checked with synthetic history and no model turn. Busy-writer concurrency, incident-version 2.1.301, installed WebView2, unfiltered native All/D20 and native installation/return remain separately unverified. Cross-platform hosted compilation, exact-main CI/coverage and all three actual updater signatures remain mandatory release gates; local fixtures do not certify those targets or the original incident root cause.

## [0.20.2] - 2026-10-16

- Read Codex `.jsonl.zst` history through bounded streaming decompression so compressed, unarchived sessions appear again; preserve archive containers, exact source/project identity, thread deduplication and corrupt-entry diagnostics
- Keep folded Native display names bound to the adapter's exact history or tab identity; saved names from another root, profile, revision or registered project cannot replace them through a matching UUID
- Fold duplicate Claude history rows again, but only when the backend proves the native source root is the legacy fixed root (`~/.claude`); hiding the closed native twin of an open legacy terminal requires the same proof plus that terminal's own frozen launch evidence, derived from the same environment snapshot written into the command (a `CLAUDE_CONFIG_DIR` override at spawn time, or a `BASH_ENV` startup script that could set one, disables it; a later config change does not retroactively apply to a running terminal). Unknown or different roots keep both projections with their own archive/restore owners
- Carry a folded-away legacy row's own evidence onto the surviving native row: Desk archive entries (legacy stores the bare session id, which the native adapter now recognizes under the same root proof) and the explicitly saved Desk name/opened time (a running native terminal's own saved name, which lives under its history key, still wins). An open terminal keeps its non-archived projection, and resolving a restore settles every origin-ambiguity check before the first write so an ambiguous restore leaves archive metadata untouched
- Reproject the session catalog when the legacy root proof arrives after the first history load, so duplicate rows do not persist until an unrelated refresh
- Show the "history may be incomplete" notice only when entries were actually omitted (read failures), not for every normally bounded Claude observation; entries skipped because no project association could be observed now count as omissions instead of only raising the completeness flag
- Expose the legacy Claude root identity to the frontend via a new read-only command used for the folding proof
- Revalidate the initial physical Legacy root for every projection and bare-ID restore, retire its folding lease after replacement or failed validation, preserve Legacy archive restore owners when Native origins are ambiguous, and revoke prior launch proof before publishing a new PTY generation. Interactive Bash, PowerShell profiles and configured Windows `USERPROFILE` overrides keep their actual root unknown

Windows source-build acceptance confirmed 13 Codex history rows across five projects in the real WebView2 session tree and verified search filtering. Archive drawer interaction, real CLI resume, cross-platform manual acceptance and D20 certification remain unverified; required main CI, authenticated coverage and three-platform signatures remain separate release gates.

## [0.20.1] - 2026-10-15

- Preserve XML-leading user prompts as Claude history titles, retain early tab display names when authenticated history identity arrives, and keep newer known display timestamps across partial reads of the same exact source
- Enable the observer for newly auto-created safe Claude templates; preserve all saved observer choices, including existing templates, without refresh-time profile writes
- Preserve separate Legacy/Native history and archive/restore owners when their source roots cannot be proven identical; a matching session UUID is insufficient to fold them
- Show observed record timestamps for bounded Claude history reads so closed rows keep a display time instead of “—”
- Project sessions without an observation channel (observer off, Codex, connecting) as a neutral running state instead of “Activity unknown”
- Read Claude history until a real title source is observed instead of stopping at `cwd`, widen the bounded title tail to 64 KiB, and drop the short wall-clock gate; only completely observed exit-only sessions without user tasks, assistant/tool work or explicit titles are omitted
- Refresh CLI titles after authenticated Native Claude identity/reply-end receipts, coalescing affected projects per profile; resumed tabs follow the exact source and ordinary output/status/focus changes do not rescan history
- Attribute Claude history rows to the first cwd observed on a supported record type and ignore cwd on unsupported record types, so the legal per-turn cwd changes of a real transcript (subdirectory navigation, letter casing) no longer mislabel a whole source `SOURCE_AMBIGUOUS` once the widened observation window reaches them

Protected-main CI, authenticated exact-source coverage and all three same-run signed platform artifacts remain required before publication. Automatic title refresh shares the existing 1,024-event observer budget; explicit history reads remain available after exhaustion. Real CLI title/rename acceptance, desktop WebView2, unfiltered native All/D20, native historical installation/return and cross-platform manual acceptance remain separately unverified.

## [0.20.0] - 2026-10-14

- Keep session rows in stable first-seen order, cluster Claude and Codex sessions without headings, move Settings to the bottom navigation anchor, and exclude exec-only Codex history
- Handle file drops through the unified terminal host for Legacy and Native sessions while retaining exact runtime ownership
- Save GUI renames as Desk display metadata for running and closed sessions, preserve unsubmitted CLI drafts, and retain exact-source cancellation and stale-completion guards
- Distinguish closed Codex artwork in both themes and suppress closed-row badges while preserving accessible labels and active session states
- Restore closed-history display-name editing without selecting or resuming a terminal, retain admitted rename drafts during queued writes and owned save failures, and reject complete dropped-path batches containing terminal control characters before either input writer
- Adopt all thirteen independently accepted Linux Chromium visual baselines with exact byte provenance; fresh PR51 no-update verification passed all 236 cases without changing tolerance

Protected-main CI, authenticated exact-source coverage and all three same-run signed platform artifacts remain required before publication. Real CLI rename acceptance, desktop WebView2, unfiltered native All/D20, native installation/return and cross-platform manual acceptance remain separately unverified. Historical strict Claude editor-normalization differences remain explicitly recorded failed characterization rows.

## [0.19.0] - 2026-10-14

- Resolve repeated Codex rollout identities to their latest thread history and prefer CLI session names while excluding injected context and automation-only sessions
- Cache bounded Codex display metadata without relaxing scoped filesystem authority, source replacement checks or incomplete-history disclosure
- Restore CLI-owned Codex archives through an explicit one-shot unarchive command, preserve their opening-time metadata, and wait for the exact command owner during application shutdown
- Add independent Claude Code and Codex support preferences for creation and history while retaining open terminal ownership
- Preserve independent Legacy/native history and archive origins even when their CLI session IDs match
- Defer release preflight runner allocation until signed platform builds finish while retaining exact-main CI, coverage and updater signature gates
- Align historical unread-receipt documentation with the existing provenance, generation, exact ACK and bounded application-lifetime cache contracts

Hosted CI and signed publication remain required. Real installed CLI, Windows WebView2, unfiltered native All and native installation/return acceptance remain separately unverified.

## [0.18.12] - 2026-10-12

- Combine each session's application and lifecycle indicators in one fixed icon slot while keeping the Claude and Codex marks recognizable and all existing states visible
- Keep authenticated native Claude reply-end unread until its exact notice is acknowledged, including after background selection and close/history retention within the app's bounded in-memory cache; show permission, input, error and unknown badges with accessible status text
- Respect reduced motion and keyboard, pointer and touch access; verify the focused session behavior and production CSP without changing native CLI ownership

## [0.18.11] - 2026-10-12

- Keep bounded Claude title-tail cwd changes from making an otherwise valid history source unavailable while retaining exact session identity and header ownership checks
- Raise aggregate native history read budgets to 64 MiB and 16,384 entries while retaining per-file caps, deadlines and fail-closed admission
- Keep open terminals ahead of closed history in session lists and preserve relative ordering within each group

## [0.18.10] - 2026-10-12

- Fix the installed application's startup black screen by compiling locale strings through Vue I18n's CSP-compatible AST runtime
- Verify the actual production bundle with the production CSP in a browser, including English/Chinese startup, Projects navigation and lazy Settings loading; make this a required frontend CI step
- Retain strict CSP, authenticated document/IPC boundaries, existing native test inventory and protected-main signed publication gates

## [0.18.9] - 2026-10-12

- Preserve staged input and partial/unknown receipts while a commit is in progress; reclaim only abandoned, uncommitted uploads with a strictly newer sequence
- Reserve clipboard input before asynchronous reads, bind it to the original run and terminal mode, cancel unsent empty/stale intents without blocking later input, and release probed native image resources
- Clear the observed session identity on restart and keep background process-exit synchronization active
- Batch authenticated history observations within the 64-project scope budget, validate each authority check against one workspace snapshot, isolate offline project admission failures, and await replacement owners before publishing grouped results
- Retire superseded profile project scopes and bound document generation floors without forgetting live-document replay guards
- Keep completion and permission attention eligible after activity hints expire, and render distinct read-only resources even when their display fields coincide
- Order Legacy clipboard, keyboard, IME and terminal protocol input through an exact-PTY FIFO with synchronous paste reservation
- Retain exact owned-process control after waiter failures, bind stops to the original request, and finish process, registry and output-stream ownership together before reclaiming a run
- Preserve quoted paths, UNC prefixes and empty arguments in both Legacy launch entry points while passing shell operators and variables literally
- Keep native resume-picker, name and transcript-path semantics for user argv while validating only the separately frozen Desk history locator
- Open normalized Windows UNC and long paths without admitting Explorer switches
- Reject ended-turn activity before status reduction, retain exact PTY boundaries across restart, and freeze each queued Legacy environment/default-option write independently
- Revoke obsolete pending history contexts immediately after profile or project changes so late reads cannot restore removed rows
- Admit the bundled data images under production CSP and retain the complete 5,000-session, 30-owner and 120-change stress workload
- Project native Claude session turn activity onto the tree with coarse arrival-order evidence: allowlisted turn hooks claim working, permission/idle notifications claim waiting for input, Stop falls back to idle, and a 90-second host turn-idle timer bounds stale working claims back to idle. Sessions no longer show the unknown question mark while they are actively working; a failed or disconnected observer still shows unknown. This is a best-effort UI projection, never completion/permission/error causes, and it does not touch authority or ownership
- Keep the bounded subagent hint priority over generic working claims and keep waiting for the user ahead of subagent-started receipts; retired subagent IDs still never revive, and late subagent stop receipts cannot turn an ended turn back to unknown

Protected-main CI, exact-source coverage and the complete signed platform set remain required before publication. Hosted checks and synthetic regressions do not certify real installed CLI, Windows WebView2 or native historical installation acceptance; existing unavailable-host disclosures remain unchanged.

## [0.18.8] - 2026-10-10 (repair candidate)

- Keep permission, completion and waiting-input status independent of which session is selected; preserve exact attention evidence and existing running/closed distinctions
- Recognize authenticated, exact-run Claude subagent start/stop evidence for bounded first invocations; retain permission priority, stale-owner rejection and conservative unknown states after evidence loss or reused agent IDs
- Share a fixed trailing slot between history time and Archive on hover or keyboard focus, expose Archive on touch, and keep the full-size Close control plus menu Archive for ended open sessions
- Observe Claude identity through adaptive bounded headers and read optional AI/custom title metadata from a same-held-file tail; retain exact-source known display titles and avoid saving opening checkpoints as manual renames
- Reuse one authenticated, bounded history observation across consecutive pages and restart an expired continuation once as a whole observation, preserving authority, revocation and absence safeguards

This prepares the authorized combined repair candidate. Disabled observers still provide no live activity evidence, and this release does not enable them or certify real installed CLI behavior. Same-run subagent ID reuse lacks an invocation epoch and remains unknown. A title outside the bounded header/tail remains unknown, and a live newly created tab without an authenticated history identity still lacks automatic-title binding. Cross-project offset-zero scans still repeat; the user’s precise unsupported history entry was not identified. Exact-source hosted checks, reviewed rendered screenshots and signed protected-main release admission remain required; existing platform and native installation acceptance boundaries remain unchanged.

## [0.18.7] - 2026-10-10 (repair candidate)

- Restore closed and historical sessions by clicking their row; already-open rows only select, and nested actions and pending attempts retain exact-owner cancellation and duplicate-launch protection
- Keep source diagnostics dismissible for the acknowledged source state, with bounded details and a specific budget hint
- Read Codex history identity through bounded header chunks instead of consuming complete large rollouts, retain source admission and resource caps, and preserve confirmed display activity when later metadata is partial
- Expose a safe explicit Retry after a source budget failure; retry freshly validates the original session/profile context even when the history row disappears or the default profile changes

This prepares the authorized repair release. Exact-source hosted checks and signed release admission remain required. Synthetic regression tests and sparse large-history fixtures do not certify the user’s installed CLI restoration or its precise failure cause.

## [0.18.6] - 2026-10-10 (repair candidate)

- Reconcile one-click close across exact Windows process control, legacy exit events and native launch receipts; retain live or unconfirmed ownership and preserve newer attempts
- Refresh the closed project's activity snapshot after exact history reconciliation, including delayed native history publication, while background activity keeps rows stable
- Isolate unsupported or unreadable history entries, preserve readable sessions and show bounded partial-source diagnostics; coordinate projection reads and publish history state reactively
- Separate Rust executable production from static checks so test shards can start earlier, preserve all required gates, and record bounded same-source build/timing evidence

This prepares the authorized repair release. Windows process-control, complete exact-source CI and signed release checks remain required before publication. These tests do not certify the user's installed Windows or real Claude/Codex interaction.

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
