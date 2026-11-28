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
    private final java.util.function.Consumer<Desktop> onProjectionChanged;
    private final DesktopStore store;
    private final PairingRecoveryJournal recovery;
    private final ScheduledThreadPoolExecutor executor = new ScheduledThreadPoolExecutor(1);
    private final AtomicBoolean closed = new AtomicBoolean();
    private final Map<UUID, Connection> paired = new ConcurrentHashMap<>();
    private Attempt pending;

    private NativeDesktopBackend(Activity activity, DesktopStore store, Runnable onChanged,
            java.util.function.Consumer<Desktop> onProjectionChanged) {
        this.activity = activity;
        this.store = store;
        this.recovery = new PairingRecoveryJournal(activity);
        this.onChanged = onChanged;
        this.onProjectionChanged = onProjectionChanged;
        executor.setExecuteExistingDelayedTasksAfterShutdownPolicy(false);
    }

    static NativeDesktopBackend load(Activity activity, DesktopStore store, Runnable onChanged,
            java.util.function.Consumer<Desktop> onProjectionChanged) {
        try {
            System.loadLibrary("cc_desk_android_native");
            // Calls a real symbol when loading so unsuitable/mismatched libraries cannot activate.
            if (nativeAndroidAbiVersion() != 3 || nativePairingState(0) != 0) return null;
            return new NativeDesktopBackend(activity, store, onChanged, onProjectionChanged);
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
                recovery.stage(attempt.link);
                attempt.handle = nativeBeginPairing(attempt.facade, localId(id),
                    invite.relay.toString(), code, invite.hostCommitment(), endpoint.spki(),
                    relay.spki(), approval.spki());
                if (attempt.handle == 0) { recovery.remove(id); fail(attempt); return; }
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
            if (!attempt.link.accepts(host)) { fail(attempt); return; }
            Desktop desktop = attempt.link.approved(host);
            if (!recovery.promote(attempt.link, desktop, store)) { fail(attempt); return; }
            paired.put(attempt.id, new Connection(desktop, attempt.handle, attempt.facade));
            pending = null;
            attempt.callback.approved(desktop);
        } catch (java.io.IOException | RuntimeException | LinkageError e) { fail(attempt); }
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
            restoreInterrupted(keys);
        });
    }

    private void restoreInterrupted(IdentityProvider keys) {
        try {
            for (PairingRecoveryLink link : recovery.read()) {
                if (closed.get()) return;
                NativeKeystoreBridge facade = null;
                try {
                    HardwareIdentity endpoint = keys.open(link.id, HardwareIdentity.Usage.ENDPOINT);
                    HardwareIdentity relay = keys.open(link.id, HardwareIdentity.Usage.RELAY);
                    HardwareIdentity approval = keys.open(link.id, HardwareIdentity.Usage.APPROVAL);
                    facade = new NativeKeystoreBridge(activity, link.id, endpoint, relay, approval);
                    String verified = nativeReadPairedHostId(facade, localId(link.id), link.relay.toString(),
                        endpoint.spki(), relay.spki(), approval.spki());
                    if (!link.accepts(verified) || closed.get()) continue;
                    Desktop desktop = link.approved(verified);
                    if (!recovery.promote(link, desktop, store)) continue;
                    paired.putIfAbsent(link.id, new Connection(desktop, 0, null));
                    changed();
                } catch (Exception | LinkageError denied) {
                    // 提示记录不是权限；没有本机有效收据就不恢复。
                } finally {
                    if (facade != null) facade.close();
                }
            }
        } catch (java.io.IOException ignored) {
            // 既有设备仍由主索引恢复。
        }
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

    List<Offer> shareOffers(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        return c != null && verified(desktop) && c.state == State.SHARES_READY
            ? c.offers : Collections.emptyList();
    }

    static final class CatalogueRow {
        final String title;
        final String lifecycle;
        CatalogueRow(String title, String lifecycle) {
            this.title = title;
            this.lifecycle = lifecycle;
        }
    }

    List<CatalogueRow> catalogueRows(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        return c != null && verified(desktop) && c.state == State.CATALOGUE_READY
            ? c.catalogue : Collections.emptyList();
    }

    boolean archiveCatalogue(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        return c != null && verified(desktop) && c.state == State.CATALOGUE_READY && c.archiveCatalogue;
    }

    void loadShares(Desktop desktop, IdentityProvider keys) { loadShares(desktop, keys, false); }

    void loadManagement(Desktop desktop, IdentityProvider keys) { loadShares(desktop, keys, false, true); }

    void loadArchivedManagement(Desktop desktop, IdentityProvider keys) { loadShares(desktop, keys, true, true); }

    boolean managementArchived(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        return c != null && verified(desktop) && c.management && c.archiveCatalogue;
    }

    void loadShares(Desktop desktop, IdentityProvider keys, boolean archived) {
        loadShares(desktop, keys, archived, false);
    }

    private void loadShares(Desktop desktop, IdentityProvider keys, boolean archived, boolean management) {
        Connection c = paired.get(desktop.localId);
        if (closed.get() || c == null || !verified(desktop)) return;
        executor.execute(() -> {
            if (closed.get() || c.share != 0 || c.observer != 0) return;
            c.archiveCatalogue = archived;
            c.management = management;
            c.managementOutcome = 0;
            c.state = State.LOADING_SHARES;
            changed();
            try {
                HardwareIdentity endpoint = keys.open(desktop.localId, HardwareIdentity.Usage.ENDPOINT);
                HardwareIdentity relay = keys.open(desktop.localId, HardwareIdentity.Usage.RELAY);
                HardwareIdentity approval = keys.open(desktop.localId, HardwareIdentity.Usage.APPROVAL);
                c.shareFacade = new NativeKeystoreBridge(activity, desktop.localId, endpoint, relay, approval);
                byte[] local = localId(desktop.localId);
                String origin = desktop.relay.toString();
                c.share = management
                    ? nativeLoadManagementList(c.shareFacade, local, origin, endpoint.spki(), relay.spki(), approval.spki(), archived)
                    : archived ? nativeLoadArchivedShares(c.shareFacade, local, origin, endpoint.spki(), relay.spki(), approval.spki())
                    : nativeLoadShares(c.shareFacade, local, origin, endpoint.spki(), relay.spki(), approval.spki());
                if (c.share == 0 || closed.get()) { stopObserver(c, State.UNAVAILABLE); return; }
                long owner = c.share;
                schedule(() -> pollShares(c, owner));
            } catch (Exception | LinkageError denied) { stopObserver(c, State.UNAVAILABLE); }
        });
    }

    void chooseShare(Desktop desktop, Offer offer) {
        Connection c = paired.get(desktop.localId);
        if (closed.get() || c == null || !verified(desktop)) return;
        executor.execute(() -> {
            if (closed.get() || c.state != State.SHARES_READY || c.share != offer.owner ||
                !c.offers.contains(offer)) return;
            try {
                if (!nativeChooseShare(offer.owner, offer.id)) {
                    stopObserver(c, State.UNAVAILABLE);
                    return;
                }
                c.selectedOffer = offer;
                c.offers = Collections.emptyList();
                c.state = State.AWAITING_OBSERVER;
                changed();
            } catch (RuntimeException | LinkageError e) { stopObserver(c, State.UNAVAILABLE); }
        });
    }

    static final class ManagementItem {
        final String title;
        final boolean archived;
        final String lifecycle;
        private final byte[] reference;
        ManagementItem(String title, boolean archived, String lifecycle, byte[] reference) {
            this.title = title; this.archived = archived; this.lifecycle = lifecycle; this.reference = reference;
        }
    }

    List<ManagementItem> managementRows(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        return c != null && verified(desktop) && c.state == State.MANAGEMENT_READY
            ? c.managementRows : Collections.emptyList();
    }

    int managementOutcome(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        return c != null && verified(desktop) ? c.managementOutcome : 0;
    }

    void renameManagement(Desktop desktop, ManagementItem item, String title) {
        if (item.archived) return;
        submitManagedMetadata(desktop, item, "rename", title);
    }

    void changeManagementArchive(Desktop desktop, ManagementItem item) {
        if ("open".equals(item.lifecycle)) return;
        submitManagedMetadata(desktop, item, item.archived ? "restore" : "archive", null);
    }

    void closeManagementRun(Desktop desktop, ManagementItem item) {
        if (item.archived || !"open".equals(item.lifecycle)) return;
        submitManagedMetadata(desktop, item, "close", null);
    }

    void reconnectManagementRun(Desktop desktop, ManagementItem item) {
        if (item.archived || !"open".equals(item.lifecycle)) return;
        submitManagedMetadata(desktop, item, "reconnect", null);
    }

    String managementProjection(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        return c != null && verified(desktop) && c.state == State.MANAGEMENT_VIEWING
            ? c.managementProjection : null;
    }

    void stopManagementViews() {
        if (closed.get()) return;
        executor.execute(() -> {
            for (Connection c : paired.values())
                if (c.state == State.MANAGEMENT_VIEWING || c.requestedProjection)
                    stopObserver(c, State.DISCONNECTED);
        });
    }

    private void submitManagedMetadata(Desktop desktop, ManagementItem item, String kind, String title) {
        Connection c = paired.get(desktop.localId);
        if (closed.get() || c == null || !verified(desktop)) return;
        executor.execute(() -> {
            if (closed.get() || c.state != State.MANAGEMENT_READY || c.share == 0 ||
                !c.managementRows.contains(item)) return;
            try {
                org.json.JSONObject body = new org.json.JSONObject().put("action", kind);
                if (title != null) body.put("title", title);
                byte[] action = body.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8);
                if (!nativeSubmitManagement(c.share, item.reference, action)) return;
                c.requestedProjection = "reconnect".equals(kind);
                c.managementOutcome = c.requestedProjection ? 0 : 3;
                c.managementRows = Collections.emptyList();
                c.state = State.MANAGEMENT_WAITING;
                changed();
            } catch (Exception | LinkageError denied) { stopObserver(c, State.UNAVAILABLE); }
        });
    }

    private static List<ManagementItem> parseManagementRows(String input) throws org.json.JSONException {
        if (input == null || input.length() > 524288) throw new org.json.JSONException("Invalid management rows");
        org.json.JSONArray items = new org.json.JSONArray(input);
        if (items.length() > 512) throw new org.json.JSONException("Too many management rows");
        List<ManagementItem> rows = new java.util.ArrayList<>();
        for (int i = 0; i < items.length(); ++i) {
            org.json.JSONObject item = items.getJSONObject(i);
            String title = item.getString("title");
            String lifecycle = item.getString("lifecycle");
            boolean archived = item.getBoolean("archived");
            org.json.JSONArray bytes = item.getJSONArray("reference");
            if (item.length() != 4 || !SessionDisplayTitle.valid(title)
                || !("open".equals(lifecycle) || "closed".equals(lifecycle)) ||
                archived && "open".equals(lifecycle) || bytes.length() != 32)
                throw new org.json.JSONException("Invalid management row");
            byte[] reference = new byte[32];
            for (int j = 0; j < 32; ++j) {
                int value = bytes.getInt(j);
                if (value < 0 || value > 255) throw new org.json.JSONException("Invalid reference");
                reference[j] = (byte) value;
            }
            rows.add(new ManagementItem(title, archived, lifecycle, reference));
        }
        return Collections.unmodifiableList(rows);
    }

    private void pollShares(Connection c, long owner) {
        if (closed.get() || owner == 0 || c.share != owner || c.observer != 0) return;
        try {
            switch (nativeShareState(owner)) {
                case 1:
                    break;
                case 2:
                    if (c.state != State.SHARES_READY) {
                        c.offers = parseOffers(owner, nativeShareOffers(owner));
                        c.state = State.SHARES_READY;
                        changed();
                    }
                    break;
                case 8:
                    if (!c.management) { stopObserver(c, State.UNAVAILABLE); return; }
                    if (c.state != State.MANAGEMENT_READY) {
                        List<ManagementItem> rows = parseManagementRows(nativeManagementRows(owner));
                        if (rows.stream().anyMatch(row -> row.archived != c.archiveCatalogue)) {
                            stopObserver(c, State.UNAVAILABLE); return;
                        }
                        c.managementRows = rows;
                        c.state = State.MANAGEMENT_READY;
                        changed();
                    }
                    break;
                case 9:
                    if (!c.management || !c.requestedProjection) {
                        stopObserver(c, State.UNAVAILABLE); return;
                    }
                    String view = nativeManagementProjection(owner);
                    if (view != null && !view.equals(c.managementProjection)) {
                        boolean first = c.state != State.MANAGEMENT_VIEWING;
                        c.managementProjection = view;
                        c.state = State.MANAGEMENT_VIEWING;
                        if (first) changed();
                        else activity.runOnUiThread(() -> {
                            if (!closed.get() && !activity.isDestroyed())
                                onProjectionChanged.accept(c.desktop);
                        });
                    }
                    break;
                case 7:
                    if (!c.management) { stopObserver(c, State.UNAVAILABLE); return; }
                    int outcome = nativeManagementOutcome(owner);
                    c.managementOutcome = c.requestedProjection ? 0 : (outcome >= 1 && outcome <= 3 ? outcome : 3);
                    stopObserver(c, State.DISCONNECTED);
                    return;
                case 4:
                    if (c.management) break;
                    if (c.state != State.AWAITING_OBSERVER) {
                        c.state = State.AWAITING_OBSERVER;
                        c.offers = Collections.emptyList();
                        changed();
                    }
                    break;
                case 6:
                    c.catalogue = parseCatalogue(nativeCatalogueRows(owner));
                    c.share = 0;
                    nativeCancelShares(owner);
                    if (c.shareFacade != null) c.shareFacade.close();
                    c.shareFacade = null;
                    c.state = State.CATALOGUE_READY;
                    changed();
                    return;
                case 5:
                    c.observer = nativeShareObserverHandle(owner);
                    if (c.observer == 0) { stopObserver(c, State.UNAVAILABLE); return; }
                    c.offers = Collections.emptyList();
                    long observer = c.observer;
                    schedule(() -> pollObserver(c, observer));
                    return;
                default:
                    stopObserver(c, State.UNAVAILABLE);
                    return;
            }
            schedule(() -> pollShares(c, owner));
        } catch (Exception | LinkageError e) { stopObserver(c, State.UNAVAILABLE); }
    }

    static List<CatalogueRow> parseCatalogue(String input) throws org.json.JSONException {
        if (input == null || input.length() > 1048576)
            throw new org.json.JSONException("Invalid catalogue");
        org.json.JSONArray items = new org.json.JSONArray(input);
        if (items.length() > 8192) throw new org.json.JSONException("Too many catalogue rows");
        List<CatalogueRow> rows = new java.util.ArrayList<>();
        for (int i = 0; i < items.length(); i++) {
            org.json.JSONObject item = items.getJSONObject(i);
            Object title = item.get("title");
            Object state = item.get("lifecycle");
            if (item.length() != 2 || !(title instanceof String) ||
                !("open".equals(state) || "closed".equals(state) || "history".equals(state)) ||
                !SessionDisplayTitle.valid((String) title))
                throw new org.json.JSONException("Invalid catalogue row");
            rows.add(new CatalogueRow((String)title, (String)state));
        }
        return Collections.unmodifiableList(rows);
    }

    private static List<Offer> parseOffers(long owner, String input) throws org.json.JSONException {
        if (input == null || input.length() > 2048) throw new org.json.JSONException("Invalid share list");
        org.json.JSONArray items = new org.json.JSONArray(input);
        if (items.length() > 4) throw new org.json.JSONException("Too many shares");
        List<Offer> offers = new java.util.ArrayList<>();
        java.util.Set<String> ids = new java.util.HashSet<>();
        for (int i = 0; i < items.length(); i++) {
            org.json.JSONObject item = items.getJSONObject(i);
            String id = item.getString("id");
            String label = item.getString("label");
            String rawRole = item.getString("role");
            Role role = "observer".equals(rawRole) ? Role.OBSERVER :
                ("controller".equals(rawRole) ? Role.CONTROLLER : null);
            if (role == null || item.length() != 3 || !id.matches("[0-9a-f]{32}") || !ids.add(id) ||
                label.trim().isEmpty() || label.length() > 48 ||
                label.codePoints().anyMatch(ch -> Character.isISOControl(ch) ||
                    Character.getType(ch) == Character.FORMAT))
                throw new org.json.JSONException("Invalid share row");
            offers.add(new Offer(owner, id, label, role));
        }
        return Collections.unmodifiableList(offers);
    }

    private void pollObserver(Connection c, long observer) {
        if (closed.get() || observer == 0 || c.observer != observer) return;
        try {
            int state = nativeObserverState(observer);
            if (state == 1) { schedule(() -> pollObserver(c, observer)); return; }
            if (state != 2) { stopObserver(c); return; }
            Role expected = c.selectedOffer == null ? null : c.selectedOffer.role;
            int role = nativeViewerRole(observer);
            if (expected == null || (expected == Role.OBSERVER ? role != 1 : role != 2)) {
                stopObserver(c, State.UNAVAILABLE);
                return;
            }
            boolean controller = expected == Role.CONTROLLER;
            String text = controller ? nativeTakeControllerFrame(observer) : nativeTakeObserverText(observer);
            if (text != null) {
                if (controller) {
                    if (text.length() > 240000) throw new IllegalArgumentException("Projection too large");
                    org.json.JSONObject frame = new org.json.JSONObject(text);
                    long frameId = frame.getLong("id");
                    String literal = frame.getString("text");
                    if (frame.length() != 2 || frameId <= 0 || literal.length() > 100000)
                        throw new IllegalArgumentException("Invalid native frame");
                    if (frameId <= c.lastConfirmedFrame) throw new IllegalArgumentException("Stale native frame");
                    c.panelReady = false;
                    c.snapshot = new Session("current-session", c.selectedOffer.label, literal, frameId);
                } else c.snapshot = new Session("current-session", c.selectedOffer.label, text, 0);
            }
            State connected = controller ? State.CONTROLLING : State.OBSERVING;
            if (c.state != connected || text != null) {
                c.state = connected;
                changed();
            }
            schedule(() -> pollObserver(c, observer));
        } catch (Exception | LinkageError e) { stopObserver(c); }
    }

    long controlHandle(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        return verified(desktop) && c.state == State.CONTROLLING && c.observer != c.stopRequestedHandle
            ? c.observer : 0;
    }

    boolean controllerReady(Desktop desktop, long handle) {
        Connection c = paired.get(desktop.localId);
        return c != null && c.observer == handle && c.panelReady && c.state == State.CONTROLLING;
    }

    void confirmDisplayedFrame(Desktop desktop, long handle, long frameId) {
        Connection c = paired.get(desktop.localId);
        if (c == null || closed.get() || !verified(desktop)) return;
        executor.execute(() -> {
            try {
                if (!validControl(c, handle) || frameId <= c.lastConfirmedFrame) return;
                if (c.snapshot == null || c.snapshot.frameId != frameId) return;
                if (!nativeConfirmControllerFrame(handle, frameId)) {
                    stopObserver(c, State.UNAVAILABLE);
                    return;
                }
                c.lastConfirmedFrame = frameId;
                if (!c.panelReady) { c.panelReady = true; changed(); }
            } catch (RuntimeException | LinkageError e) { stopObserver(c, State.UNAVAILABLE); }
        });
    }

    void sendControl(Desktop desktop, long handle, String text, TerminalControlPanel.Key key,
            TerminalControlPanel.Completion completion) {
        Connection c = paired.get(desktop.localId);
        if (closed.get() || c == null || handle == 0 || (text == null) == (key == null)) {
            finishControl(completion, TerminalControlPanel.Result.REJECTED);
            return;
        }
        executor.execute(() -> {
            try {
                if (!validControl(c, handle) || !c.panelReady || c.inputPending || c.inputUncertain) {
                    finishControl(completion, TerminalControlPanel.Result.REJECTED);
                    return;
                }
                c.inputPending = true;
                long id = text != null ? nativeQueueControllerText(handle, text) :
                    nativeQueueControllerKey(handle, keyCode(key));
                if (id == 0) {
                    c.inputPending = false;
                    finishControl(completion, TerminalControlPanel.Result.REJECTED);
                } else {
                    long deadline = android.os.SystemClock.elapsedRealtime() + 10000;
                    schedule(() -> pollControl(c, handle, id, deadline, completion));
                }
            } catch (RuntimeException | LinkageError e) {
                c.inputPending = false;
                c.inputUncertain = true;
                finishControl(completion, TerminalControlPanel.Result.UNKNOWN);
            }
        });
    }

    private void pollControl(Connection c, long handle, long input, long deadline,
            TerminalControlPanel.Completion completion) {
        if (closed.get() || c.observer != handle) {
            finishControl(completion, TerminalControlPanel.Result.UNKNOWN);
            return;
        }
        int outcome = 0;
        try { outcome = nativeControllerInputOutcome(handle, input); }
        catch (RuntimeException | LinkageError ignored) {}
        if (outcome == 1 && android.os.SystemClock.elapsedRealtime() < deadline) {
            schedule(() -> pollControl(c, handle, input, deadline, completion));
            return;
        }
        c.inputPending = false;
        TerminalControlPanel.Result result;
        if (outcome == 2) result = TerminalControlPanel.Result.COMMITTED;
        else if (outcome == 3) result = TerminalControlPanel.Result.REJECTED;
        else {
            c.inputUncertain = true;
            result = TerminalControlPanel.Result.UNKNOWN;
        }
        finishControl(completion, result);
    }

    private boolean validControl(Connection c, long handle) {
        return !closed.get() && handle != c.stopRequestedHandle && c.observer == handle && c.state == State.CONTROLLING &&
            nativeViewerRole(handle) == 2;
    }

    private void finishControl(TerminalControlPanel.Completion completion, TerminalControlPanel.Result result) {
        activity.runOnUiThread(() -> { if (!closed.get() && !activity.isDestroyed()) completion.finish(result); });
    }

    private static int keyCode(TerminalControlPanel.Key key) {
        switch (key) {
            case ENTER: return 1;
            case BACKSPACE: return 2;
            case TAB: return 3;
            case ESCAPE: return 4;
            case UP: return 5;
            case DOWN: return 6;
            case LEFT: return 7;
            case RIGHT: return 8;
            case CTRL_C: return 9;
            default: throw new IllegalArgumentException("Unsupported key");
        }
    }

    void disconnectObserver(Desktop desktop) {
        Connection c = paired.get(desktop.localId);
        if (!closed.get() && verified(desktop)) executor.execute(() -> stopObserver(c));
    }

    void disconnectController(Desktop desktop, long exact) {
        Connection c = paired.get(desktop.localId);
        if (closed.get() || c == null || exact == 0 || !verified(desktop) || c.observer != exact) return;
        c.stopRequestedHandle = exact;
        executor.execute(() -> { if (c.observer == exact) stopObserver(c); });
    }

    private void stopObserver(Connection c) { stopObserver(c, State.DISCONNECTED); }

    private void stopObserver(Connection c, State terminal) {
        long observer = c.observer;
        long share = c.share;
        NativeKeystoreBridge facade = c.shareFacade;
        c.observer = 0;
        c.share = 0;
        c.shareFacade = null;
        c.snapshot = null;
        c.selectedOffer = null;
        c.inputPending = false;
        c.inputUncertain = false;
        c.panelReady = false;
        c.lastConfirmedFrame = 0;
        c.offers = Collections.emptyList();
        c.catalogue = Collections.emptyList();
        c.managementRows = Collections.emptyList();
        c.managementProjection = null;
        c.requestedProjection = false;
        c.management = false;
        c.state = terminal;
        try {
            if (observer != 0) nativeCancelObserver(observer);
        } finally {
            try { if (share != 0) nativeCancelShares(share); }
            finally {
                if (facade != null) facade.close();
                changed();
            }
        }
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

    void cancelEnrollment(java.util.function.Consumer<Boolean> completion) {
        if (!closed.get()) executor.execute(() -> {
            Attempt attempt = pending;
            // 若当前没有配对请求，终态回调已经进入队列。
            if (attempt == null) return;
            boolean removed = false;
            try { removed = recovery.remove(attempt.id); }
            catch (RuntimeException ignored) { /* 无法确认记录已移除时，不报告取消成功。 */ }
            try { cancelPending(); }
            finally { completion.accept(removed); }
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
        final PairingRecoveryLink link;
        final UUID id;
        final EnrollmentCallback callback;
        long handle;
        NativeKeystoreBridge facade;
        Attempt(PairingInvite invite, UUID id, EnrollmentCallback callback) {
            this.id = id;
            this.link = new PairingRecoveryLink(id, invite.relay,
                invite.hostEndpointId != null ? invite.hostEndpointId : invite.hostFingerprintPrefix);
            this.callback = callback;
        }
    }

    private static final class Connection {
        final Desktop desktop;
        final long pairing;
        final NativeKeystoreBridge facade;
        long share;
        NativeKeystoreBridge shareFacade;
        volatile List<Offer> offers = Collections.emptyList();
        volatile List<CatalogueRow> catalogue = Collections.emptyList();
        volatile List<ManagementItem> managementRows = Collections.emptyList();
        volatile boolean management;
        volatile int managementOutcome;
        volatile boolean requestedProjection;
        volatile String managementProjection;
        volatile boolean archiveCatalogue;
        volatile long observer;
        volatile long stopRequestedHandle;
        Offer selectedOffer;
        volatile boolean panelReady;
        long lastConfirmedFrame;
        boolean inputPending;
        boolean inputUncertain;
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
    static native String nativeReadPairedHostId(NativeKeystoreBridge facade, byte[] localId,
        String canonicalWss, byte[] endpointSpki, byte[] relaySpki, byte[] approvalSpki);
    static native long nativeLoadShares(NativeKeystoreBridge facade, byte[] localId,
        String canonicalWss, byte[] endpointSpki, byte[] relaySpki, byte[] approvalSpki);
    static native long nativeLoadArchivedShares(NativeKeystoreBridge facade, byte[] localId,
        String canonicalWss, byte[] endpointSpki, byte[] relaySpki, byte[] approvalSpki);
    static native long nativeLoadManagementList(NativeKeystoreBridge facade, byte[] localId,
        String canonicalWss, byte[] endpointSpki, byte[] relaySpki, byte[] approvalSpki, boolean archived);
    static native String nativeManagementRows(long handle);
    static native boolean nativeSubmitManagement(long handle, byte[] opaqueReference, byte[] typedAction);
    static native int nativeManagementOutcome(long handle);
    static native String nativeManagementProjection(long handle);
    static native int nativeShareState(long handle);
    static native String nativeShareOffers(long handle);
    static native String nativeCatalogueRows(long handle);
    static native boolean nativeChooseShare(long handle, String offer);
    static native long nativeShareObserverHandle(long handle);
    static native void nativeCancelShares(long handle);
    static native int nativeViewerRole(long handle);
    static native String nativeTakeControllerFrame(long handle);
    static native boolean nativeConfirmControllerFrame(long handle, long frameId);
    static native long nativeQueueControllerText(long handle, String text);
    static native long nativeQueueControllerKey(long handle, int key);
    static native int nativeControllerInputOutcome(long handle, long input);
    static native int nativeObserverState(long handle);
    static native String nativeTakeObserverText(long handle);
    static native void nativeCancelObserver(long handle);
}
