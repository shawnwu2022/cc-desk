package cn.ccdesk.android;

import android.app.Activity;
import android.hardware.biometrics.BiometricManager;
import android.hardware.biometrics.BiometricPrompt;
import android.os.CancellationSignal;
import android.os.Handler;
import android.os.Looper;
import java.security.GeneralSecurityException;
import java.util.Arrays;
import java.util.UUID;

/** Trusted native role adapter; callbacks never construct approval claims from UI. */
final class NativeKeystoreBridge {
    private final Activity activity;
    private final HardwareIdentity endpoint;
    private final HardwareIdentity relay;
    private final HardwareIdentity approval;
    private final AndroidPairingReceiptStore receipts;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final java.util.concurrent.ExecutorService worker =
        java.util.concurrent.Executors.newSingleThreadExecutor();
    private long pendingId;
    private CancellationSignal signal;
    private HardwareIdentity.PendingApproval pendingOperation;
    private boolean closed;

    NativeKeystoreBridge(Activity activity, UUID localId, HardwareIdentity endpoint,
            HardwareIdentity relay, HardwareIdentity approval) throws GeneralSecurityException {
        if (!endpoint.belongsTo(localId, HardwareIdentity.Usage.ENDPOINT) ||
            !relay.belongsTo(localId, HardwareIdentity.Usage.RELAY) ||
            !approval.belongsTo(localId, HardwareIdentity.Usage.APPROVAL)) {
            throw new GeneralSecurityException("Identity role mismatch");
        }
        this.activity = activity;
        this.endpoint = endpoint;
        this.relay = relay;
        this.approval = approval;
        this.receipts = new AndroidPairingReceiptStore(activity, localId);
    }

    byte[] signEndpointTls(byte[] input) throws GeneralSecurityException {
        return endpoint.signEndpointTls(input);
    }

    byte[] signRelay(byte[] input) throws GeneralSecurityException {
        return relay.signRelay(input);
    }

    boolean sealPairingRecord(String lowerHexLocalId, byte[] record) {
        return receipts.insertOnce(lowerHexLocalId, record);
    }

    byte[] readPairingRecord(String lowerHexLocalId) {
        return receipts.readSealed(lowerHexLocalId);
    }

    void requestApproval(long requestId, byte[] tbs, String session, String run, String generation) {
        if (requestId == 0 || tbs == null || tbs.length == 0 || tbs.length > 65536 ||
            !safeLabel(session) || !safeLabel(run) || !safeLabel(generation)) {
            nativeCompleteApproval(requestId, null);
            return;
        }
        byte[] captured = tbs.clone();
        if (!main.post(() -> beginPrompt(requestId, captured))) {
            Arrays.fill(captured, (byte)0);
            nativeCompleteApproval(requestId, null);
        }
    }

    private void beginPrompt(long requestId, byte[] tbs) {
        if (closed || pendingId != 0 || activity.isFinishing() || activity.isDestroyed()) {
            Arrays.fill(tbs, (byte)0);
            nativeCompleteApproval(requestId, null);
            return;
        }
        signal = new CancellationSignal();
        pendingId = requestId;
        worker.execute(() -> {
            HardwareIdentity.PendingApproval operation = null;
            try { operation = approval.prepareApproval(tbs); }
            catch (GeneralSecurityException | RuntimeException ignored) {}
            finally { Arrays.fill(tbs, (byte)0); }
            HardwareIdentity.PendingApproval prepared = operation;
            if (!main.post(() -> showPrompt(requestId, prepared)) && prepared != null) prepared.close();
        });
    }

    private void showPrompt(long requestId, HardwareIdentity.PendingApproval operation) {
        if (pendingId != requestId || closed) {
            if (operation != null) operation.close();
            return;
        }
        if (operation == null || activity.isFinishing() || activity.isDestroyed()) {
            settle(requestId, null, true);
            return;
        }
        pendingOperation = operation;
        try {
            BiometricPrompt prompt = new BiometricPrompt.Builder(activity)
                .setTitle("确认本次远程观察")
                .setSubtitle("仅允许观察已在电脑上批准的会话")
                .setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG)
                .setNegativeButton("取消", activity.getMainExecutor(),
                    (dialog, button) -> settle(requestId, null, true))
                .build();
            prompt.authenticate(operation.cryptoObject(), signal, activity.getMainExecutor(),
                new BiometricPrompt.AuthenticationCallback() {
                    @Override public void onAuthenticationSucceeded(
                            BiometricPrompt.AuthenticationResult result) {
                        if (requestId != pendingId) return;
                        worker.execute(() -> {
                            byte[] signed = null;
                            try { signed = operation.finish(result); }
                            catch (GeneralSecurityException | RuntimeException ignored) {}
                            byte[] proof = signed;
                            if (!main.post(() -> {
                                if (requestId == pendingId) settle(requestId, proof, false);
                                else if (proof != null) Arrays.fill(proof, (byte)0);
                            }) && proof != null) Arrays.fill(proof, (byte)0);
                        });
                    }

                    @Override public void onAuthenticationError(int errorCode, CharSequence error) {
                        settle(requestId, null, false);
                    }
                });
        } catch (RuntimeException e) {
            settle(requestId, null, true);
        }
    }

    void cancelApproval(long requestId) {
        main.post(() -> {
            if (pendingId == requestId && requestId != 0) settle(requestId, null, true);
        });
    }

    void close() {
        main.post(() -> {
            closed = true;
            worker.shutdown();
            if (pendingId != 0) settle(pendingId, null, true);
        });
    }

    private void settle(long id, byte[] signature, boolean cancel) {
        if (id != pendingId || id == 0) return;
        CancellationSignal held = signal;
        pendingId = 0;
        if (pendingOperation != null) pendingOperation.close();
        pendingOperation = null;
        signal = null;
        try {
            if (cancel && held != null) held.cancel();
        } finally {
            try { nativeCompleteApproval(id, signature); }
            finally { if (signature != null) Arrays.fill(signature, (byte)0); }
        }
    }

    private static boolean safeLabel(String value) {
        return value != null && value.length() <= 256 &&
            value.codePoints().noneMatch(c -> Character.isISOControl(c) ||
                                               Character.getType(c) == Character.FORMAT);
    }

    private static native void nativeCompleteApproval(long requestId, byte[] rawP1363OrNull);
}
