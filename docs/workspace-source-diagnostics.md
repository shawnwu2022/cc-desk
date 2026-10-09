# Workspace source diagnostics

The workspace partial-load warning remains visible when any source read fails. Its collapsed details identify a fixed source category and an allowlisted diagnostic code. Categories distinguish project discovery, saved project metadata, launch configurations, registered projects, Claude compatibility history, native Claude history, Codex history, and the session catalog.

`useUnifiedWorkspaceRuntime.refresh` collects failures locally and publishes only when that refresh still owns completion. Repeated category/code/stage triples collapse to one entry; at most twelve appear, with an explicit omitted-errors message if more distinct failures occurred. Later catalog failures merge into the current diagnostics only while their refresh ownership remains valid. A successful explicit refresh clears the reported failures.

`workspaceSourceWarnings` accepts exact known public codes. Unknown codes, native field contents, paths, profile and session identifiers, titles, and raw exception text are never rendered. The production document bootstrap and projection client tests cover raw authenticated request envelopes, Rust-style safe error rejection, unavailable projection responses, malformed response rejection, and bounded diagnostic output. Runtime tests cover isolated failures, open-session preservation, stale refreshes, subsequent catalog failures, and the real App details surface.

This change does not change document admission, source-reader budgets, filesystem capabilities, or CLI behavior. The generic warning is not proof of one root cause: `FORBIDDEN` is distinct from a history-reader result such as `SOURCE_TOO_LARGE` or `SOURCE_UNSUPPORTED`. Real Windows source data and WebView lifecycle acceptance remain separate from fixture coverage.

Launch preparation also preserves the availability issue's allowlisted code. The failed preparation keeps its existing explicit edit/retry behavior, while the banner and session diagnostics distinguish missing program selection from unavailable executables, runners, environment references and invalid configuration. Raw issue fields/messages are discarded. Saving a repair does not launch a process.

The synthetic `HistoryDiagnostics_` cases exercise both providers through the production projection service: minimal profiles with omitted optional defaults, omitted/null/registered project targets, and the current history request envelope. An invalid inherited environment name reproduces `INVALID_REQUEST` for both providers at `scope-environment`; a malformed old profile instead produces `WORKSPACE_INVALID`. This matrix does not establish the installed application's cause.

The shared history request is `native_get_scope` with a strict profile target (profile ID, string profile revision, registered project ID), followed by `native_list_resources` with the issued SourceRef, `history`, a string request epoch, null query/sessionId, limit 200 and an offset. Neither request admits an arbitrary source path. No payload or private configuration is needed for diagnosis.

Formal projection commands now reject with `{code, stage, retryable}`. `ProjectionStage` is a fixed backend enum; native field/index values and parser messages are not serialized. The original safe code survives (including fixed storage/schema/run errors). Frontend validation, bridge serialization/invoke and response validation have separate fixed stages; unknown codes/stages remain unavailable/fallback diagnostics. The existing admission, validation, environment and read budgets are unchanged.

| Stage | Returned from |
| --- | --- |
| `frontend-bridge`, `frontend-serialization` | Document bridge lookup or UTF-8 JSON preparation |
| `scope-invoke`, `read-invoke` | Transport rejection without a recognized backend stage |
| `scope-request-decode`, `read-request-decode` | Raw-body requirement, body budget or serde decoding |
| `scope-request-validation`, `read-request-validation` | Strict target/read request checks |
| `scope-profile-validation` | Stored profile/revision/legacy configuration checks |
| `scope-environment` | Shared environment construction |
| `scope-project-registration` | Registered project lookup, validation or directory identity |
| `scope-source-selection`, `scope-source-root` | Supported source-root selection and opening |
| `scope-capability`, `read-capability` | Scope/run ownership, revision or revocation checks |
| `read-source-enumeration` | Authorized source reading, limits or unavailable source response |
| `scope/read-document-admission`, `scope/read-response-admission` | Document authority before request or after asynchronous work |
| `scope/read-response-validation`, `scope/read-task` | Frontend response validation or backend worker failure |

After deploying a diagnostic build, the minimal field check is: open the workspace, click its existing Retry once, expand Details and report only the source label plus `CODE / stage`. A full screenshot, workspace file, environment value, path or transcript is unnecessary. Without a desktop control tool, automated fixture/WebView evidence must not be reported as real user-source UI acceptance.

## Windows inherited environment aliases

The local default Windows execution environment reproduced both default profiles failing with `INVALID_REQUEST / environment.aliasConflict`: 54 entries contained one group of Windows-equivalent names with differing values. The elevated Cargo environment and a cmd child did not reproduce it. The old case-sensitive map retained those aliases, then shared environment validation rejected them before history or program discovery could proceed. This establishes a real local failure mechanism, not direct inspection of the desktop application's environment.

`capture_environment` now groups the raw Windows entries before collecting a map. A group with conflicting values uses `std::env::var_os`, which delegates to Windows' effective lookup; the returned value must belong to the captured group. Missing or changed values fail with a fixed safe field. Unique entries and same-value aliases retain their captured value without a second lookup, including empty/non-Unicode values and inherited drive entries. Authored profile, terminal, legacy and observer layers retain their existing validation. Availability, discovery, history scopes and launch freezing share this capture path.

`D08_Environment_WindowsHostDefaultProfiles_014` is an opt-in local probe (`--ignored --nocapture`) that reads only its own inherited environment and synthetic default profiles. It prints shape counts and fixed categories, never variable names/values or private configuration. Synthetic regression cases cover OS-effective resolution independent of spelling/order, exact-name duplicates, empty values, capture races, history for both providers and program discovery's unchanged project exclusions. Real desktop-source acceptance remains separate.

Local RED/GREEN: the same default execution context retained 54 entries and one conflicting alias group; both default profiles changed from `environment.aliasConflict` to `ok`. The host probe plus 51 focused environment, availability, history, discovery, launch, observer and projection tests passed, including the real WebView formal-command fixture. Rust formatting and strict all-target Clippy passed. Independent read-only review found no blocking issues. None of these checks reads real user histories or proves the installed desktop UI has recovered.

## Explorer reserved-name regression

The user's next field test still failed after the alias repair. Product startup identity confirmed the new build had run; the desktop and Start Menu links targeted its installed EXE. A hidden test probe launched through an existing Explorer folder-view Shell dispatch then reproduced a distinct failure: confirmed Explorer parent PID, 65 inherited entries, no alias conflicts and one reserved pseudo-drive entry. Both default profiles failed with `environment.name`. The Codex-launched 54-entry environment had omitted this entry, so its earlier success did not cover the desktop startup environment.

Windows's [Rust environment iterator](https://raw.githubusercontent.com/rust-lang/rust/1.98.0/library/std/src/sys/env/windows.rs) accepts a leading `=` before searching for the key/value separator. The inherited-name validator had only allowed `=A:` drive-letter forms, rejecting Explorer's `=::` form. It now accepts one leading `=` with a nonempty tail and no further `=` only for Windows inheritance. It preserves the OS string, including non-Unicode data. Profile, legacy, terminal and observer inputs still reject these reserved names; NUL, empty names and embedded separators remain invalid.

The opt-in probe can write a bounded report only when its caller-owned current directory contains `ccdesk-host-probe.request`. Its report contains PID/parent PID, fixed shape counts and fixed per-CLI result categories; it never writes environment entries or reads private application settings. Capture and validation use the same raw snapshot. Use a fresh report directory, confirm the parent is Explorer, and keep this evidence distinct from full product/UI acceptance.

## History metadata compatibility

After the Explorer environment repair, the user confirmed both native CLIs opened after their first program confirmation. History still reported Claude `SOURCE_UNSUPPORTED` and Codex `SOURCE_TOO_LARGE`. These are current source-enumeration failures, not stale launch notices; the existing successful-refresh regression verifies warning removal. Private transcripts were not inspected, so synthetic reproduction establishes compatible failure mechanisms rather than claiming a unique diagnosis of the user's files.

Claude stores independent subagent transcripts at `projects/{project}/{sessionId}/subagents/` ([official documentation](https://code.claude.com/docs/en/sub-agents#resume-subagents)). Main-session history excludes that exact regular-directory position, keeping unknown deep layouts and links unavailable. It does not merge child transcripts into main history.

History listing reads at most an initial 64 KiB per transcript. If the first JSONL record has no complete newline yet, reading may extend in 64 KiB chunks up to the existing 2 MiB file cap; Codex session metadata may contain long base instructions. The total 16 MiB, 4,096-entry and cooperative five-second limits remain. The same held-root capability, regular-file and before/after length/mtime checks apply. Complete small files retain the original parser. Truncated observations parse only complete JSONL records and extract known identity/cwd/title metadata, never partial JSON or guessed Codex IDs. Missing identity, conflicting IDs/cwd, malformed complete records and duplicate native IDs still fail. Truncated observations report no claimed latest timestamp and mark their session summary truncated.

Ready History responses carry `historyMetadataIncomplete: true` whenever any file observation was truncated, including files filtered out by project. The frontend validates this fixed boolean only on ready History responses and suppresses absence evidence for incomplete observations. A separate information notice marks partial summaries; the restore chooser also retains partial status without rejecting known positive sessions. Complete refreshes clear the notice. A ready list can provide positive session observations without proving a missing session was deleted. Message detail and search retain their strict full-file limits. Aggregate budget, unsupported schemas or first records above the cap can still produce explicit unavailable results; this change does not promise every possible history layout or size will load.

Extra runtime Node/node_repl/cmd windows observed alongside the user's field test belonged to Codex service process trees. No automatic CLI discovery executes a candidate, and first confirmation performs one persisted profile update followed by its owned retry. A successful program binding is reused; a new CLI/configuration still needs its own binding. Do not suppress genuine history warnings or kill unrelated Codex processes to make this field test appear successful.

The independent D12/D13 validation crates compile the same reader and environment test modules. D12's junction fixture uses the standard Windows command flags directly, without an application-only platform-module dependency. D13 explicitly pins the Windows ToolHelp/Foundation bindings used by the ignored fixed-category host probe. These are test-engineering changes; they do not alter the deployed application.
