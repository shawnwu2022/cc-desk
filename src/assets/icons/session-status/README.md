# CC Desk session status shapes

These SVGs were created specifically for CC Desk and are self-owned
project artwork distributed under the repository's MIT license. They contain
only local static geometry, with no external artwork, references or scripts.

| File | Shape identifier | Outline |
|---|---|---|
| `starting.svg` | `gap-ring` | Gapped circular ring |
| `running.svg` | `idle-dot` | Static solid center, process running with known idle activity |
| `working.svg` | `work-dot` | Solid center and dashed ring, green pulse supplied by the component |
| `permission.svg` | `permission-bars` | Pause bars with gold pulse supplied by the component |
| `completed.svg` | `completed-ring` | Hollow green rings, explicit response completion |
| `stopped.svg` | `stopped-dot` | Solid gray center for the selected retained terminal |
| `closed.svg` | `closed-ring` | Hollow gray ring for closed history or unselected stopped terminal |
| `unknown.svg` | `unknown-dot` | Small gray center for unavailable/unordered turn activity |
| `needs-user.svg` | `reply-dot` | Circle containing a filled attention dot |
| `confirming.svg` | `question-circle` | Circle containing a question mark |
| `ended.svg` | `stop-circle` | Circle containing a filled square stop symbol |
| `failed.svg` | `alert-circle` | Circle containing an exclamation mark |

The sticky turn-error state reuses the bundled `failed.svg` geometry with its
own localized error label. Launch failure remains a separate lifecycle state.

All states share a circular silhouette. The application supplies semantic color
using `currentColor`; the spinner gap and internal marks also distinguish states
without relying on color. SVGs are decorative inside
one labelled, keyboard-focusable tooltip trigger. All motion lives in the Vue
component stylesheet, including the reduced-motion override.
