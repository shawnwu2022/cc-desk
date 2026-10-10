# CC Desk session status shapes

These local, static SVGs are CC Desk-owned project artwork under the repository's
MIT license. They contain no external artwork, links, scripts or runtime markup.
All shapes share a solid 16px circular backplate filled by semantic `currentColor`.
Contrasting marks use the theme background ink, so meaning never depends on color.

- starting: clock; running/known idle: plain solid dot
- working: sparkle; thinking: dots; tool execution: hammer
- subagent: simple Y branch; compacting: inward arrows
- needs-user/pending: reply bubble; waiting input: caret; permission: shield
- explicit completion: check; sticky error/failed launch: alert with separate labels
- stopped/ended: square; closed: cross; archived: archive box
- unknown activity: question; uncertain process: confirmation clock

Existing data-shape identifiers remain stable where possible; names containing
ring/dot are historical IDs and no longer describe hollow artwork. Each asset is
allowlisted in SessionStatusIcon and is decorative inside one localized accessible
Tooltip trigger. Known starting and working have slight stylesheet breathing; thinking dots brighten
sequentially. Permission/input entry and completion animate once on a real transition,
without replay on selection, refresh or remount. Reduced-motion mode is fully static. Detail symbols never supply missing native activity evidence.
