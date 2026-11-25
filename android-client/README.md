# Native Android client: platform identity boundary

This branch does not contain or build the browser launcher, an APK, a working
pairing flow, an Android service, or an Observer UI. It supplies only the
AndroidKeystore identity adapter and tests for its signature wire shape.

## Contracts

- Requires API 31+, P-256 AndroidKeyStore non-exportable keys and local
  TEE/StrongBox security-level evidence. StrongBox may fall back to TEE; software
  keys fail closed. Local reports are not proof to a remote verifier. Desktop
  must verify chain, approved roots and revocation, exact fresh challenge,
  app/signing identity and security levels before installing an identity.
- Each locally produced enrollment UUID has independent alias namespaces for
  endpoint TLS, Relay admission and grant approval. Never derive aliases from
  untrusted QR data, pool a grant between hosts, or send a private key. No scope,
  origin or authorization is inferred from the presence of an alias.
- Endpoint TLS expects DER; Relay proof expects fixed P-256 r||s as does ES256
  COSE. The caller must supply verified, role-correct signing requests. Grant
  approval uses the original biometric CryptoObject and one completion, not a
  naked successful UI event. No device credential downgrade is implemented.
- Keystore operations may block. Do not call them on an async owner poll or UI
  thread. In particular rustls synchronous SigningKey integration must first
  qualify worker isolation, deadline/cancellation and late completion. No JNI
  functions, provider registration, runtime key provisioning, or real key use
  appear on this branch. No production-sensitive data is logged or persisted.

## Pending protocol integration

The existing NativeClient must retain outer WSS and relay proof, inner peer pin,
AppBind, role/scope grant, install ACK and explicit retirement. Current relay
admission needs an already registered public key before Join; a fresh phone
cannot use the existing flow without a separately authorized enrollment path.
Do not serialize a static relay key or private key in an invitation. Invitation
lifetime and installed grant lifetime are separate, and current registration
owner time budgets are not changed by a 1/5/10-minute invitation selection.
Desktop owns invite mint/redeem/approve/revoke; multi-host state must keep these
lifecycles independent. Real Android device acceptance is required, including
software-key rejection, biometric cancel, host approval/expiry/revoke, and
Observer non-interference. No production publication is possible here.

## Local checks

    ./android-client/check-platform.ps1 -Sdk <sdk-directory> -Jdk <jdk-directory>

WSK-MSI existing JDK 21.0.8 and SDK 35 compile the platform adapter. Pure Java
checks cover nine DER cases and 128 real software fixture key roundtrips with
independent JCA P1363 verification, plus wrong-message rejection. This does not
prove Android runtime identity, hardware attestation or interoperability.

- [Android Keystore](https://developer.android.com/privacy-and-security/keystore)
- [Verify Android key attestation](https://developer.android.com/privacy-and-security/security-key-attestation)


## Coordination before wire versioning

Agree the desktop/phone/Relay enrollment schema and compatible version before
adding parsers. The same self-hosted Relay can expose a separate enrollment
endpoint; data-plane admission stays authenticated and default-deny. The manual
code requires a verified user-selected Relay locator if not already known.
Specify authorization and correlation for desktop mint, one-time redemption,
phone proof-of-possession and fresh attestation, desktop explicit approval,
expiry and replay/late response behavior, and independent per-host revocation.
Do not put reusable secrets in the QR, auto-trust a Host ID from the QR, or log
transient invite data. Validate the actual pairing contract and real device
before connecting any Java signer to native transport.
