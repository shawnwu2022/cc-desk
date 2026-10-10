# Unified session status semantics

The session tree keeps process lifetime, terminal visibility, turn activity and
attention cause separate. A running PTY is not proof that a CLI is working; an
ended process is not proof that its response completed. Opening or closing a
terminal is a host action, independent of a CLI turn.

## Historical mapping checklist

| Historical signal | Unified field | Row state | Preserved presentation |
|---|---|---|---|
| Explicit working | `activityState: working` | working | Solid green backplate with work sparkle; slight scale pulse |
| Thinking | `activityState: thinking` | working | Specific contrasting glyph and localized detail label; sequential dot brightness |
| Tool executing | `activityState: tool_executing` | working | Specific contrasting glyph and localized detail label; working scale pulse |
| Subagent running | `activityState: subagent_running` | working | Specific contrasting glyph and localized detail label; working scale pulse |
| Compacting | `activityState: compacting` | working | Specific contrasting glyph and localized detail label; working scale pulse |
| Explicit idle while process runs | `activityState: idle` | running | Static solid green dot |
| Permission request | `attentionKind: permission` or `activityState: waiting_permission` | permission | Static shield on solid gold backplate |
| Waiting for input | `activityState: waiting_input` | needs-user | Input caret on solid gold backplate |
| Ordered coarse waiting | `activityState: waiting` | needs-user | Reply bubble on solid gold backplate; no invented permission/completion cause |
| Response completed | `attentionKind: completed` | completed | Static check on solid green backplate |
| Sticky error | `attentionKind: error` or `activityState: error` | error | Alert mark on solid red backplate, retained on ordinary acknowledgment |
| Pending without known cause | `attentionState: needs-user` | needs-user | Reply bubble on solid gold backplate |
| Stopped retained selected terminal | `processState: stopped`, `opened: true`, selected | stopped | Stop square on solid neutral backplate |
| Closed history/unselected stopped terminal | `processState: stopped` | closed | Close cross on solid neutral backplate |
| Missing, unavailable or unordered activity | `activityState: unknown` | unknown | Distinct question mark on solid neutral backplate |
| Starting process | `processState: starting` | starting | Clock on solid info backplate, gentle breathing |
| Uncertain process ownership | `processState: unknown` | confirming | Confirmation clock on solid neutral backplate |
| Failed launch | `processState: failed` | failed | Alert on solid error backplate with launch-failure label |

Process lifecycle wins over turn feedback. For a running session, the historical
row priority remains working, error, permission, completed, generic pending,
then known idle or unknown. Selection never converts a known permission,
completion or waiting cause to Running. An explicit permission/error cause takes
priority over the narrower causal subagent lifecycle hint. Actual visible/focused
acknowledgment still belongs to the attention owner, independently of rendering.

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
  `working | waiting | unknown` projection, bounded causal `subagent_running`
  exception described below, and observer availability. A starting
  tab holds at most one latest projection for its exact request/run/generation;
  only that attempt's running receipt publishes it. Unknown/failure/end/restart/
  close invalidate that pending projection. Unavailable/off observations cannot
  publish working or waiting, and stale attempts cannot update a newer one.
- Existing observer event-ID bounds, canonical monotone `sourceSequence`
  validation, duplicate/reordered rejection and exact-run binding are unchanged.
  No sequence or turn status is inferred from HTTP arrival order.
- Native occurrence notices use the existing authenticated Claude hook envelope
  independently of the ordered activity reducer. They retain only a fixed kind,
  opaque event ID, host receipt time and run/generation. The hook subscription
  owns the existing observer lifetime; the tab store privately binds the current
  request/run/generation and validates that published notices were accepted by
  that owner. Codex/raw and ended/uncertain tabs cannot publish these notices.
- A starting tab holds at most the latest receipt and latest unread reply-end
  receipt, publishing them only after its exact running launch receipt. Unknown,
  failure/end, restart and close clear published/pending notices. The latest
  unread reply-end event survives a later prompt/tool receipt until actual
  visible, selected and focused ownership acknowledges its exact event ID. Each
  hook owner and tab attempt retains at most 1024 accepted IDs; exhaustion rejects
  further events without evicting IDs and allowing replay.

## Current native evidence gap

### Bounded subagent lifecycle exception

Authenticated Claude SubagentStart/SubagentStop now retain only the provider's
opaque agent ID (nonempty, control-free, at most 128 UTF-8 bytes). An exact
run/generation reducer tracks first observed invocations independently of global
turn order. At least one admitted start with no observed stop can supply the
specific subagent-working hint; a stop retires that ID even when delivered before
its start. Duplicate receipts, foreign owners and stopped/invalidated attempts
cannot resurrect it. Permission/waiting evidence blocks the hint, and observer
off/unavailable/uncertain/end/restart/close boundaries invalidate its activity.
When a source does supply a sequence, malformed or reordered lifecycle receipts
must pass the same fail-closed sequence fence as every other ordered event.

This is a best-effort lifecycle observation, not provider-wide ordered turn
state or completion proof. Claude can reuse the same agent ID on resume or a new
teammate message, and another SubagentStop hook can veto ending. Without a unique
invocation epoch, an already retired ID remains unknown until a new exact run
generation; no arrival timestamp, server counter or transcript path supplies
that missing identity. A lost stop may leave an observed first-invocation hint
until invalidation; no timeout, output or Stop creates completion. See the
[official hook lifecycle contract](https://code.claude.com/docs/en/hooks#subagentstart).

### General turn ordering remains unavailable

The current authenticated Claude hook mapper emits no `sourceSequence`; the
backend hook processes do not establish a trusted CLI event order. The observer
reducer therefore keeps general turn activity unknown apart from the bounded
subagent exception above. `Stop` and
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

The Native row now shows a separate localized event marker. An unread `Stop`
receipt is described as “Reply-end notice received (unread); current activity
unverified”. It never becomes `attentionKind: completed` or changes the current
activity icon. Other allowlisted hooks show recent prompt, tool, permission,
input, subagent or compaction events. These are receipt observations in host
arrival order, not proof of provider event order or present activity. The five
existing optional observer capabilities remain `partial`; observation defaults,
profile fields, CLI launch arguments and permissions are unchanged.

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
2. **Implemented bounded compromise: occurrence notices:** use the existing
   authenticated hook envelope and subscription to keep independent recent and
   unread reply-end receipts, with exact request/run/generation ownership and
   bounded deduplication. The unified attention coordinator requests window
   attention once for each owned unread reply-end receipt only while the observer
   is active and the owner is not actually viewed. A truly visible/focused
   selected owner may acknowledge an already owned receipt even if observation
   has since become off/unavailable. Actual authenticated CLI delivery and OS
   taskbar behavior remain uncertified by host fixtures.

The first option supplies the missing current-state proof. The second supplies
useful explicit event notices without presenting a receipt as the latest CLI
state. Neither creates completion from Stop, silence, process lifetime or
fabricated sequence numbers.

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
- `tests/stores/nativeTabObservationNotice.test.ts` verifies receipt ownership,
  starting publication, invalidation, replay bounds, exact acknowledgment,
  defensive copies and unified projection with unknown current activity.
  `tests/components/nativeObservationNoticeRow.test.ts` verifies the distinct
  accessible marker, both languages and all twelve receipt kinds. Mapper,
  hook-bus, Native terminal composition and unified window-attention tests cover
  their respective receipt lifetime boundaries without launching a real CLI.
- Existing observer, native tab, adapter, session row/tree and localization tests
  remain required. These tests use fabricated trusted inputs at host boundaries;
  they do not certify authenticated real CLI behavior, taskbar flash delivery,
  Windows rendering, scaling or screenshot acceptance.
- Causal observer, hook-store and Legacy status-monitor regressions cover two
  interleaved agents, stop-before-start, duplicate/malformed IDs, supplied stale
  sequences, permission barriers, identity reuse, unavailable observation,
  exact-run recovery/restart/close and foreign-owner refusal. The Rust observer
  harness imports actual production sources and verifies bounded lifecycle
  redaction through validated envelopes; this is host proof, not live CLI
  certification or permission to enable a disabled profile.

## Unreleased filled-glyph presentation

The existing status priority and evidence boundaries above are unchanged. Details
are shown only when their matching coarse state is already justified; a raw native
receipt cannot become Thinking, Working, Completed or Permission. Archived rows
use an archive-box label/shape without claiming runtime status. Hover descriptions
close on pointer leave, keyboard descriptions close on blur/Escape, and all shapes
keep their accessible localized name. Known starting and justified working breathe
slightly, while thinking dots brighten sequentially. Tool/branch symbols never rotate.
Entering permission/input or completed once triggers a brief transition; selection,
refresh and remount do not replay it. Reduced-motion disables every animation.

Every semantic icon uses a filled 16px circular backplate and a contrasting local
mark. Icon-scoped palettes strengthen green in the light theme and info/error in
the dark theme; muted states use secondary text ink. Theme arithmetic covers both
symbol/backplate and selected/hover surface contrast at 3:1. This unit evidence is
not a substitute for reviewing rendered pixels or Windows accessibility/scaling.

## Focus switching and archive tail regression

The selected-only Running substitution was introduced by commit `198c3f89`;
the authenticated unordered-hook projection was introduced separately by
`585c8265`. They are distinct causes: focus never removed Native subscriptions,
and reverting the unknown projection wholesale would restore untrusted arrival
order guesses. The regression now exercises two simultaneously owned Legacy
sessions through the real adapter, catalog activation and normal App, checking
stable subagent/permission labels and unchanged PTY ownership on both switches.

Closed history keeps time and Archive in the same fixed tail slot, showing time
at rest and Archive on row hover or keyboard focus. Coarse/no-hover pointers show
Archive directly. An open ended/failed terminal prioritizes Close in that slot
and retains Archive in its menu. Menu/context/keyboard entry points share the
same typed action and cannot resume or activate the row through bubbling. The
rendered fixtures cover these transitions separately from unchanged approved
screenshots; local DOM/CSS success is not installed Windows acceptance.
