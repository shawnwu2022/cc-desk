# alacritty_terminal 0.26.0 provenance

- Source: crates.io registry package `alacritty_terminal` version `0.26.0`.
- Cargo.lock SHA-256 checksum: `bda177466b9524d59f1b12f0dd30b68696788e9992a7e959021c4a0ed96fcf59`.
- Package archive SHA-256 independently observed after `cargo info`: the same value.
- License declared by the package and included as `LICENSE-APACHE`: Apache-2.0.
- Upstream repository: `https://github.com/alacritty/alacritty`.
- `.cargo_vcs_info.json` commit: `94e7c8874e526b1e67b349d9ba30ddf81669119e`;
  `path_in_vcs`: `alacritty_terminal`.
- Rust version declared by the normalized registry package: `1.85.0`.
- Local dependency declaration: exact `=0.26.0`, optional, default features disabled.
- Upstream vte declaration: `0.15.0`, default features disabled, features `std` and
  `ansi`. The workspace `[patch.crates-io]` forces that edge to `remote/vendor/vte`.

This dependency is not vendored. Cargo verifies the registry archive against the
lockfile checksum. The local source tree is not modified.

## Reviewed source boundaries

- `Term::new` allocates both active and inactive grids immediately, so local geometry
  validation runs before construction.
- `Config.scrolling_history` is explicitly zero for this checkpoint.
- `CellExtra.zerowidth` is an unbounded `Vec<char>`; the16-scalar target is OPEN.
- Titles and hyperlink identifiers/URIs are owned strings. The1KiB title and2KiB URI/
  256-link budgets are OPEN.
- `log` trace sites include title and hyperlink values. No global logger override is
  installed. The experimental gate rejects OSC/string controls before handler
  dispatch, so these payload-bearing sites are unreachable in this slice; real PTY
  attachment remains blocked pending the full log/handler privacy audit.
- Synchronized update, DCS, reflow, history, and CPU budgets are not certified here.
