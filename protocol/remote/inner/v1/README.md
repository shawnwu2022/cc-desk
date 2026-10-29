# Endpoint raw record framing (E01 prerequisite)

Shared endpoint framing lives in the main cc-desk repository. The relay consumes
only the existing outer bundle; it must not link this crate or parse inner records.
The framing API is consumed by the actual async transport adapter. The separate
AppBind and [projection trial codec](PROJECTION.md) validate bounded payload
syntax; none of these modules grants endpoint authority.

Byte layout: `length:BEu32 | type:BEu16 | minor:BEu16 | opaque payload`.
Length counts type+minor+payload, excluding its own prefix. Type2 Control and type4
AppBind payloads are at most16384 bytes (16392 total); type3 Projection is at
most32768 (32776 total). Bootstrap AppBind uses minor0 independently of the
selected data minor. Control/Projection require the caller's exact selected0/1;
unknown types/minors and oversized or short lengths reject before body allocation.
Empty payload is structurally valid here, not proof of a valid JSON record.

This is framing, not a protocol state machine. Payload remains untrusted. The
enclosing E01/E02 owner must enforce phase, JSON schema/UTF8/depth/field limits,
negotiation/security floors, peer/target/context binding and both AppBind MACs
before dispatch. In particular, parsing a bootstrap record on a selected-minor
reader never permits re-binding an established session. N/N-1 negotiation and
complete operation DTOs are not certified by these framing tests.

`cargo test --locked --manifest-path protocol/remote/inner/v1/Cargo.toml`
executes independent byte-oracle and boundary cases. The existing outer-only
protocol source, lock and relay vendored bundle remain unchanged.
