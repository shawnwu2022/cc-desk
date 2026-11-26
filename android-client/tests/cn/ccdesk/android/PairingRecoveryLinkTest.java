package cn.ccdesk.android;

import java.net.URI;
import java.util.UUID;

public final class PairingRecoveryLinkTest {
    public static void main(String[] args) {
        UUID id = new UUID(5, 9);
        URI relay = URI.create("wss://relay.example/relay");
        String host = String.format("%064x", 1L);
        String wrong = "f" + host.substring(1);
        PairingRecoveryLink exact = new PairingRecoveryLink(id, relay, host);
        if (!exact.accepts(host) || exact.accepts(wrong) || exact.accepts(null))
            throw new AssertionError("QR host must match exactly");
        PairingRecoveryLink manual = new PairingRecoveryLink(id, relay, PairingInvite.fingerprint(host));
        if (!manual.accepts(host) || manual.accepts(wrong) || manual.accepts("BAD"))
            throw new AssertionError("Manual host must match committed fingerprint");
        DesktopBackend.Desktop desktop = exact.approved(host);
        if (!desktop.localId.equals(id) || !desktop.hostId.equals(host) || !desktop.relay.equals(relay))
            throw new AssertionError("Recovery changed identity");
        try { exact.approved(wrong); throw new AssertionError("Accepted wrong receipt"); }
        catch (IllegalArgumentException expected) {}
        System.out.println("PairingRecoveryLink: 4 cases passed");
    }
}
