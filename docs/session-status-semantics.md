# Unified session status semantics

The session tree keeps process lifetime, terminal visibility, turn activity and
attention cause separate. A running PTY is not proof that a CLI is working; an
ended process is not proof that its response completed. Opening or closing a
terminal is a host action, independent of a CLI turn.

## Historical mapping checklist

| Historical signal | Unified field | Row state | Preserved presentation |
|---|---|---|---|
| Explicit working | `activityState: working` | working | Green filled center, dashed ring, pulse |
| Thinking | `activityState: thinking` | working | Working presentation, detail remains representable |
| Tool executing | `activityState: tool_executing` | working | Working presentation, detail remains representable |
| Subagent running | `activityState: subagent_running` | working | Working presentation, detail remains representable |
| Compacting | `activityState: compacting` | working | Working presentation, detail remains representable |
| Explicit idle while process runs | `activityState: idle` | running | Static green filled center |
| Permission request | `attentionKind: permission` or `activityState: waiting_permission` | permission | Gold pause mark, pulse |
| Waiting for input | `activityState: waiting_input` | needs-user | Gold attention mark |
| Ordered coarse waiting | `activityState: waiting` | needs-user | Generic gold attention; no invented permission/completion cause |
| Response completed | `attentionKind: completed` | completed | Hollow green rings |
| Sticky error | `attentionKind: error` or `activityState: error` | error | Red alert mark, retained on ordinary acknowledgment |
| Pending without known cause | `attentionState: needs-user` | needs-user | Gold fallback |
| Stopped retained selected terminal | `processState: stopped`, `opened: true`, selected | stopped | Solid gray center |
| Closed history/unselected stopped terminal | `processState: stopped` | closed | Hollow gray ring |
| Missing, unavailable or unordered activity | `activityState: unknown` | unknown | Gray activity-unknown mark |
| Starting process | `processState: starting` | starting | Existing starting indicator |
| Uncertain process ownership | `processState: unknown` | confirming | Existing status confirmation indicator |
| Failed launch | `processState: failed` | failed | Existing launch-failure indicator |

Process lifecycle wins over turn feedback. For a running session, the historical
row priority remains working, error, permission, completed, generic pending,
then known idle or unknown. Selected rows suppress permission/completed/pending
badges as before; errors remain visible. This visual suppression is not an
acknowledgment and does not change the stored activity or cause.

Collapsed projects show counts in the historical priority: error, permission,
completed, generic attention, then running processes. Archived records do not
contribute. Running process counts and explicit working counts remain separate.
The global Workspace navigation badge aggregates only running, unarchived rows:
red explicit errors take priority over gold explicit permission requests.
Completion and generic pending causes never become a global permission badge.
Its localized accessible description remains outside the decorative icon glyph.

## Sources and ownership

- The Legacy adapter preserves existing `tab.working` when no explicit unknown
  activity overrides it and observation is not disabled/unavailable, and consumes the injected attention store's exact PTY
  item. A known session-ID mismatch is rejected. Stopped tabs and history rows
  never inherit attention from an old PTY.
- Legacy completion uses the existing `attentionFromEvent` interpretation of
  explicit `notification.idle_prompt`. `Stop`, process exit, input/output traffic
  and silence never create completion. Permission uses the existing permission
  notification types; sticky error uses `stopFailure`.
- Native tabs preserve the existing observer reducer's ordered
  `working | waiting | unknown` projection and observer availability. A starting
  tab holds at most one latest projection for its exact request/run/generation;
  only that attempt's running receipt publishes it. Unknown/failure/end/restart/
  close invalidate that pending projection. Unavailable/off observations cannot
  publish working or waiting, and stale attempts cannot update a newer one.
- Existing observer event-ID bounds, canonical monotone `sourceSequence`
  validation, duplicate/reordered rejection and exact-run binding are unchanged.
  No sequence or turn status is inferred from HTTP arrival order.

## Current native evidence gap

The current authenticated Claude hook mapper emits no `sourceSequence`; the
backend hook processes do not establish a trusted CLI event order. The observer
reducer therefore keeps their current activity unknown. `Stop` and
`notification.idle_prompt` also map to unknown, and the native observer contract
has no ordered completed/error/permission-reason projection. Native profiles
default observation off, and Codex/raw/Shell launches receive no Claude overlay.

Consequently this patch restores reliable downstream projection and legacy
attention semantics; it does **not** establish live native working/completed
acceptance. A future source must provide authenticated exact-run, source-owned
ordering and explicit turn/reason evidence before adding those claims. An HTTP
counter, timer, last output, process exit or raw hook kind cannot supply that
proof. Keep any capability-specific source integration and actual authenticated
Claude/Codex acceptance separate from this host/UI repair.

## Smallest follow-up choices

The current gap is at the producer/DTO boundary, not at the icon renderer:
`HookEventPayload` has authenticated run/generation/event ID but no source order;
`claudeObserver` maps `idle_prompt`/`stopFailure` to coarse unknown and drops their
reason; `ObservationState` has no turn identifier or attention cause. Codex/raw/
Shell have no Claude source and must keep their own capability boundary.

1. **Restore accurate current turn state:** require a certified producer with an
   authenticated exact-run identity, source-owned monotone sequence, explicit
   turn identity and working/permission/error/completion facts. Carry that proof
   through the backend observer DTO and the existing strict reducer before
   projecting it. Parallel hook processes and server arrival counters cannot
   supply this contract. Certify the actual selected CLI before enabling it; do
   not change CLI flags or optional observer defaults as a UI repair.
2. **Smaller product compromise: unread event notices:** retain authenticated
   explicit `idle_prompt`, permission and `stopFailure` reason in a separate
   exact-run receipt DTO/model. Display “Completion notice received; current
   activity unverified”, rather than claiming current completed state. Deduplicate
   event ID, bind it to the owning request/run/generation and backend observer
   lease, acknowledge only an actually visible/focused selected owner, and allow
   that unread notice to request taskbar attention. Keep the current activity
   reducer and its unknown result unchanged. This requires a small reason/receipt
   transport extension plus actual authenticated hook acceptance; it is a
   concrete follow-up proposal, **not implemented or certified by this patch**.

The first option supplies the missing proof. The second supplies useful explicit
notifications without presenting stale event receipt as the latest CLI state.
Neither uses Stop, silence, process lifetime or fabricated sequence numbers.

## Regression evidence

- `tests/utils/unifiedStatusSemantics.test.ts` tests all historical detail
  activity mappings, attention priority, selected/stopped/history distinctions,
  Legacy completion/acknowledgment/error behavior, Native ordered activity through
  the real store/adapter, latest pending state, stale owner rejection, unavailable
  observations, restart invalidation, current native completion absence, and
  project cause counts.
- `tests/components/primaryNavStatus.test.ts` checks real navigation badge
  priority, running/archive exclusions, reactive removal, both languages and
  accessible description ownership. Native adapter tests explicitly reject
  Claude activity overlays on Codex/raw records and inactive observation.
- `tests/components/sessionStatusSemantics.test.ts` checks the actual bundled
  geometry and English/Chinese accessible labels, work/permission pulse and
  reduced-motion rules, and collapsed project cause/count priority.
- Existing observer, native tab, adapter, session row/tree and localization tests
  remain required. These tests use fabricated trusted inputs at host boundaries;
  they do not certify authenticated real CLI behavior, taskbar flash delivery,
  Windows rendering, scaling or screenshot acceptance.
