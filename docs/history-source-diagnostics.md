# History source diagnostics and bounded metadata

History warnings belong to the workspace's source checks, not to an individual terminal. Dismiss closes both unavailable-source and partial-history notices from the terminal area. The sessions sidebar retains **History sources (incomplete)**, which opens the current fixed diagnostics, explicit Retry and applicable launch-configuration actions.

Acknowledgements are in-memory and keyed to fixed source/error/stage plus canonical profile revision and admitted opaque root identity. Partial observations additionally retain their exact registered-project context. A repeated background refresh, explicit retry with the same result, locale change or workspace/project navigation does not reopen dismissed terminal notices. A changed identity/error or observed recovery followed by recurrence can surface a new notice. Dismiss does not clear failures, make a source healthy, grant session admission or prove absence. Pending reads and failed adapter-owned forced reads never display a success claim.

## Guard meanings

- `SOURCE_BUDGET_EXCEEDED`: the cooperative five-second deadline, checked between operations; it cannot interrupt an OS filesystem call
- `SOURCE_TOO_LARGE`: individual file/read or aggregate byte budget
- `SOURCE_TOO_MANY_ENTRIES`: the entry budget

The existing maximums remain 2 MiB per file/read, 16 MiB aggregate, 4,096 entries and five seconds. No error code establishes the configured root's volume or identifies an offending file.

## Codex metadata sampling

History scans the admitted source layout and retains the path/link/root/revocation checks. Codex's first complete `session_meta` record is sampled using 4 KiB read chunks, extending within the same caps for a long header. Every byte actually read, including chunk surplus, is charged. Every complete record in the observation is parsed and validated; all observed native IDs still participate in duplicate detection before project filtering. Unread tails remain explicitly incomplete. A first-user title is retained when its complete record is in the observed chunks; otherwise the title can be unknown. Incomplete observations do not fabricate a native latest-activity timestamp or substitute file modification time.

Claude retains its existing 64 KiB observation window: its project cwd/title can occur after an initial snapshot. Full Messages/Search keep their original complete read/parser behavior and limits. Small completely read histories retain native timestamps and full validation.

Previously known activity may be retained only as display metadata for the exact source/context/session identity. Local cached observations do not populate source `updatedAt`; persisted display records remain identity-checked. Unknown/partial source state, fresh verification and absence rules are unchanged. A new source/root/key never inherits another source's last-known activity.

## Scale evidence and limits

Synthetic fixtures cover 1,677 sparse Codex histories across 159 directories, approximately 3.715 GiB logical size, including long valid headers and missing titles. The former mandatory 64 KiB sample failed with `SOURCE_TOO_LARGE`; the adaptive observation returns all fixture identities within the unchanged aggregate budget. This is a byte-budget regression test, not proof that a particular installed user's five-second failure is repaired.

A separate 12 KiB-header fixture still exceeds the unchanged 16 MiB aggregate limit. Sufficiently large identity records, too many entries, slow guarded I/O and Claude's existing observation window can still exceed legitimate limits. No stale cached presence or client-supplied ID/path shortcut bypasses current-source verification. Real installed CLI and configured-root acceptance remains separate from synthetic tests.
