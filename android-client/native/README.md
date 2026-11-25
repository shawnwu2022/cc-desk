# Android native Observer transport

This is the native transport owner, not an enrollment implementation or an APK
runtime. Only the backend's current authenticated enrollment owner may hand
`start_verified_observer` a `NativeProfile`. Do not assemble it from a QR payload,
displayed metadata, code redemption, cached labels, or an admission result.
The Android app must stay unavailable until authenticated durable enrollment,
the Keystore signer implementations and the Java backend are wired and verified.

The authenticated input includes an independently pinned desktop TLS key,
owner-approved exact Observer session scope, canonical Relay and client/host key
epochs, a bounded tunnel, and distinct Endpoint, Relay and verified local
presence signing authorities. The transport uses the existing pinned TLS,
authenticated app binding, grant, and peer installation. It provides no new key
export, fallback signer, interop acceptance flag, input/control command, or
automatic reconnect. A later deliberate start needs a newly authorized profile;
keeping a durable pairing alone does not authorize a session.

`NativeDesktopBackend` JNI uses a process-local handle delivered by the trusted
Rust backend after starting that exact owner:

- `nativeObserverState(long): int` returns 0 closed or unknown, 1 connecting,
  2 authenticated receipt available or consumed, 3 failed or retired.
- `nativeTakeObserverSnapshot(long): byte[][]` takes the one bounded, authenticated
  Observer snapshot once. `null` means no snapshot; nothing is acknowledged.
  The Java renderer must validate/render the exact protocol and clear on retirement.
- `nativeCancelObserver(long): void` hides any pending snapshot and requests
  owner retirement. Return does **not** mean the native transport already joined.
  No next owner is allowed until the previous task and both original native
  transport owners have verifiably retired.

For local development, check the Android target with a pinned Rust 1.100.0 and
the Android NDK toolchain for the app's minimum API:
`cargo +1.100.0 check --target aarch64-linux-android --locked --offline`.
A production native build must further link, satisfy 16 KiB ELF segment
alignment, package as the exact signed APK, and install/run on supported Android.
There is no Android/desktop enrollment or full runtime verification in this crate.
