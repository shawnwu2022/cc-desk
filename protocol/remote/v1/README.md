# Remote outer framing v1 candidate

This dependency-free crate is the single main-repository source for the outer mux
envelope. It is consumed by the remote transport and eventually vendored unchanged
into relay under a pinned bundle digest. It contains no TLS, keys, admission, grants,
PTY or platform dependencies. It is not the complete inner protocol or protocol freeze.

One binary WSS message is one envelope. All integers are unsigned big endian.

| Field | Width | Value / rule |
| --- | --- | --- |
| major | 1 byte |1; unknown rejects |
| kind | 1 byte |1 (opaque tunnel byte data); unknown rejects |
| count | 2 bytes |0..32 inclusive |
| per-subframe tunnel |16 bytes |Opaque routing handle, not an identity or authorization |
| per-subframe length |4 bytes |Number of payload bytes following this header |
| per-subframe payload |length bytes |Opaque encrypted stream fragment |

Total envelope size including all headers is at most65536 bytes. No trailing bytes,
truncated header/payload, overflow, count mismatch or larger envelope is accepted.
Single-subframe maximum payload is65512 bytes. Duplicate/all-zero handles are valid
codec values: route ownership and duplicates are enforced by the admitted mux actor,
not inferred from byte shape. Payload boundaries do not imply TLS record boundaries.

Explicit v1 ruling: count0 is a4-byte no-op envelope; zero-length subframes are allowed
but each still consumes one of32 slots and its20-byte header. They grant no permission,
create no routes and must still count against ingress message/rate limits. There is no
special keepalive, route creation or heartbeat authorization hidden in empty data.

Decoder borrows payloads and allocates at most32 metadata entries, never an allocation
derived from a declared payload length. The WSS layer must apply its own65,536-byte
message/parser limits before buffering: this codec cannot undo an upstream allocation.
Encoder validates the full size before allocating its at-most65,536-byte output.
Debug output of Subframe redacts bytes and handles; errors carry fixed categories.

`vectors/one-frame.hex` is a reviewed literal: route000102030405060708090a0b0c0d0e0f,
payloadaabbcc. Tests compare independent byte values, not just encoder round-trips.

```powershell
cargo test --locked --manifest-path protocol/remote/v1/Cargo.toml
cargo clippy --locked --manifest-path protocol/remote/v1/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path protocol/remote/v1/Cargo.toml -- --check
```

No rollout, peer compatibility, auth, WSS or capacity claim follows from codec tests.
