package cn.ccdesk.android;

import java.util.UUID;

/** Single-process mailbox. Activity intents carry only the owner ID, never QR payload. */
final class ScanMailbox {
    private static UUID owner;
    private static PairingInvite invite;
    static synchronized UUID start() {
        clear(owner);
        owner = UUID.randomUUID();
        return owner;
    }
    static synchronized boolean complete(UUID request, PairingInvite result) {
        if (request == null || !request.equals(owner) || invite != null) return false;
        invite = result;
        return true;
    }
    static synchronized PairingInvite take(UUID request) {
        if (request == null || !request.equals(owner)) return null;
        PairingInvite result = invite;
        invite = null;
        owner = null;
        return result;
    }
    static synchronized void clear(UUID request) {
        if (request != null && request.equals(owner)) {
            if (invite != null) invite.close();
            invite = null;
            owner = null;
        }
    }
    private ScanMailbox() {}
}
