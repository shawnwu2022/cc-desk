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

Claude observes 4 KiB chunks until a complete, validated supported record exposes its actual cwd. Leading metadata is not a project-association proof, and no encoded directory name is promoted into one. A long first user record can extend within the same original caps. A complete metadata-only file with no observed cwd remains incomplete for a project-filtered read.

The official `ai-title` / `aiTitle` / `sessionId` record is recognized with exact filename/session-ID agreement. Custom title takes priority over AI title, then first-user summary. After required headers and whole-tree duplicate/link checks finish, positively associated project rows may use spare budget for a bounded title tail: the observed header and at most 4 KiB from EOF are read on one newly held file handle, with header identity/cwd and before/after stamp checks. First-read header + reread header + tail remain within the per-file cap; every byte/entry is charged to the original aggregate limits. Near the deadline or without sufficient spare budget, optional title work is skipped. A title in an unread middle section is not observed. Tail metadata does not supply activity timestamps, source completeness or absence evidence.

Finite `titleSource` and `titleUnknown` observations let exact-context/root/session display metadata preserve a prior known title when a later bounded observation is unknown or lower-priority. Fresh equal/higher-priority observations and complete observations can replace that display fallback. New opening-order checkpoints do not persist an automatic title as a manual override. Existing nonempty saved names are preserved because older records do not distinguish automatic names from user renames; no destructive migration guesses from their text.

Full Messages/Search keep their original complete read/parser behavior and limits. Small completely read histories retain native timestamps and full validation.

Previously known activity may be retained only as display metadata for the exact source/context/session identity. Local cached observations do not populate source `updatedAt`; persisted display records remain identity-checked. Unknown/partial source state, fresh verification and absence rules are unchanged. A new source/root/key never inherits another source's last-known activity.

## Scale evidence and limits

Synthetic fixtures cover 1,677 sparse Codex histories across 159 directories, approximately 3.715 GiB logical size, including long valid headers and missing titles. The former mandatory 64 KiB sample failed with `SOURCE_TOO_LARGE`; the adaptive observation returns all fixture identities within the unchanged aggregate budget. This is a byte-budget regression test, not proof that a particular installed user's five-second failure is repaired.

A separate 12 KiB-header fixture still exceeds the unchanged 16 MiB aggregate limit. Sufficiently large identity records, too many entries, slow guarded I/O and long Claude identity records can still exceed legitimate limits. No stale cached presence or client-supplied ID/path shortcut bypasses current-source verification. Real installed CLI and configured-root acceptance remains separate from synthetic tests.

## Consecutive History pages

A 200-row IPC page no longer triggers another whole-root scan for every offset. Consecutive pages of the same exact admitted scope/request epoch reuse one bounded observation and retain its original observedAt. The backend still checks owner/reference, profile/project authority and held roots before publication. Offset-zero loads always scan anew; late older loads cannot retire or replace a newer receipt. The retained pool is bounded to 64 MiB in total, 32 MiB per observation and the registry's existing scope capacity. Small observations from three or more scopes can coexist; a two-slot count eviction is not used. Receipts expire after five seconds and retire after the final page.

An expired/evicted continuation emits fixed SOURCE_SNAPSHOT_EXPIRED. The history store may restart that entire owned observation once with a new request epoch, discarding all older pages and diagnostics from the abandoned attempt. It never stitches pages from different observations, retries a root/authority error, or falls back to stale cached presence. A second expiration is explicit and retryable. Paginated reads remain positive discovery only and do not acquire extra absence authority.

## Verified coverage and remaining diagnosis

Synthetic fixtures cover 1,251 Claude transcripts with valid large bodies, leading permission-mode/snapshot records and long first-user identity records. Fixed-window aggregate exhaustion and actual missing-positive-row regressions are reproduced separately from any installed user's fault. Strict bridge + real history-store tests cover three concurrent project scopes, bounded restart and unchanged absence protection.

The earlier legacy path enumerated project-local filenames, resolved only the requested/recent subset through its name cache, and could retain a row despite unreadable content. Native projection performs whole-source identity/duplicate/link checks before project filtering and exposes incomplete observations. Profile × registered-project root scans still repeat; consecutive-page reuse does not solve that larger cost. Unknown record-only transcripts, empty files and unsupported deeper containers can still be skipped honestly. A screenshot's SOURCE_UNSUPPORTED does not identify which category failed.

Useful future non-content diagnostics would count required files/bytes/entries, elapsed guarded-read time, supported versus unrecognized transcript/container failures, incomplete identity observations, optional tail skips, and pagination receipt/restart outcomes. Paths, prompt bodies, title text, credentials and raw exceptions are not needed for these counters. A running Native new tab with no observed actual session ID cannot safely be associated with a history file by choosing the newest file; this repair does not fabricate that identity.
