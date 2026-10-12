# CC Desk session status shapes

These local, static SVGs are CC Desk-owned project artwork under the repository's
MIT license. They contain no external artwork, links, scripts or runtime markup.
All shapes share a solid 16px circular backplate filled by semantic `currentColor`.
Contrasting marks use the theme background ink, so meaning never depends on color.

- starting: clock; running/known idle: plain solid dot
- working: sparkle; thinking: dots; tool execution: hammer
- subagent: simple Y branch; compacting: inward arrows
- needs-user/pending and waiting input: reply bubble; permission: lock
- explicit completion: check; sticky error/failed launch: alert with separate labels
- stopped/ended: square; closed: cross; archived: archive box
- unknown activity: question; uncertain process: confirmation clock

Existing data-shape identifiers remain stable where possible; names containing
ring/dot are historical IDs and no longer describe hollow artwork. Each asset is
allowlisted in SessionStatusIcon and is decorative inside one localized accessible
Tooltip trigger. Known starting and working have slight stylesheet breathing; thinking dots brighten
sequentially. Permission/input entry and completion animate once on a real transition,
without replay on selection, refresh or remount. Reduced-motion mode is fully static. Detail symbols never supply missing native activity evidence.

Session rows embed these shapes decoratively in SessionAppIcon's approximately
8px solid corner badge (10px SVG box), inside one 20px application slot. Embedded
shapes are static; only the recognizable app mark breathes by opacity for 2.6s
while working, or brightens once for 650ms when completion enters. The combined
Tooltip/accessible name retains CLI, current state and independent receipt facts.
