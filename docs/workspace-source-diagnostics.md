# Workspace source diagnostics

The workspace partial-load warning remains visible when any source read fails. Its collapsed details identify a fixed source category and an allowlisted diagnostic code. Categories distinguish project discovery, saved project metadata, launch configurations, registered projects, Claude compatibility history, native Claude history, Codex history, and the session catalog.

`useUnifiedWorkspaceRuntime.refresh` collects failures locally and publishes only when that refresh still owns completion. Repeated category/code pairs collapse to one entry; at most twelve appear, with an explicit omitted-errors message if more distinct pairs failed. Later catalog failures merge into the current diagnostics only while their refresh ownership remains valid. A successful explicit refresh clears the reported failures.

`workspaceSourceWarnings` accepts exact known public codes. Unknown codes, native field contents, paths, profile and session identifiers, titles, and raw exception text are never rendered. The production document bootstrap and projection client tests cover raw authenticated request envelopes, Rust-style safe error rejection, unavailable projection responses, malformed response rejection, and bounded diagnostic output. Runtime tests cover isolated failures, open-session preservation, stale refreshes, subsequent catalog failures, and the real App details surface.

This change does not change document admission, source-reader budgets, filesystem capabilities, or CLI behavior. The generic warning is not proof of one root cause: `FORBIDDEN` is distinct from a history-reader result such as `SOURCE_TOO_LARGE` or `SOURCE_UNSUPPORTED`. Real Windows source data and WebView lifecycle acceptance remain separate from fixture coverage.
