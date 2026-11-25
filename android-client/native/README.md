# Android native Observer owner

The Android application supplies its three verified hardware identities for one
explicit pairing action. The native worker makes one authenticated Relay claim,
reconciles ambiguous delivery by polling only that request ID, and verifies the
desktop’s signature against the commitment carried by its QR or full pairing
string. Relay admission is not desktop authorization. A paired result is
published only after the platform commits the bounded signed receipt to its
hardware-sealed no-backup store.

A persisted receipt can reauthenticate the desktop identity against the current
three Keystore roles. It cannot reauthorize the original session. Pairing may
contain no shared session; it must not construct a connection in that case.
A separate explicit viewing action can consume an exact still-current scope.
No wildcard, fallback signer, automatic reconnect, keyboard, control, ACK, or
generic command surface is exposed.

The Java backend uses the private `NativeDesktopBackend` JNI contract for
starting and polling a pairing, independently rechecking stored identity,
explicitly starting the paired scope, and taking one bounded plain-text
authenticated Observer snapshot. It clears the view on retirement. Native
cancellation requests shutdown and hides pending data; it does not assert the
original TLS/WSS/provider work already joined.

Use a pinned Rust 1.100.0 plus the Android NDK toolchain:
`cargo +1.100.0 check --target aarch64-linux-android --locked --offline`.
A release additionally requires a modern Android NDK link, verified 16 KiB ELF
and APK native segment/entry alignment, signing, and combined real-device
validation. Do not infer hardware, installation, or deployment readiness from
a compile-only check.
