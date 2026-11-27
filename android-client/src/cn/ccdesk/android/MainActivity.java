package cn.ccdesk.android;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.os.Bundle;
import android.text.InputType;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.EditText;
import android.widget.ScrollView;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.io.IOException;
import java.net.URI;
import java.util.List;
import java.util.UUID;

public final class MainActivity extends Activity {
    private static final String HOME_SESSIONS = "home.sessions";
    private LinearLayout content;
    private RemoteHomeView home;
    private SessionBrowser sessionBrowser;
    private boolean showingSessions;
    private boolean projectionForeground;
    private final java.util.Map<UUID, TextView> managementViewports = new java.util.HashMap<>();
    private boolean lastHomeSessions;
    private UUID lastHomeSelection;
    private DesktopStore store;
    private NativeDesktopBackend backend;
    private boolean pairingPending;
    private boolean cancellingEnrollment;
    private enum PairingNotice { NONE, FAILED, SAVED, CANCELLED, CANCEL_UNCONFIRMED }
    private PairingNotice pairingNotice = PairingNotice.NONE;
    private TerminalControlPanel controlPanel;
    private DesktopBackend.Desktop controlDesktop;
    private long controlHandle;
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
        showingSessions = state != null && state.getBoolean(HOME_SESSIONS, false);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_SECURE);
        getWindow().setDecorFitsSystemWindows(false);
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        store = new DesktopStore(this);
        backend = NativeDesktopBackend.load(this, store, this::updateConnectionViews,
            this::updateManagementProjection);
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

    @Override protected void onSaveInstanceState(Bundle state) {
        super.onSaveInstanceState(state);
        state.putBoolean(HOME_SESSIONS, showingSessions);
    }

    @Override public void onDestroy() {
        active = false;
        if (controlPanel != null) controlPanel.closeLocally();
        ScanMailbox.clear(scanOwner);
        if (pendingConfirmation != null) pendingConfirmation.close();
        enrollmentEpoch++;
        if (backend != null) backend.close();
        super.onDestroy();
    }

    private void render() {
        if (showingSessions && lastHomeSessions && sessionBrowser != null && home.root.isAttachedToWindow()
            && java.util.Objects.equals(lastHomeSelection, selected)) {
            sessionBrowser.refresh();
            return;
        }
        sessionBrowser = null;
        managementViewports.clear();
        RemoteHomeView previous = home;
        int scrollY = previous != null && lastHomeSessions == showingSessions
            && java.util.Objects.equals(lastHomeSelection, selected) ? previous.scroll.getScrollY() : 0;
        home = new RemoteHomeView(this);
        content = home.body;
        home.build(showingSessions, () -> navigateHome(false), () -> navigateHome(true));
        setContentView(home.root);
        home.root.requestApplyInsets();
        lastHomeSessions = showingSessions;
        lastHomeSelection = selected;
        try {
            List<DesktopBackend.Desktop> rows = store.read();
            if (selected != null && rows.stream().noneMatch(row -> row.localId.equals(selected))) selected = null;
            if (showingSessions) renderSessions(rows);
            else renderDevices(rows);
        } catch (IOException e) {
            LinearLayout card = home.card(content);
            home.title(card, "无法读取设备", 19, true);
            home.body(card, "本机保存的设备暂不可用，没有发起连接。", 15);
            home.action(card, "重试", true, this::render);
        }
        if (scrollY > 0) {
            RemoteHomeView shown = home;
            shown.scroll.post(() -> { if (home == shown) shown.scroll.scrollTo(0, scrollY); });
        }
    }

    private void navigateHome(boolean sessions) {
        if (showingSessions == sessions) return;
        showingSessions = sessions;
        render();
    }

    private void renderDevices(List<DesktopBackend.Desktop> rows) {
        home.title(content, "设备", 28, true);
        home.body(content, "管理已配对的电脑。断开连接不会移除设备。", 15);
        if (backend == null) home.body(content, "本版本没有可用的连接组件，仍可查看本机记录。", 15);
        if (pairingPending) {
            LinearLayout card = home.card(content);
            home.title(card, "等待电脑确认", 20, true);
            home.status(card, "已发起配对", true);
            home.body(card, "在电脑的远程连接页同意这次请求。本机验证并保存凭据后才算配对完成。", 15);
            home.action(card, cancellingEnrollment ? "正在取消…" : "取消配对", false,
                this::cancelEnrollment).setEnabled(!cancellingEnrollment);
        } else {
            LinearLayout card = home.card(content);
            if (pairingNotice == PairingNotice.FAILED) {
                home.title(card, "配对未完成", 20, true);
                home.body(card, "请检查电脑和中转是否可用。若邀请已过期或被拒绝，请在电脑上创建新的邀请后重试。", 15);
            } else if (pairingNotice == PairingNotice.SAVED) {
                home.title(card, "已配对并保存", 20, true);
                home.body(card, "可以在设备卡片中查看会话。是否有内容取决于电脑的共享授权。", 15);
            } else if (pairingNotice == PairingNotice.CANCELLED) {
                home.title(card, "已取消配对", 20, true);
            } else if (pairingNotice == PairingNotice.CANCEL_UNCONFIRMED) {
                home.title(card, "无法确认配对已取消", 20, true);
                home.body(card, "请检查设备列表。如果该电脑已配对，可在电脑端撤销手机授权。", 15);
            } else if (rows.isEmpty()) {
                home.title(card, "配对第一台电脑", 20, true);
                home.body(card, "在电脑的远程连接页创建邀请，用手机扫描二维码或输入配对码。", 15);
            }
            home.action(card, "添加电脑", true, this::showPairingOptions).setEnabled(backend != null);
        }
        for (DesktopBackend.Desktop row : rows) {
            LinearLayout card = home.card(content);
            String name = displayName(rows, row);
            home.title(card, name, 20, true);
            if (canUseDesktop(row)) {
                DesktopBackend.State state = backend.state(row);
                home.status(card, deviceStatus(state), isEngaged(state));
            } else {
                home.status(card, "已保存 · 配对凭据尚未验证", false);
                home.body(card, "凭据验证前无法查看远程会话。", 15);
            }
            if (canUseDesktop(row) && backend.state(row) != DesktopBackend.State.REVOKED) {
                home.action(card, "查看会话", true, () -> {
                    select(row.localId);
                    refreshDesktop(row);
                }).setContentDescription("查看 " + name + " 的会话");
            }
            home.action(card, "设备设置", false, () -> editDesktopAlias(row, name))
                .setContentDescription("打开 " + name + " 的设备设置");
        }
    }

    private void showPairingOptions() {
        if (backend == null || pairingPending || scanOwner != null) return;
        new AlertDialog.Builder(this)
            .setTitle("添加电脑")
            .setMessage("在电脑的远程连接页创建邀请。扫描二维码会自动填写连接信息；也可以手动输入配对码。")
            .setPositiveButton("扫描二维码", (dialog, which) -> scanPairing())
            .setNeutralButton("输入配对码", (dialog, which) -> manualPairing())
            .setNegativeButton("取消", null).show();
    }

    private void renderSessions(List<DesktopBackend.Desktop> rows) {
        sessionBrowser = new SessionBrowser(rows);
    }

    private final class SessionBrowser {
        private List<DesktopBackend.Desktop> rows;
        private final Button scope;
        private final Button refreshButton;
        private final EditText search;
        private final Button clear;
        private final LinearLayout searchRow;
        private final TextView searchCoverage;
        private final LinearLayout results;

        SessionBrowser(List<DesktopBackend.Desktop> rows) {
            this.rows = rows;
            home.title(content, "会话", 28, true);
            scope = home.action(content, "全部电脑", false, () -> chooseSessionDevice(this.rows));
            refreshButton = home.action(content, "刷新会话", true, this::refreshSelected);
            searchRow = new LinearLayout(MainActivity.this);
            searchRow.setGravity(android.view.Gravity.CENTER_VERTICAL);
            search = new EditText(MainActivity.this);
            search.setHint("搜索电脑或会话标题");
            search.setSingleLine(true);
            search.setSaveEnabled(false);
            search.setImportantForAutofill(android.view.View.IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS);
            search.setImeOptions(android.view.inputmethod.EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING);
            search.setFilters(new android.text.InputFilter[] { new android.text.InputFilter.LengthFilter(80) });
            searchRow.addView(search, new LinearLayout.LayoutParams(0, -2, 1));
            clear = new Button(MainActivity.this);
            clear.setText("清除");
            clear.setContentDescription("清除会话搜索");
            clear.setMinHeight(Math.round(48 * getResources().getDisplayMetrics().density));
            clear.setOnClickListener(unused -> search.setText(""));
            searchRow.addView(clear);
            content.addView(searchRow);
            searchCoverage = home.body(content, "", 14);
            results = new LinearLayout(MainActivity.this);
            results.setOrientation(LinearLayout.VERTICAL);
            content.addView(results);
            search.addTextChangedListener(new android.text.TextWatcher() {
                @Override public void beforeTextChanged(CharSequence value, int start, int count, int after) {}
                @Override public void onTextChanged(CharSequence value, int start, int before, int count) {
                    if (sessionBrowser == SessionBrowser.this) renderResults(false);
                }
                @Override public void afterTextChanged(android.text.Editable value) {}
            });
            renderResults(false);
        }

        private boolean canRefreshRow(DesktopBackend.Desktop row) {
            return (selected == null || selected.equals(row.localId)) && canUseDesktop(row) && canRefresh(backend.state(row));
        }

        private void refreshSelected() {
            for (DesktopBackend.Desktop row : rows) {
                if (canRefreshRow(row)) refreshDesktop(row);
            }
        }

        void refresh() {
            try {
                rows = store.read();
                if (selected != null && rows.stream().noneMatch(row -> row.localId.equals(selected))) selected = null;
                lastHomeSelection = selected;
                renderResults(true);
            } catch (IOException denied) {
                rows = java.util.Collections.emptyList();
                setToolbarVisible(false);
                results.removeAllViews();
                LinearLayout card = home.card(results);
                home.title(card, "无法读取设备", 19, true);
                home.body(card, "未发起远程连接。", 15);
                home.action(card, "重试", true, this::refresh);
            }
        }

        private void setToolbarVisible(boolean visible) {
            scope.setVisibility(visible ? android.view.View.VISIBLE : android.view.View.GONE);
            refreshButton.setVisibility(visible && rows.stream().anyMatch(this::canRefreshRow)
                ? android.view.View.VISIBLE : android.view.View.GONE);
            searchRow.setVisibility(visible ? android.view.View.VISIBLE : android.view.View.GONE);
            if (!visible) searchCoverage.setVisibility(android.view.View.GONE);
        }

        private void renderSearchCoverage(String query, SessionSearchCoverage coverage) {
            if (query.isEmpty()) {
                searchCoverage.setVisibility(android.view.View.GONE);
                return;
            }
            StringBuilder text = new StringBuilder("只搜索已读取的标题和本机设备名称，不包含全部历史。");
            if (coverage.isPartial()) {
                appendCoverage(text, coverage.count(SessionSearchCoverage.Device.LOADING), " 台设备正在读取或连接");
                appendCoverage(text, coverage.count(SessionSearchCoverage.Device.NOT_CONNECTED), " 台设备未连接或列表不可用");
                appendCoverage(text, coverage.count(SessionSearchCoverage.Device.UNVERIFIED), " 台设备的配对凭据未验证");
                appendCoverage(text, coverage.count(SessionSearchCoverage.Device.ACTIVE_ONLY), " 台设备仅显示当前会话");
            }
            searchCoverage.setText(text);
            searchCoverage.setVisibility(android.view.View.VISIBLE);
        }

        private void appendCoverage(StringBuilder text, int count, String suffix) {
            if (count > 0) text.append('\n').append(count).append(suffix);
        }

        private void renderResults(boolean preserveScroll) {
            RemoteHomeView page = home;
            int savedScroll = page.scroll.getScrollY();
            setToolbarVisible(!rows.isEmpty());
            results.removeAllViews();
            if (rows.isEmpty()) {
                LinearLayout card = home.card(results);
                home.title(card, "还没有可查看的会话", 20, true);
                home.body(card, "先配对电脑，再按电脑上的共享授权查看会话。", 15);
                home.action(card, "去配对电脑", true, () -> navigateHome(false));
                return;
            }
            DesktopBackend.Desktop chosen = rows.stream().filter(row -> row.localId.equals(selected))
                .findFirst().orElse(null);
            scope.setText(chosen == null ? "全部电脑 ▾" : displayName(rows, chosen) + " ▾");
            scope.setContentDescription(chosen == null ? "筛选电脑：全部" : "筛选电脑：" + displayName(rows, chosen));
            boolean hasText = search.length() > 0;
            clear.setVisibility(hasText ? android.view.View.VISIBLE : android.view.View.GONE);
            String query = SessionTitleFilter.normalize(search.getText());
            SessionSearchCoverage coverage = new SessionSearchCoverage();
            boolean matched = false;
            for (DesktopBackend.Desktop row : rows) {
                if (selected != null && !selected.equals(row.localId)) continue;
                String name = displayName(rows, row);
                try { matched |= renderDesktop(results, row, name, query, coverage); }
                catch (RuntimeException denied) {
                    coverage.add(SessionSearchCoverage.Device.NOT_CONNECTED);
                    if (!SessionTitleFilter.matches(query, name, null)) continue;
                    matched = true;
                    LinearLayout card = home.card(results);
                    home.title(card, name, 19, true);
                    home.body(card, "暂时无法读取，请返回设备页查看状态。", 15);
                }
            }
            renderSearchCoverage(query, coverage);
            if (!matched && !query.isEmpty()) {
                LinearLayout card = home.card(results);
                home.title(card, coverage.isPartial() ? "已读取的标题中没有匹配" : "没有匹配的会话", 19, true);
                home.body(card, "只搜索此范围内的电脑名称和当前可见的会话标题。", 15);
                home.action(card, "清除搜索", false, () -> search.setText(""));
            }
            if (preserveScroll && savedScroll > 0) {
                page.scroll.post(() -> {
                    if (home == page && sessionBrowser == this && query.equals(SessionTitleFilter.normalize(search.getText())))
                        page.scroll.scrollTo(0, savedScroll);
                });
            }
        }
    }

    private void chooseSessionDevice(List<DesktopBackend.Desktop> rows) {
        String[] labels = new String[rows.size() + 1];
        labels[0] = "全部电脑";
        int checked = 0;
        for (int i = 0; i < rows.size(); i++) {
            labels[i + 1] = displayName(rows, rows.get(i));
            if (rows.get(i).localId.equals(selected)) checked = i + 1;
        }
        new AlertDialog.Builder(this).setTitle("会话范围")
            .setSingleChoiceItems(labels, checked, (dialog, which) -> {
                dialog.dismiss();
                select(which == 0 ? null : rows.get(which - 1).localId);
            }).setNegativeButton("取消", null).show();
    }

    private void select(UUID localId) {
        selected = localId;
        showingSessions = true;
        if (localId == null) getPreferences(MODE_PRIVATE).edit().remove("selected").apply();
        else getPreferences(MODE_PRIVATE).edit().putString("selected", localId.toString()).apply();
        render();
    }

    private String displayName(List<DesktopBackend.Desktop> rows, DesktopBackend.Desktop desktop) {
        // 兼容已经保存的旧版默认名，但不在首页露出身份摘要。
        return desktop.name.equals("电脑 " + desktop.hostId.substring(0, 8))
            ? "电脑 " + (rows.indexOf(desktop) + 1) : desktop.name;
    }

    private boolean canUseDesktop(DesktopBackend.Desktop desktop) {
        return backend != null && backend.verified(desktop);
    }

    private boolean canRefresh(DesktopBackend.State state) {
        return state == DesktopBackend.State.DISCONNECTED || state == DesktopBackend.State.UNAVAILABLE
            || state == DesktopBackend.State.CATALOGUE_READY;
    }

    private void refreshDesktop(DesktopBackend.Desktop desktop) {
        if (canUseDesktop(desktop) && canRefresh(backend.state(desktop))) backend.loadShares(desktop, identities);
    }

    private void loadArchived(DesktopBackend.Desktop desktop) {
        if (canUseDesktop(desktop) && canRefresh(backend.state(desktop)))
            backend.loadShares(desktop, identities, true);
    }

    private boolean isEngaged(DesktopBackend.State state) {
        return state == DesktopBackend.State.OBSERVING || state == DesktopBackend.State.CONTROLLING
            || state == DesktopBackend.State.MANAGEMENT_VIEWING;
    }

    private String deviceStatus(DesktopBackend.State state) {
        return state == DesktopBackend.State.DISCONNECTED ? "已配对 · 未连接" : label(state);
    }

    private SessionSearchCoverage.Device searchCoverageFor(DesktopBackend.State state) {
        switch (state) {
            case SHARES_READY:
            case CATALOGUE_READY:
            case MANAGEMENT_READY:
                return SessionSearchCoverage.Device.LOADED;
            case OBSERVING:
            case CONTROLLING:
            case MANAGEMENT_VIEWING:
                return SessionSearchCoverage.Device.ACTIVE_ONLY;
            case WAITING_FOR_DESKTOP:
            case LOADING_SHARES:
            case AWAITING_OBSERVER:
            case MANAGEMENT_WAITING:
                return SessionSearchCoverage.Device.LOADING;
            default:
                return SessionSearchCoverage.Device.NOT_CONNECTED;
        }
    }

    private boolean renderDesktop(LinearLayout target, DesktopBackend.Desktop desktop, String name,
            String query, SessionSearchCoverage coverage) {
        boolean nameMatches = SessionTitleFilter.matches(query, name, null);
        if (!canUseDesktop(desktop)) {
            coverage.add(SessionSearchCoverage.Device.UNVERIFIED);
            if (!nameMatches) return false;
            LinearLayout card = home.card(target);
            home.title(card, name, 20, true);
            home.status(card, "已保存 · 配对凭据尚未验证", false);
            return true;
        }
        DesktopBackend.State state = backend.state(desktop);
        List<DesktopBackend.Offer> offers = state == DesktopBackend.State.SHARES_READY
            ? backend.shareOffers(desktop) : java.util.Collections.emptyList();
        List<DesktopBackend.Session> snapshots = state == DesktopBackend.State.OBSERVING
            ? backend.observerSessions(desktop) : java.util.Collections.emptyList();
        List<NativeDesktopBackend.CatalogueRow> catalogue = backend.catalogueRows(desktop);
        List<NativeDesktopBackend.ManagementItem> manageable = backend.managementRows(desktop);
        boolean matched = nameMatches
            || manageable.stream().anyMatch(row -> SessionTitleFilter.matches(query, name, row.title))
            || catalogue.stream().anyMatch(row -> SessionTitleFilter.matches(query, name, row.title))
            || offers.stream().anyMatch(offer -> SessionTitleFilter.matches(query, name, offer.label))
            || snapshots.stream().anyMatch(session -> SessionTitleFilter.matches(query, name, session.title));
        boolean hasLiveAction = isEngaged(state) || state == DesktopBackend.State.LOADING_SHARES
            || state == DesktopBackend.State.AWAITING_OBSERVER
            || state == DesktopBackend.State.MANAGEMENT_READY || state == DesktopBackend.State.MANAGEMENT_WAITING;
        SessionSearchCoverage.Device coverageState = searchCoverageFor(state);
        if (!matched && !hasLiveAction) {
            coverage.add(coverageState);
            return false;
        }

        LinearLayout card = home.card(target);
        home.title(card, name, 20, true);
        home.status(card, deviceStatus(state), isEngaged(state));
        switch (state) {
            case MANAGEMENT_READY:
                if (manageable.isEmpty()) home.body(card, backend.managementArchived(desktop)
                    ? "没有可管理的归档会话" : "没有可管理的会话", 15);
                for (boolean archived : new boolean[]{false, true}) {
                    boolean heading = false;
                    for (NativeDesktopBackend.ManagementItem row : manageable) {
                        if (row.archived != archived || !SessionTitleFilter.matches(query, name, row.title)) continue;
                        if (!heading) {
                            home.title(card, archived ? "归档会话" : "普通会话", 16, true);
                            heading = true;
                        }
                        home.title(card, row.title, 16, false);
                        if (!archived) {
                            home.body(card, "open".equals(row.lifecycle) ? "打开" : "已关闭", 14);
                            home.action(card, "改名", false, () -> editManagedTitle(desktop, row));
                        }
                        if (!"open".equals(row.lifecycle))
                            home.action(card, archived ? "恢复" : "归档", false, () -> backend.changeManagementArchive(desktop, row));
                        else if (!archived) {
                            home.action(card, "接回原会话", true, () -> backend.reconnectManagementRun(desktop, row));
                            home.action(card, "关闭会话", false, () -> confirmManagedClose(desktop, row));
                        }
                    }
                }
                home.action(card, "关闭管理", false, () -> backend.disconnectObserver(desktop));
                break;
            case MANAGEMENT_VIEWING:
                home.action(card, "退出查看", false, () -> backend.disconnectObserver(desktop));
                home.body(card, "正在查看原会话 · 只读", 15);
                String projection = projectionForeground ? backend.managementProjection(desktop) : null;
                if (projection != null) {
                    android.widget.HorizontalScrollView grid = new android.widget.HorizontalScrollView(this);
                    grid.setSaveEnabled(false);
                    card.addView(grid);
                    TextView viewport = new TextView(this);
                    viewport.setSaveEnabled(false);
                    viewport.setTypeface(android.graphics.Typeface.MONOSPACE);
                    viewport.setTextSize(14);
                    viewport.setText(projection);
                    grid.addView(viewport);
                    managementViewports.put(desktop.localId, viewport);
                }
                break;
            case MANAGEMENT_WAITING:
                home.body(card, "正在等待桌面处理", 15);
                home.action(card, "断开", false, () -> backend.disconnectObserver(desktop));
                break;
            case DISCONNECTED:
            case UNAVAILABLE:
                int managementResult = backend.managementOutcome(desktop);
                if (managementResult != 0) home.body(card, managementResult == 1 ? "操作已完成"
                    : (managementResult == 2 ? "操作未获授权" : "结果尚未确认，请刷新核对，不会自动重试"), 15);
                home.action(card, "管理会话", false, () -> backend.loadManagement(desktop, identities));
                home.action(card, "管理归档", false, () -> backend.loadArchivedManagement(desktop, identities));
                home.body(card, "刷新以检查这台电脑可用的会话。", 15);
                home.action(card, "刷新这台电脑", false, () -> refreshDesktop(desktop));
                home.action(card, "查看归档", false, () -> loadArchived(desktop));
                break;
            case CATALOGUE_READY:
                boolean archiveCatalogue = backend.archiveCatalogue(desktop);
                if (archiveCatalogue) home.title(card, "归档会话（只读）", 16, true);
                if (catalogue.isEmpty()) home.body(card, archiveCatalogue ? "没有可查看的归档会话。" : "没有可查看的会话。", 15);
                for (NativeDesktopBackend.CatalogueRow row : catalogue) {
                    if (!SessionTitleFilter.matches(query, name, row.title)) continue;
                    home.title(card, row.title, 16, false);
                    home.body(card, "open".equals(row.lifecycle) ? "打开" :
                        ("closed".equals(row.lifecycle) ? "已关闭" : "历史"), 14);
                }
                home.action(card, "管理会话", false, () -> backend.loadManagement(desktop, identities));
                home.action(card, "管理归档", false, () -> backend.loadArchivedManagement(desktop, identities));
                home.action(card, "查看普通会话", false, () -> refreshDesktop(desktop));
                home.action(card, archiveCatalogue ? "刷新归档" : "查看归档", false, () -> loadArchived(desktop));
                break;
            case SHARES_READY:
                if (offers.isEmpty()) home.body(card, "没有可查看的会话。请检查电脑上的共享设置。", 15);
                for (DesktopBackend.Offer offer : offers) {
                    if (!SessionTitleFilter.matches(query, name, offer.label)) continue;
                    boolean controller = offer.role == DesktopBackend.Role.CONTROLLER;
                    home.title(card, offer.label, 16, false);
                    home.action(card, controller ? "请求控制" : "查看快照", true,
                        () -> backend.chooseShare(desktop, offer)).setContentDescription(
                            (controller ? "请求控制 " : "查看快照 ") + offer.label + "，" + name);
                }
                home.action(card, "关闭会话列表", false, () -> backend.disconnectObserver(desktop));
                break;
            case CONTROLLING:
                home.action(card, "打开控制台", true, () -> openControl(desktop, name));
                home.action(card, "停止控制", false, () -> backend.disconnectObserver(desktop));
                break;
            case LOADING_SHARES:
            case AWAITING_OBSERVER:
                home.action(card, "取消", false, () -> backend.disconnectObserver(desktop));
                break;
            case OBSERVING:
                home.action(card, "停止查看", false, () -> backend.disconnectObserver(desktop));
                for (DesktopBackend.Session session : snapshots) {
                    if (!SessionTitleFilter.matches(query, name, session.title)) continue;
                    // 来源始终与所属电脑绑定，不按会话 ID 跨主机合并。
                    home.title(card, session.title, 16, true);
                    home.body(card, "仅当前快照，不会自动更新。", 14);
                    TextView snapshot = home.title(card, session.text, 14, false);
                    snapshot.setTypeface(android.graphics.Typeface.MONOSPACE);
                }
                break;
            default:
                break;
        }
        coverage.add(coverageState);
        return matched;
    }

    private void confirmManagedClose(DesktopBackend.Desktop desktop, NativeDesktopBackend.ManagementItem row) {
        new android.app.AlertDialog.Builder(this).setTitle("关闭会话？")
            .setMessage(row.title + "正在运行。关闭会结束这个会话，历史会保留。")
            .setNegativeButton("取消", null)
            .setPositiveButton("关闭会话", (dialog, which) -> backend.closeManagementRun(desktop, row))
            .show();
    }

    private void editManagedTitle(DesktopBackend.Desktop desktop, NativeDesktopBackend.ManagementItem row) {
        android.widget.EditText title = new android.widget.EditText(this);
        title.setSingleLine(true);
        title.setFilters(new android.text.InputFilter[]{new android.text.InputFilter.LengthFilter(256)});
        title.setText(row.title);
        title.selectAll();
        new android.app.AlertDialog.Builder(this).setTitle("改名").setView(title)
            .setNegativeButton("取消", null)
            .setPositiveButton("保存", (dialog, which) -> {
                String value = title.getText().toString().trim();
                if (!value.isEmpty() && value.getBytes(java.nio.charset.StandardCharsets.UTF_8).length <= 256)
                    backend.renameManagement(desktop, row, value);
            }).show();
    }

    private void openControl(DesktopBackend.Desktop desktop, String source) {
        long exact = backend.controlHandle(desktop);
        if (exact == 0) return;
        controlDesktop = desktop;
        controlHandle = exact;
        controlPanel = new TerminalControlPanel(this, source,
            java.util.EnumSet.allOf(TerminalControlPanel.Key.class), new TerminalControlPanel.Actions() {
                @Override public void send(String text, TerminalControlPanel.Completion done) {
                    backend.sendControl(desktop, exact, text, null, done);
                }
                @Override public void key(TerminalControlPanel.Key key, TerminalControlPanel.Completion done) {
                    backend.sendControl(desktop, exact, null, key, done);
                }
                @Override public void leave() { leaveControl(true); }
            });
        setContentView(controlPanel);
        controlPanel.requestApplyInsets();
        updateConnectionViews();
    }

    private void updateManagementProjection(DesktopBackend.Desktop desktop) {
        if (!projectionForeground) return;
        TextView viewport = managementViewports.get(desktop.localId);
        if (viewport == null) return;
        if (!viewport.isAttachedToWindow()) {
            managementViewports.remove(desktop.localId);
            return;
        }
        String frame = backend.managementProjection(desktop);
        viewport.setText(frame == null ? "" : frame);
    }

    private void updateConnectionViews() {
        if (controlPanel == null) { render(); return; }
        DesktopBackend.Desktop desktop = controlDesktop;
        long original = controlHandle;
        if (backend.controlHandle(desktop) != original) {
            DesktopBackend.State last = backend.state(desktop);
            boolean unconfirmed = controlPanel.hasUnconfirmedInput();
            leaveControl(false);
            showControlEnded(desktop, last, unconfirmed);
            return;
        }
        List<DesktopBackend.Session> updates = backend.observerSessions(desktop);
        DesktopBackend.Session frame = updates.isEmpty() ? null : updates.get(0);
        boolean ready = backend.controllerReady(desktop, original);
        controlPanel.update(frame == null ? "" : frame.text, ready, ready ? "已连接，可以输入" : "等待电脑确认并同步输出");
        if (frame != null && frame.frameId > 0)
            backend.confirmDisplayedFrame(desktop, original, frame.frameId);
    }

    private void showControlEnded(DesktopBackend.Desktop desktop, DesktopBackend.State last,
            boolean unconfirmed) {
        if (!active || isFinishing()) return;
        boolean revoked = last == DesktopBackend.State.REVOKED;
        String message = revoked
            ? "这台电脑已撤销控制权限。需要在电脑上重新授权。"
            : last == DesktopBackend.State.DISCONNECTED
                ? "与这台电脑的连接已断开。可以重新读取共享会话。"
                : "暂时无法继续控制。请检查网络、电脑是否在线，以及电脑是否仍允许共享。";
        AlertDialog.Builder dialog = new AlertDialog.Builder(this)
            .setTitle(revoked ? "权限已撤销" : "控制连接已结束")
            .setMessage(message + (unconfirmed
                ? "\n无法确认最后一次输入是否送达。请先在电脑核对，不要重复发送。"
                : "\n已清除旧画面和草稿，未自动重发输入。"))
            .setNegativeButton("返回会话", null);
        if ((last == DesktopBackend.State.DISCONNECTED || last == DesktopBackend.State.UNAVAILABLE)
            && canUseDesktop(desktop)) {
            dialog.setPositiveButton("读取可用会话", (ignored, button) -> {
                if (!active || !canUseDesktop(desktop)) return;
                DesktopBackend.State current = backend.state(desktop);
                if (current == DesktopBackend.State.DISCONNECTED || current == DesktopBackend.State.UNAVAILABLE)
                    backend.loadShares(desktop, identities);
            });
        }
        dialog.show();
    }

    private void leaveControl(boolean disconnect) {
        if (controlPanel == null) return;
        DesktopBackend.Desktop original = controlDesktop;
        long exact = controlHandle;
        controlPanel.closeLocally();
        controlPanel = null;
        controlDesktop = null;
        controlHandle = 0;
        if (disconnect) backend.disconnectController(original, exact);
        render();
    }

    @Override protected void onStart() {
        super.onStart();
        projectionForeground = true;
    }

    @Override protected void onStop() {
        projectionForeground = false;
        if (controlPanel != null) leaveControl(true);
        for (TextView viewport : managementViewports.values()) viewport.setText("");
        if (backend != null) backend.stopManagementViews();
        super.onStop();
    }

    @Override public void onBackPressed() {
        if (controlPanel != null) leaveControl(true);
        else if (showingSessions) navigateHome(false);
        else super.onBackPressed();
    }

    private void editDesktopAlias(DesktopBackend.Desktop desktop, String displayName) {
        EditText input = new EditText(this);
        input.setSingleLine();
        input.setSaveEnabled(false);
        input.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);
        input.setHint("仅保存在这部手机");
        input.setText(displayName);
        input.selectAll();
        AlertDialog dialog = new AlertDialog.Builder(this)
            .setTitle("设备设置")
            .setMessage("别名仅保存在本机，不会更改电脑上的名称或共享授权。")
            .setView(input)
            .setNegativeButton("取消", null)
            .setPositiveButton("保存", null).create();
        dialog.setOnShowListener(unused -> dialog.getButton(AlertDialog.BUTTON_POSITIVE)
            .setOnClickListener(view -> {
                try {
                    store.renameAlias(desktop, input.getText().toString());
                    dialog.dismiss();
                    render();
                } catch (IllegalArgumentException invalid) {
                    input.setError("请输入 1–100 字符的别名，不含换行或控制字符。");
                } catch (IOException | RuntimeException failure) {
                    input.setError("未保存。请返回设备列表后再试。");
                }
            }));
        dialog.show();
        dialog.getWindow().addFlags(WindowManager.LayoutParams.FLAG_SECURE);
    }

    private void manualPairing() {
        if (backend == null || pairingPending) return;
        LinearLayout form = new LinearLayout(this);
        form.setOrientation(LinearLayout.VERTICAL);
        EditText relay = new EditText(this);
        relay.setSingleLine();
        relay.setSaveEnabled(false);
        relay.setHint("从电脑复制中转地址");
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
        addPairingField(form, "中转地址", relay);
        addPairingField(form, "配对码", code);
        relay.setNextFocusForwardId(code.getId());
        relay.setImeOptions(android.view.inputmethod.EditorInfo.IME_ACTION_NEXT
            | android.view.inputmethod.EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING);
        code.setImeOptions(android.view.inputmethod.EditorInfo.IME_ACTION_DONE
            | android.view.inputmethod.EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING);
        AlertDialog dialog = new AlertDialog.Builder(this).setTitle("配对桌面")
            .setView(pairingDialogContent(form)).setNegativeButton("取消", null).setPositiveButton("请求桌面批准", null).create();
        dialog.setOnDismissListener(unused -> code.getText().clear());
        dialog.getWindow().addFlags(WindowManager.LayoutParams.FLAG_SECURE);
        dialog.getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        dialog.show();
        if (relay.length() == 0) relay.requestFocus(); else code.requestFocus();
        code.setOnEditorActionListener((view, actionId, event) -> {
            if (actionId != android.view.inputmethod.EditorInfo.IME_ACTION_DONE) return false;
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).performClick();
            return true;
        });
        dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(unused -> {
            final URI locator;
            try { locator = DesktopBackend.Desktop.checkedRelay(relay.getText().toString().trim()); }
            catch (IllegalArgumentException e) { relay.requestFocus(); relay.setError("请输入有效的中转地址，可从电脑版复制"); return; }
            final PairingInvite invite;
            try { invite = PairingInvite.fromPasted(locator, code.getText()); }
            catch (IllegalArgumentException e) { code.requestFocus(); code.setError("请粘贴电脑显示的完整配对码"); return; }
            dialog.dismiss();
            confirm(invite);
        });
    }

    private void addPairingField(LinearLayout form, String caption, EditText input) {
        input.setId(android.view.View.generateViewId());
        input.setImportantForAutofill(android.view.View.IMPORTANT_FOR_AUTOFILL_NO);
        TextView label = home.title(form, caption, 15, false);
        label.setLabelFor(input.getId());
        form.addView(input, new LinearLayout.LayoutParams(-1, -2));
    }

    private ScrollView pairingDialogContent(LinearLayout content) {
        int padding = Math.round(20 * getResources().getDisplayMetrics().density);
        content.setPadding(padding, padding / 2, padding, padding / 2);
        ScrollView scroll = new ScrollView(this);
        scroll.addView(content, new ScrollView.LayoutParams(-1, -2));
        return scroll;
    }

    private void scanPairing() {
        if (backend == null || pairingPending || scanOwner != null) return;
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
            .setView(pairingDialogContent(confirmation))
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

    private void cancelEnrollment() {
        if (!pairingPending || cancellingEnrollment) return;
        long epoch = enrollmentEpoch;
        cancellingEnrollment = true;
        render();
        backend.cancelEnrollment(removed -> runOnUiThread(() ->
            finishEnrollment(epoch, removed ? PairingNotice.CANCELLED : PairingNotice.CANCEL_UNCONFIRMED)));
    }

    private void finishEnrollment(long epoch, PairingNotice notice) {
        if (!active || epoch != enrollmentEpoch) return;
        enrollmentEpoch++;
        pairingPending = false;
        cancellingEnrollment = false;
        pairingNotice = notice;
        render();
    }

    private void redeem(PairingInvite invite) {
        long epoch = ++enrollmentEpoch;
        pairingNotice = PairingNotice.NONE;
        pairingPending = true;
        render();
        DesktopBackend.EnrollmentCallback completion = new DesktopBackend.EnrollmentCallback() {
            @Override public void approved(DesktopBackend.Desktop desktop) {
                runOnUiThread(() -> finishEnrollment(epoch, PairingNotice.SAVED));
            }
            @Override public void state(DesktopBackend.State state) {
                if (state != DesktopBackend.State.WAITING_FOR_DESKTOP)
                    runOnUiThread(() -> finishEnrollment(epoch, PairingNotice.FAILED));
            }
        };
        try { backend.redeemInvite(invite, identities, completion); }
        finally { invite.close(); }
    }

    private String label(DesktopBackend.State state) {
        switch (state) {
            case WAITING_FOR_DESKTOP: return "请在电脑上点同意";
            case LOADING_SHARES: return "正在读取共享会话";
            case MANAGEMENT_READY: return "会话管理已连接";
            case MANAGEMENT_VIEWING: return "原会话已接入";
            case MANAGEMENT_WAITING: return "正在提交管理操作";
            case SHARES_READY: return "共享会话";
            case CATALOGUE_READY: return "会话列表（只读）";
            case AWAITING_OBSERVER: return "正在连接会话，请按手机提示确认";
            case OBSERVING: return "会话快照";
            case CONTROLLING: return "已获控制批准";
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
