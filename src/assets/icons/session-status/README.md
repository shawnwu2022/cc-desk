# CC Desk session status shapes

These six SVGs were created specifically for CC Desk and are self-owned
project artwork distributed under the repository's MIT license. They contain
only local static geometry, with no external artwork, references or scripts.

| File | Shape identifier | Outline |
|---|---|---|
| `starting.svg` | `gap-ring` | Gapped circular ring |
| `running.svg` | `active-play` | Circle containing a play shape |
| `needs-user.svg` | `reply-dot` | Conversation bubble containing a filled dot |
| `confirming.svg` | `question-diamond` | Diamond containing a question mark |
| `ended.svg` | `stop-square` | Filled square stop symbol |
| `failed.svg` | `alert-triangle` | Triangle containing an exclamation mark |

The application supplies semantic color using `currentColor`. Shape remains
the primary differentiator; color is supplementary. SVGs are decorative inside
one labelled, keyboard-focusable tooltip trigger. All motion lives in the Vue
component stylesheet, including the reduced-motion override.
