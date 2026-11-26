package cn.ccdesk.android;

import java.net.URI;
import java.util.UUID;

/** Journal hint only; cannot make a device eligible without native receipt validation. */
final class PairingRecoveryLink {
    final UUID id;
    final URI relay;
    final String commitment;

    PairingRecoveryLink(UUID id, URI relay, String commitment) {
        if (id == null || id.equals(new UUID(0, 0)) || commitment == null
            || !commitment.matches("[0-9a-f]{64}|[0-9A-HJKMNP-TV-Z]{20}"))
            throw new IllegalArgumentException("Invalid pairing recovery link");
        this.id = id;
        this.relay = DesktopBackend.Desktop.checkedRelay(relay.toString());
        this.commitment = commitment;
    }

    boolean accepts(String verifiedHost) {
        return verifiedHost != null && verifiedHost.matches("[0-9a-f]{64}")
            && (commitment.length() == 64 ? commitment.equals(verifiedHost)
                : commitment.equals(PairingInvite.fingerprint(verifiedHost)));
    }

    DesktopBackend.Desktop approved(String verifiedHost) {
        if (!accepts(verifiedHost)) throw new IllegalArgumentException("Unverified pairing host");
        return new DesktopBackend.Desktop(id, verifiedHost, "电脑 " + verifiedHost.substring(0, 8), relay);
    }
}
