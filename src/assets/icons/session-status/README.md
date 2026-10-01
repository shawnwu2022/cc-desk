# CC Desk session status shapes

These six SVGs were created specifically for CC Desk and are self-owned
project artwork distributed under the repository's MIT license. They contain
only local static geometry, with no external artwork, references or scripts.

| File | Shape identifier | Outline |
|---|---|---|
| `starting.svg` | `gap-ring` | Gapped circular ring |
| `running.svg` | `active-play` | Circle containing a play shape |
| `needs-user.svg` | `reply-dot` | Circle containing a filled attention dot |
| `confirming.svg` | `question-circle` | Circle containing a question mark |
| `ended.svg` | `stop-circle` | Circle containing a filled square stop symbol |
| `failed.svg` | `alert-circle` | Circle containing an exclamation mark |

All states share a circular silhouette. The application supplies semantic color
using `currentColor`; the spinner gap and internal marks also distinguish states
without relying on color. SVGs are decorative inside
one labelled, keyboard-focusable tooltip trigger. All motion lives in the Vue
component stylesheet, including the reduced-motion override.
