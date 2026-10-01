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

## Codex CLI

- Owner: OpenAI; Copyright 2025 OpenAI
- Source: the Codex CLI's own sign-in-page favicon and “Open Codex” mark:
  https://github.com/openai/codex/blob/d6c3b448a41311ece3255c52ec3dbfd9ff36f154/codex-rs/login/src/assets/success.html
- `codex.svg` preserves the original 32 × 32 viewBox, path and stroke dimensions;
  only the display width/height are set to 16 × 16. Black is shown in the light
  theme and white in the dark theme, consistent with the upstream monochrome
  `currentColor` treatment. Session status never recolors either application.
- Path SHA-256: `d172e73cdb5075fb000dc82eee4573cae1a905d1e612eabeefab765173cd1255`
- The source repository is Apache-2.0 licensed; the license is included in
  `LICENSE-OpenAI.txt`. The relevant upstream notice is Copyright 2025 OpenAI.
  Apache-2.0 does not grant trademark rights. Brand terms:
  https://openai.com/brand/

## Accessibility and rendering

The shared component keeps the full accessible names “Claude Code” / “Codex
CLI”, keyboard focus and tooltips. Decorative images have empty alt text so
screen readers do not announce the application twice. `CC` / `CX` are only
image-loading-error fallbacks. Codex's monochrome contrast is tested against
both themes and selected rows; Claude retains the official brand color rather
than claiming the previous neutral-ink 3:1 guarantee for its logo.

Assets are local and contain only static SVG geometry; rendering does not
request provider sites or execute SVG code.
