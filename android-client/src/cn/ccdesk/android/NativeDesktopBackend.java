package cn.ccdesk.android;

import android.app.Activity;
import java.nio.ByteBuffer;
import java.security.SecureRandom;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ScheduledThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;

/** Android UI driver. Only the real retained Rust owner grants eligibility. */
final class NativeDesktopBackend implements DesktopBackend {
    private final Activity activity;
    private final Runnable onChanged;
    private final ScheduledThreadPoolExecutor executor = new ScheduledThreadPoolExecutor(1);
    private final AtomicBoolean closed = new AtomicBoolean();
    private final Map<UUID, Connection> paired = new ConcurrentHashMap<>();
    private Attempt pending;

    private NativeDesktopBackend(Activity activity, Runnable onChanged) {
        this.activity = activity;
        this.onChanged = onChanged;
        executor.setExecuteExistingDelayedTasksAfterShutdownPolicy(false);
    }

    static NativeDesktopBackend load(Activity activity, Runnable onChanged) {
        try {
            System.loadLibrary("cc_desk_android_native");
            // Calls a real symbol when loading so unsuitable/mismatched libraries cannot activate.
            if (nativeAndroidAbiVersion() != 1 || nativePairingState(0) != 0) return null;
            return new NativeDesktopBackend(activity, onChanged);
        } catch (SecurityException | LinkageError ignored) {
            return null;
        }
    }

    @Override public void redeemInvite(PairingInvite invite, IdentityProvider keys,
            EnrollmentCallback callback) {
        if (closed.get()) { callback.state(State.UNAVAILABLE); return; }
        char[] chars = invite.copyCode();
        byte[] code = new byte[chars.length];
        for (int i = 0; i < chars.length; i++) code[i] = (byte) chars[i];
        Arrays.fill(chars, (char)0);
        executor.execute(() -> {
            cancelPending();
            if (closed.get()) { Arrays.fill(code, (byte)0); return; }
            UUID id = UUID.randomUUID();
            Attempt attempt = new Attempt(invite, id, callback);
            pending = attempt;
            try {
                // Local challenge only; never present it as a remotely verified hardware attestation.
                HardwareIdentity endpoint = provision(keys, id, HardwareIdentity.Usage.ENDPOINT);
                HardwareIdentity relay = provision(keys, id, HardwareIdentity.Usage.RELAY);
                HardwareIdentity approval = provision(keys, id, HardwareIdentity.Usage.APPROVAL);
                attempt.facade = new NativeKeystoreBridge(activity, id, endpoint, relay, approval);
                if (closed.get()) { cancelPending(); return; }
                attempt.handle = nativeBeginPairing(attempt.facade, localId(id),
                    invite.relay.toString(), code, invite.hostCommitment(), endpoint.spki(),
                    relay.spki(), approval.spki());
                if (attempt.handle == 0) { fail(attempt); return; }
                callback.state(State.WAITING_FOR_DESKTOP);
                schedule(() -> pollEnrollment(attempt));
            } catch (Exception | LinkageError denied) { fail(attempt); }
            finally { Arrays.fill(code, (byte)0); }
        });
    }

    private void pollEnrollment(Attempt attempt) {
        if (closed.get() || pending != attempt) return;
        try {
            int status = nativePairingState(attempt.handle);
            if (status == 1) { schedule(() -> pollEnrollment(attempt)); return; }
            if (status != 2) { fail(attempt); return; }
            String host = nativePairingHostId(attempt.handle);
            if (!attempt.invite.matchesHost(host)) { fail(attempt); return; }
            Desktop desktop = new Desktop(attempt.id, host, "电脑 " + host.substring(0, 8),
                attempt.invite.relay);
            paired.put(attempt.id, new Connection(desktop, attempt.handle, attempt.facade));
            pending = null;
            attempt.callback.approved(desktop);
        } catch (RuntimeException | LinkageError e) { fail(attempt); }
    }

    private void fail(Attempt attempt) {
        if (pending == attempt) cancelPending();
        if (!closed.get()) attempt.callback.state(State.UNAVAILABLE);
    }

    void restore(List<Desktop> candidates, IdentityProvider keys) {
        if (closed.get()) return;
        executor.execute(() -> {
            for (Desktop desktop : candidates) {
                if (closed.get()) return;
                NativeKeystoreBridge facade = null;
                try {
                    HardwareIdentity endpoint = keys.open(desktop.localId, HardwareIdentity.Usage.ENDPOINT);
                    HardwareIdentity relay = keys.open(desktop.localId, HardwareIdentity.Usage.RELAY);
                    HardwareIdentity approval = keys.open(desktop.localId, HardwareIdentity.Usage.APPROVAL);
                    facade = new NativeKeystoreBridge(activity, desktop.localId, endpoint, relay, approval);
                    String verified = nativeReadPairedHostId(facade, localId(desktop.localId),
                        desktop.relay.toString(), endpoint.spki(), relay.spki(), approval.spki());
                    if (desktop.hostId.equals(verified)) {
                        paired.putIfAbsent(desktop.localId, new Connection(desktop, 0, null));
                        changed();
                    }
                } catch (Exception | LinkageError denied) {
                    // 未验签或本地硬件钥匙不匹配：不从 UI 元数据恢复。
                } finally {
                    if (facade != null) facade.close();
                }
            }
        });
    }

    boolean verified(Desktop desktop) {
        Connection connection = paired.get(desktop.localId);
        return connection != null && connection.desktop.hostId.equals(desktop.hostId) &&
            connection.desktop.relay.equals(desktop.relay);
    }

    @Override public State state(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        return verified(desktop) ? c.state : State.UNAVAILABLE;
    }

    @Override public List<Session> observerSessions(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        Session current = c == null ? null : c.snapshot;
        return verified(desktop) && current != null ? Collections.singletonList(current) :
            Collections.emptyList();
    }

    boolean canObserve(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        return verified(desktop) && c.pairing != 0 && c.observer == 0 && !c.consumed.get();
    }

    void startObserver(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        if (!canObserve(desktop) || !c.consumed.compareAndSet(false, true)) return;
        executor.execute(() -> {
            if (closed.get()) return;
            try {
                c.observer = nativeStartPairedObserver(c.pairing);
                c.state = c.observer == 0 ? State.UNAVAILABLE : State.AWAITING_OBSERVER;
                changed();
                if (c.observer != 0) schedule(() -> pollObserver(c));
            } catch (RuntimeException | LinkageError denied) { stopObserver(c); }
        });
    }

    private void pollObserver(Connection c) {
        if (closed.get() || c.observer == 0) return;
        try {
            int state = nativeObserverState(c.observer);
            if (state == 1) { schedule(() -> pollObserver(c)); return; }
            if (state != 2) { stopObserver(c); return; }
            String text = nativeTakeObserverText(c.observer);
            if (text != null) c.snapshot = new Session("current-observer", "已批准的会话", text);
            if (c.state != State.OBSERVING || text != null) {
                c.state = State.OBSERVING;
                changed();
            }
            schedule(() -> pollObserver(c));
        } catch (RuntimeException | LinkageError e) { stopObserver(c); }
    }

    void disconnectObserver(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        if (!closed.get() && verified(desktop)) executor.execute(() -> stopObserver(c));
    }

    private void stopObserver(Connection c) {
        if (c.observer != 0) nativeCancelObserver(c.observer);
        c.observer = 0;
        c.snapshot = null;
        c.state = State.DISCONNECTED;
        changed();
    }

    private void changed() {
        activity.runOnUiThread(() -> { if (!closed.get() && !activity.isDestroyed()) onChanged.run(); });
    }

    private void schedule(Runnable action) {
        if (!closed.get()) executor.schedule(action, 400, TimeUnit.MILLISECONDS);
    }

    private void cancelPending() {
        Attempt attempt = pending;
        pending = null;
        if (attempt == null) return;
        if (attempt.handle != 0) nativeCancelPairing(attempt.handle);
        if (attempt.facade != null) attempt.facade.close();
    }

    void cancelEnrollment() {
        if (!closed.get()) executor.execute(this::cancelPending);
    }

    void discardUnpublished(Desktop desktop) {
        if (closed.get()) return;
        executor.execute(() -> {
            Connection c = paired.remove(desktop.localId);
            if (c == null) return;
            stopObserver(c);
            if (c.pairing != 0) nativeCancelPairing(c.pairing);
            if (c.facade != null) c.facade.close();
        });
    }

    @Override public void close() {
        if (!closed.compareAndSet(false, true)) return;
        executor.execute(() -> {
            try {
                cancelPending();
                for (Connection c : paired.values()) {
                    stopObserver(c);
                    if (c.pairing != 0) nativeCancelPairing(c.pairing);
                    if (c.facade != null) c.facade.close();
                }
            } finally { executor.shutdown(); }
        });
    }

    private static HardwareIdentity provision(IdentityProvider keys, UUID id, HardwareIdentity.Usage usage)
            throws Exception {
        byte[] challenge = new byte[32];
        new SecureRandom().nextBytes(challenge);
        return keys.provision(id, usage, challenge);
    }

    private static byte[] localId(UUID id) {
        return ByteBuffer.allocate(16).putLong(id.getMostSignificantBits())
            .putLong(id.getLeastSignificantBits()).array();
    }

    private static final class Attempt {
        final PairingInvite invite;
        final UUID id;
        final EnrollmentCallback callback;
        long handle;
        NativeKeystoreBridge facade;
        Attempt(PairingInvite invite, UUID id, EnrollmentCallback callback) {
            this.invite = invite; this.id = id; this.callback = callback;
        }
    }

    private static final class Connection {
        final Desktop desktop;
        final long pairing;
        final NativeKeystoreBridge facade;
        final AtomicBoolean consumed = new AtomicBoolean();
        volatile long observer;
        volatile State state = State.DISCONNECTED;
        volatile Session snapshot;
        Connection(Desktop desktop, long pairing, NativeKeystoreBridge facade) {
            this.desktop = desktop; this.pairing = pairing; this.facade = facade;
        }
    }

    static native int nativeAndroidAbiVersion();
    static native long nativeBeginPairing(NativeKeystoreBridge facade, byte[] localId, String canonicalWss,
        byte[] code, byte[] hostCommitment, byte[] endpointSpki, byte[] relaySpki, byte[] approvalSpki);
    static native int nativePairingState(long handle);
    static native String nativePairingHostId(long handle);
    static native void nativeCancelPairing(long handle);
    static native long nativeStartPairedObserver(long handle);
    static native String nativeReadPairedHostId(NativeKeystoreBridge facade, byte[] localId,
        String canonicalWss, byte[] endpointSpki, byte[] relaySpki, byte[] approvalSpki);
    static native int nativeObserverState(long handle);
    static native byte[][] nativeTakeObserverSnapshot(long handle);
    static native String nativeTakeObserverText(long handle);
    static native void nativeCancelObserver(long handle);
}
