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
import java.util.Arrays;
import java.util.List;
import java.util.UUID;

public final class MainActivity extends Activity {
    private LinearLayout content;
    private DesktopStore store;
    private DesktopBackend backend;
    private UUID selected;
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
        // 直到正式 Rust owner 安装之前，不创建假 backend 或任何网络/密钥活动。
        backend = null;
        String last = getPreferences(MODE_PRIVATE).getString("selected", null);
        if (last != null) {
            try { selected = UUID.fromString(last); }
            catch (IllegalArgumentException ignored) {}
        }
        active = true;
        render();
    }

    @Override public void onDestroy() {
        active = false;
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
            if (selected != null && rows.stream().noneMatch(row -> row.localId.equals(selected))) selected = null;
            button("全部会话", () -> select(null));
            if (rows.isEmpty()) text("没有已批准的桌面", 16);
            for (DesktopBackend.Desktop row : rows) {
                button(row.name + " · " + row.hostId.substring(0, 8),
                    () -> select(row.localId));
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
        Button scan = button("扫码添加电脑", () -> startActivityForResult(new Intent(this, ScanActivity.class), 401));
        scan.setEnabled(backend != null);
        Button manual = button("改用配对码", this::manualPairing);
        manual.setEnabled(backend != null);
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
        if (state != DesktopBackend.State.OBSERVING) return;
        for (DesktopBackend.Session session : backend.observerSessions(desktop)) {
            // 来源始终与行绑定，不按照会话 ID 跨主机合并。
            text(session.title + " · " + source, 16);
        }
    }

    private void manualPairing() {
        if (backend == null) return;
        LinearLayout form = new LinearLayout(this);
        form.setOrientation(LinearLayout.VERTICAL);
        EditText relay = new EditText(this);
        relay.setSingleLine();
        relay.setSaveEnabled(false);
        relay.setHint("自建中继 WSS 地址");
        try {
            for (DesktopBackend.Desktop desktop : store.read())
                if (desktop.localId.equals(selected)) relay.setText(desktop.relay.toString());
        } catch (IOException ignored) {}
        relay.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        EditText code = new EditText(this);
        code.setSingleLine();
        code.setSaveEnabled(false);
        code.setHint("桌面显示的一次性配对码");
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
            catch (IllegalArgumentException e) { relay.setError("请输入不含凭据的 WSS 地址"); return; }
            char[] material = canonicalCode(code.getText());
            if (material == null) { code.setError("请输入桌面显示的 16 位配对码"); return; }
            dialog.dismiss();
            redeem(locator, material);
        });
    }

    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (!active || backend == null || request != 401 || result != RESULT_OK || data == null)
            return;
        String invite = data.getStringExtra(ScanActivity.RESULT_INVITE);
        data.removeExtra(ScanActivity.RESULT_INVITE);
        if (invite == null || invite.isEmpty() || invite.length() > 2048) return;
        redeem(null, invite.toCharArray());
    }

    private void redeem(URI locator, char[] material) {
        long epoch = ++enrollmentEpoch;
        DesktopBackend.EnrollmentCallback completion = new DesktopBackend.EnrollmentCallback() {
            @Override public void approved(DesktopBackend.Desktop desktop) {
                runOnUiThread(() -> {
                    if (!active || epoch != enrollmentEpoch) return;
                    enrollmentEpoch++;
                    try { store.addApproved(desktop); select(desktop.localId); }
                    catch (IOException e) { text("批准结果未保存，未显示为已配对。", 16); }
                });
            }
            @Override public void state(DesktopBackend.State state) {
                runOnUiThread(() -> { if (active && epoch == enrollmentEpoch) text(label(state), 16); });
            }
        };
        try {
            if (locator == null) backend.redeemScannedInvite(material, identities, completion);
            else backend.redeemCode(locator, material, identities, completion);
        } finally {
            Arrays.fill(material, (char) 0);
        }
    }

    // Relay wire accepts only canonical Crockford symbols; UI permits separators
    // and ASCII lowercase without mapping lookalike symbols to other codes.
    private static char[] canonicalCode(CharSequence source) {
        if (source.length() > 64) return null;
        final String alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        char[] code = new char[16];
        int size = 0;
        for (int i = 0; i < source.length(); i++) {
            char c = source.charAt(i);
            if (c == ' ' || c == '-') continue;
            if (c >= 'a' && c <= 'z') c = (char)(c - 'a' + 'A');
            if (size == code.length || alphabet.indexOf(c) < 0) {
                Arrays.fill(code, (char) 0);
                return null;
            }
            code[size++] = c;
        }
        if (size == code.length) return code;
        Arrays.fill(code, (char) 0);
        return null;
    }

    private String label(DesktopBackend.State state) {
        switch (state) {
            case WAITING_FOR_DESKTOP: return "请在电脑上点同意";
            case OBSERVING: return "正在观察";
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
