# CC Desk vte provenance

- Crate: `vte 0.15.0`
- Registry: `https://crates.io/crates/vte/0.15.0`
- Repository: `https://github.com/alacritty/vte`
- Upstream VCS revision recorded by the crate: `3b3da71c34cc1256c7e20981cf03f8eb95e08ffc`
- Original `.crate` SHA-256: `a5924018406ce0063cd67f8e008104968b74b563ee1b85dde3ed1f7cb87d3dbd`
- License: `Apache-2.0 OR MIT`; the unmodified upstream license files are retained.
- Retrieved with: `cargo info vte@0.15.0 --verbose` from the official crates.io registry.
- Local change: `src/lib.rs` only, recorded by `../../patches/vte-osc-bound.patch`.

The local patch adds a 4096-byte pre-append OSC payload limit for the `std`
parser, discards an overlong OSC through BEL or ST without dispatching a
truncated command, and exposes a sticky overflow signal for the CC Desk
terminal adapter. It also exposes read-only pending string-sequence state and a
sticky/take entry signal, so a restricted caller can reject OSC/DCS/SOS/PM/APC even
when the sequence is terminated or cancelled in the same batch before forwarding to
a richer ANSI handler. The vendored source remains an experimental E04 input and is
not evidence that full terminal emulator semantics or all parser resources are bounded.

The 4096-byte overflow behavior and its public observation helpers are compiled
only with `std`. The upstream `no_std` const-generic `ArrayVec` behavior remains
unchanged: buffers below the input length dispatch their truncated capacity,
while a larger configured buffer can dispatch more than 4096 bytes.
