# Semantic projection trial profile1

This main-owned record module is the schema source. It is a default-off E05
prerequisite, not an enrolled endpoint or a frozen production protocol. Native
authentication/AppBind and the inner header's exact negotiated minor validation
precede `projection::decode`. Both currently supported minors0/1 use profile1.
Optional additive fields are ignored; required fields, discriminants, profile,
canonical decimal counters and fixed lowercase hex identifiers are validated.
Unknown required semantics require a new negotiated profile; never downgrade.

Each payload is UTF8 JSON <=32768bytes, at most128 cells/changes. Counters are
canonical u64 decimal strings, including values above JS's safe integer range.
Target is Host32bytes/session16bytes/run<=128UTF8bytes/generation/incarnation;
binding32bytes and barrier16bytes are supplied by the trusted Host context.
The codec cannot establish that context or authorize terminal access.

SnapshotPart repeats one exact stamp/state, starts at a row-major cell index,
and covers contiguous nonempty parts. Geometry is1..100rows,2..240columns.
The Host borrowing encoder emits at most128 cells per part and caps aggregate
payload at4MiB; a partial/failed replacement cannot install or enable input.
Cells use short keys `c`scalar,`r`Host role,`w`wrap-end,`f`foreground,`b`background,
`s`style. Colors are Palette0..268 or Rgb(u8,u8,u8). Style bits0..5 are
bold/dim/italic/inverse/hidden/strikeout; bits6..8 select underline0..5
(none/single/double/curly/dotted/dashed). All higher bits and underline6/7 reject.
No cell contains HTML, links, native effects or terminal protocol commands.

Delta is one atomic update with exact installed base and increasing next
revision, <=128 unique ascending indices. Larger changes require a new snapshot;
multipart deltas are not admitted. Empty changes support mode-only revisions.
Dimension/buffer/structure/sync changes require a new Host barrier/replacement.
Core counts every actual admitted mode transition, even when one batch ends at
its initial mode; repeated no-ops do not increment. Core structure/dimension
counters are not Host sync authority. Epoch overflow freezes before batch effects.

The consumer requires a separately verified exact barrier announcement before
snapshot parts. It installs complete views atomically, retains old display as
historical on gaps, and emits no ACK while unsynchronized. ACK is a local install
stamp, not proof of pixels, a Host acceptance decision, a lease or an input grant.
The shared control payload is `{message:"ProjectionAck",profile:1,stamp:...}`,
<=16384UTF8bytes, exact selected minor0/1. Mandatory kind prevents confusing an
unrelated control object with an ACK. Native issued/delivered/ACK queue checks
exist as an opt-in prerequisite; endpoint authorization, fences/leases and
reconnect remain OPEN. No network dispatch selects the private local writer or
internal-reply path. A native install proof must not be used as a data grant.

`test-vectors/projection-snapshot.json` is consumed by Rust codec and TS tests.
`remote/ui-terminal` has no npm dependencies and uses installed Node24's type
stripping/test runner. Browser layout, fonts, signed mobile shell, IME and device
performance remain separate acceptance gates. Do not infer those from Node tests.
