package cn.ccdesk.android;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.os.Bundle;
import android.text.InputType;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import java.io.IOException;
import java.net.URI;
import java.util.List;
import java.util.UUID;

public final class MainActivity extends Activity {
    private LinearLayout content;
    private DesktopStore store;
    private NativeDesktopBackend backend;
    private boolean pairingPending;
    private UUID selected;
    private UUID scanOwner;
    private PairingInvite pendingConfirmation;
    private long enrollmentEpoch;
    private boolean active;

    private final DesktopBackend.IdentityProvider identities = new DesktopBackend.IdentityProvider() {
        @Override public HardwareIdentity open(UUID localId, HardwareIdentity.Usage usage) throws Exception {
            return HardwareIdentity.open(localId, usage);
        }
        @Override public HardwareIdentity provision(UUID localId, HardwareIdentity.Usage usage,
                byte[] approvedChallenge) throws Exception {
            return HardwareIdentity.provision(localId, usage, approvedChallenge, true);
        }
    };

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_SECURE);
        store = new DesktopStore(this);
        backend = NativeDesktopBackend.load(this, this::render);
        String last = getPreferences(MODE_PRIVATE).getString("selected", null);
        if (last != null) {
            try { selected = UUID.fromString(last); }
            catch (IllegalArgumentException ignored) {}
        }
        active = true;
        if (backend != null) {
            try { backend.restore(store.read(), identities); }
            catch (IOException ignored) {}
        }
        render();
    }

    @Override public void onDestroy() {
        active = false;
        ScanMailbox.clear(scanOwner);
        if (pendingConfirmation != null) pendingConfirmation.close();
        enrollmentEpoch++;
        if (backend != null) backend.close();
        super.onDestroy();
    }

    private void render() {
        ScrollView scroll = new ScrollView(this);
        content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        int padding = (int) (24 * getResources().getDisplayMetrics().density);
        content.setPadding(padding, padding, padding, padding);
        scroll.setFillViewport(true);
        scroll.addView(content);
        setContentView(scroll);
        text("CC Desk", 26);
        text("远程会话 · 仅观察", 16);
        if (backend == null) text("原生连接组件尚未接入。当前不能配对或读取远程会话。", 16);

        try {
            List<DesktopBackend.Desktop> rows = store.read();
            rows.removeIf(row -> backend == null || !backend.verified(row));
            if (selected != null && rows.stream().noneMatch(row -> row.localId.equals(selected))) selected = null;
            button("全部会话", () -> select(null));
            if (rows.isEmpty()) text("没有已批准的桌面", 16);
            for (DesktopBackend.Desktop row : rows) {
                button(row.name + " · " + row.hostId.substring(0, 8),
                    () -> select(row.localId));
            }
            if (selected == null && backend != null && !rows.isEmpty()) {
                button("读取各电脑的共享会话", () -> {
                    for (DesktopBackend.Desktop row : rows) backend.loadShares(row, identities);
                });
            }
            TextView heading = text(selected == null ? "全部会话" : "桌面会话", 22);
            for (DesktopBackend.Desktop row : rows) {
                if (selected != null && !selected.equals(row.localId)) continue;
                if (selected != null) heading.setText(row.name + " · 仅观察");
                try { renderDesktop(row); } catch (RuntimeException e) {
                    text(row.name + " · 暂时不可用，请检查电脑连接", 16);
                }
            }
        } catch (IOException e) {
            text("本地桌面索引不可用。未建立任何连接。", 16);
            return;
        }
        Button scan = button("扫码添加电脑", this::scanPairing);
        scan.setEnabled(backend != null && !pairingPending);
        Button manual = button("改用配对码", this::manualPairing);
        manual.setEnabled(backend != null && !pairingPending);
        if (pairingPending) {
            text("请在电脑上点同意", 16);
            button("取消配对", () -> {
                enrollmentEpoch++; pairingPending = false; backend.cancelEnrollment(); render();
            });
        }
    }

    private void select(UUID localId) {
        selected = localId;
        if (localId == null) getPreferences(MODE_PRIVATE).edit().remove("selected").apply();
        else getPreferences(MODE_PRIVATE).edit().putString("selected", localId.toString()).apply();
        render();
    }

    private void renderDesktop(DesktopBackend.Desktop desktop) {
        DesktopBackend.State state = backend == null ? DesktopBackend.State.UNAVAILABLE : backend.state(desktop);
        String source = desktop.name + " · " + desktop.hostId.substring(0, 8);
        text(source + " · " + label(state), 18);
        if (backend != null && (state == DesktopBackend.State.DISCONNECTED ||
            state == DesktopBackend.State.UNAVAILABLE)) {
            button("查看可用会话",
                () -> backend.loadShares(desktop, identities));
        }
        if (state == DesktopBackend.State.SHARES_READY) {
            List<DesktopBackend.Offer> offers = backend.shareOffers(desktop);
            button("关闭会话列表", () -> backend.disconnectObserver(desktop));
            if (offers.isEmpty()) text("电脑没有共享会话，请在电脑上开启共享。", 15);
            for (DesktopBackend.Offer offer : offers) {
                button(offer.label + " · " + source, () -> backend.chooseShare(desktop, offer));
            }
        }
        if (state == DesktopBackend.State.OBSERVING || state == DesktopBackend.State.AWAITING_OBSERVER ||
            state == DesktopBackend.State.LOADING_SHARES) {
            button(state == DesktopBackend.State.OBSERVING ? "停止查看" : "取消", () ->
                backend.disconnectObserver(desktop));
        }
        if (state != DesktopBackend.State.OBSERVING) return;
        for (DesktopBackend.Session session : backend.observerSessions(desktop)) {
            // 来源始终与行绑定，不按照会话 ID 跨主机合并。
            text(session.title + " · " + source, 16);
            text("这是获取时的会话内容，不会自动更新。", 14);
            TextView snapshot = text(session.text, 14);
            snapshot.setTypeface(android.graphics.Typeface.MONOSPACE);
        }
    }

    private void manualPairing() {
        if (backend == null) return;
        LinearLayout form = new LinearLayout(this);
        form.setOrientation(LinearLayout.VERTICAL);
        EditText relay = new EditText(this);
        relay.setSingleLine();
        relay.setSaveEnabled(false);
        relay.setHint("中转地址");
        try {
            for (DesktopBackend.Desktop desktop : store.read())
                if (desktop.localId.equals(selected)) relay.setText(desktop.relay.toString());
        } catch (IOException ignored) {}
        relay.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        EditText code = new EditText(this);
        code.setSingleLine();
        code.setSaveEnabled(false);
        code.setHint("粘贴电脑显示的完整配对码");
        code.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
        form.addView(relay);
        form.addView(code);
        AlertDialog dialog = new AlertDialog.Builder(this).setTitle("配对桌面")
            .setView(form).setNegativeButton("取消", null).setPositiveButton("请求桌面批准", null).create();
        dialog.setOnDismissListener(unused -> code.getText().clear());
        dialog.show();
        dialog.getWindow().addFlags(WindowManager.LayoutParams.FLAG_SECURE);
        dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(unused -> {
            final URI locator;
            try { locator = DesktopBackend.Desktop.checkedRelay(relay.getText().toString().trim()); }
            catch (IllegalArgumentException e) { relay.setError("请输入有效的中转地址，可从电脑版复制"); return; }
            final PairingInvite invite;
            try { invite = PairingInvite.fromPasted(locator, code.getText()); }
            catch (IllegalArgumentException e) { code.setError("请粘贴电脑显示的完整配对码"); return; }
            dialog.dismiss();
            confirm(invite);
        });
    }

    private void scanPairing() {
        if (backend == null) return;
        scanOwner = ScanMailbox.start();
        startActivityForResult(new Intent(this, ScanActivity.class)
            .putExtra(ScanActivity.SCAN_OWNER, scanOwner.toString()), 401);
    }

    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != 401) return;
        PairingInvite invite = ScanMailbox.take(scanOwner);
        scanOwner = null;
        if (invite == null) return;
        if (!active || backend == null || result != RESULT_OK) { invite.close(); return; }
        confirm(invite);
    }

    private void confirm(PairingInvite invite) {
        if (pendingConfirmation != null) pendingConfirmation.close();
        pendingConfirmation = invite;
        LinearLayout confirmation = new LinearLayout(this);
        confirmation.setOrientation(LinearLayout.VERTICAL);
        TextView instructions = new TextView(this);
        instructions.setText("中继：" + invite.relay.getHost() + "\n请求后，请在电脑上点同意。");
        confirmation.addView(instructions);
        Button details = new Button(this);
        details.setText("查看连接详情");
        TextView detailText = new TextView(this);
        detailText.setText("电脑指纹：" + invite.hostFingerprintPrefix + "\n中继地址：" + invite.relay);
        detailText.setVisibility(android.view.View.GONE);
        details.setOnClickListener(unused -> {
            boolean visible = detailText.getVisibility() == android.view.View.VISIBLE;
            detailText.setVisibility(visible ? android.view.View.GONE : android.view.View.VISIBLE);
            details.setText(visible ? "查看连接详情" : "收起连接详情");
        });
        confirmation.addView(details);
        confirmation.addView(detailText);
        AlertDialog dialog = new AlertDialog.Builder(this).setTitle("确认添加这台电脑")
            .setView(confirmation)
            .setNegativeButton("取消", null)
            .setPositiveButton("请求电脑批准", (unused, button) -> redeem(invite)).create();
        dialog.setOnDismissListener(unused -> {
            invite.close();
            if (pendingConfirmation == invite) pendingConfirmation = null;
        });
        dialog.setOnCancelListener(unused -> invite.close());
        dialog.show();
        dialog.getWindow().addFlags(WindowManager.LayoutParams.FLAG_SECURE);
    }

    private void redeem(PairingInvite invite) {
        long epoch = ++enrollmentEpoch;
        pairingPending = true;
        render();
        DesktopBackend.EnrollmentCallback completion = new DesktopBackend.EnrollmentCallback() {
            @Override public void approved(DesktopBackend.Desktop desktop) {
                runOnUiThread(() -> {
                    if (!active || epoch != enrollmentEpoch) {
                        backend.discardUnpublished(desktop);
                        return;
                    }
                    enrollmentEpoch++;
                    pairingPending = false;
                    render();
                    if (!invite.matchesHost(desktop.hostId) || !invite.relay.equals(desktop.relay)) {
                        backend.discardUnpublished(desktop);
                        text("电脑身份不一致，请重新扫码。", 16);
                        return;
                    }
                    try { store.addApproved(desktop); select(desktop.localId); }
                    catch (IOException e) {
                        backend.discardUnpublished(desktop);
                        text("批准结果未保存，未显示为已配对。", 16);
                    }
                });
            }
            @Override public void state(DesktopBackend.State state) {
                runOnUiThread(() -> {
                    if (!active || epoch != enrollmentEpoch) return;
                    if (state != DesktopBackend.State.WAITING_FOR_DESKTOP) {
                        pairingPending = false;
                        render();
                        text("没有完成配对，请确认电脑在线并重新扫码。", 16);
                    }
                });
            }
        };
        try { backend.redeemInvite(invite, identities, completion); }
        finally { invite.close(); }
    }

    private String label(DesktopBackend.State state) {
        switch (state) {
            case WAITING_FOR_DESKTOP: return "请在电脑上点同意";
            case LOADING_SHARES: return "正在读取共享会话";
            case SHARES_READY: return "共享会话";
            case AWAITING_OBSERVER: return "正在连接会话，请按手机提示确认";
            case OBSERVING: return "会话快照";
            case DISCONNECTED: return "未连接";
            case REVOKED: return "已撤销";
            default: return "暂不可用，请检查电脑连接";
        }
    }

    private TextView text(String value, int size) {
        TextView view = new TextView(this);
        view.setText(value);
        view.setTextSize(size);
        view.setPadding(0, 8, 0, 8);
        content.addView(view);
        return view;
    }

    private Button button(String title, Runnable action) {
        Button button = new Button(this);
        button.setText(title);
        button.setOnClickListener(unused -> action.run());
        content.addView(button);
        return button;
    }
}
