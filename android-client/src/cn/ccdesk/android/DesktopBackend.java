package cn.ccdesk.android;

import java.net.URI;
import java.util.List;
import java.util.UUID;

/** Internal app contract. No DTO in this file is a wire format or authorization token. */
interface DesktopBackend {
    enum State { DISCONNECTED, WAITING_FOR_DESKTOP, LOADING_SHARES, SHARES_READY, AWAITING_OBSERVER, OBSERVING, CONTROLLING, REVOKED, UNAVAILABLE }

    interface IdentityProvider {
        HardwareIdentity open(UUID localId, HardwareIdentity.Usage usage) throws Exception;
        HardwareIdentity provision(UUID localId, HardwareIdentity.Usage usage, byte[] approvedChallenge)
                throws Exception;
    }

    final class Desktop {
        final UUID localId;
        final String hostId;
        final String name;
        final URI relay;

        Desktop(UUID localId, String hostId, String name, URI relay) {
            if (localId == null || !hostId.matches("[0-9a-f]{64}") ||
                name.isEmpty() || name.length() > 100 ||
                name.codePoints().anyMatch(c -> Character.isISOControl(c) ||
                                                Character.getType(c) == Character.FORMAT))
                throw new IllegalArgumentException("Invalid desktop metadata");
            this.localId = localId;
            this.hostId = hostId;
            this.name = name;
            this.relay = checkedRelay(relay.toString());
        }

        Desktop withAlias(String alias) {
            if (alias == null) throw new IllegalArgumentException("Missing desktop alias");
            return new Desktop(localId, hostId, alias.trim(), relay);
        }

        static URI checkedRelay(String text) {
            URI relay = URI.create(text);
            if (!"wss".equals(relay.getScheme()) || relay.getHost() == null ||
                relay.getRawUserInfo() != null || relay.getRawQuery() != null ||
                relay.getRawFragment() != null || relay.getPort() == 0 || relay.getPort() > 65535 ||
                text.length() > 512 || !text.equals(relay.toASCIIString()) ||
                text.contains("%") || !relay.normalize().equals(relay))
                throw new IllegalArgumentException("Expected WSS URL without credentials");
            return relay;
        }
    }

    enum Role { OBSERVER, CONTROLLER }

    final class Offer {
        final long owner;
        final String id;
        final String label;
        final Role role;
        Offer(long owner, String id, String label, Role role) {
            this.owner = owner; this.id = id; this.label = label; this.role = role;
        }
    }

    final class Session {
        final String id;
        final String title;
        final String text;
        final long frameId;
        Session(String id, String title, String text, long frameId) {
            this.id = id; this.title = title; this.text = text; this.frameId = frameId;
        }
    }

    // All callbacks and worker ownership are to be implemented by the real native
    // backend; completion must be checked against the current per-host owner.
    interface EnrollmentCallback {
        void approved(Desktop desktop);
        void state(State state);
    }
    // Copy only the needed code synchronously; caller zeroizes its invite after return.
    // Verify WSS/WebPKI, the phone-bound desktop signature and exact expected Host
    // before persisting a native receipt or reporting approved; never trust UI data.
    void redeemInvite(PairingInvite invite, IdentityProvider keys, EnrollmentCallback callback);
    State state(Desktop desktop);
    List<Session> observerSessions(Desktop desktop);
    void close();
}
