# CLI application marks used by CC Desk

These bundled assets identify the application behind each session. They are
third-party brand assets, not CC Desk artwork, and are not relicensed under the
repository's MIT license. Their use does not imply endorsement or affiliation.
Both were checked against the official sources on 2026-10-01.

## Claude Code

- Owner: Anthropic, PBC
- Source: https://claude.com/ — the orange starburst path in the official
  `svg[aria-label="Claude"]` wordmark
- `claude.svg` contains that starburst path unchanged, in its original 125 × 125
  coordinate space and original `#D97757` color. The separate wordmark lettering
  is omitted; the mark is displayed at 16 × 16 CSS pixels.
- Path SHA-256: `055f133268cfc756c83c8731e02b234d522d27bdb7745bb46eb5439de61cc7dc`
- Brand rights and usage terms:
  https://www.anthropic.com/legal/trademark-guidelines
- No open-source license or trademark permission is inferred from public
  availability. Anthropic retains its rights in the Claude mark.

## Codex CLI: ChatGPT / OpenAI knot

- Owner: OpenAI
- The user-selected visual identifier is the recognizable ChatGPT / OpenAI
  knot (officially called the Blossom); the application name remains “Codex CLI”.
- Official source page and brand usage terms: https://openai.com/brand/
- Source artwork: the publicly displayed `Blossom_Light.svg` on that page:
  https://images.ctfassets.net/kftzwdyauwt9/3hUGLn3ypllZ0oa01qOYVq/28e8188e6f11b84c3e876569d492734f/Blossom_Light.svg?w=3840&q=90
- Source SVG SHA-256: `01485e70cea6df8422f5abc643fbbd3c153442cc41da0e7d8e7451801ebf26e2`
- `codex.svg` contains the source's first black-filled Blossom path unchanged.
  The surrounding spacing guides and duplicate diagram are omitted; the viewBox
  frames that path in its original coordinates (`146.694 227.042 267.198 264.812`).
  The original proportions are preserved at 16 × 16 CSS pixels, with black in
  the light theme and white in the dark theme. Session status never recolors it.
- Path SHA-256: `fb0a32a5384df5cdacc7d1a304f5b14df3c38a9a41ed08878adc99653efd0a33`
- This replaces the previous Codex terminal glyph sourced from the Apache-2.0
  Codex repository. That repository license does not apply to this brand-page
  artwork. No open-source license or additional trademark rights are inferred
  from public availability; OpenAI retains its rights in the mark. It identifies
  the OpenAI CLI inside CC Desk and is not CC Desk's own branding.

## Accessibility and rendering

The shared component keeps the full accessible names “Claude Code” / “Codex
CLI”, keyboard focus and tooltips. Decorative images have empty alt text so
screen readers do not announce the application twice. `CC` / `CX` are only
image-loading-error fallbacks. Codex's monochrome contrast is tested against
both themes and selected rows; Claude retains the official brand color rather
than claiming the previous neutral-ink 3:1 guarantee for its logo.

Assets are local and contain only static SVG geometry; rendering does not
request provider sites or execute SVG code.
