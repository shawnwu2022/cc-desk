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
