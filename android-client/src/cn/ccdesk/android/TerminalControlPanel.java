package cn.ccdesk.android;

import android.content.Context;
import android.graphics.Insets;
import android.graphics.Typeface;
import android.text.InputFilter;
import android.text.InputType;
import android.text.TextUtils;
import android.view.Gravity;
import android.view.View;
import android.view.WindowInsets;
import android.view.ViewTreeObserver;
import android.widget.Button;
import android.widget.EditText;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import java.util.EnumSet;

/** One exact control screen. Rendering remote output cannot mutate/send the draft. */
final class TerminalControlPanel extends LinearLayout {
    enum Key { ENTER, TAB, ESCAPE, UP, DOWN, LEFT, RIGHT, CTRL_C, BACKSPACE }
    enum Result { COMMITTED, REJECTED, UNKNOWN }
    interface Completion { void finish(Result result); }
    interface Actions {
        void send(String explicitText, Completion completion);
        void key(Key key, Completion completion);
        void leave();
    }

    private final Actions actions;
    private final ScrollView console;
    private final TextView output;
    private final TextView feedback;
    private final EditText draft;
    private final Button send;
    private final Button jumpToLatest;
    private final LinearLayout keys;
    private final HorizontalScrollView controls;
    private final Button toggleKeys;
    private final TerminalKeyShelfState keyShelf = new TerminalKeyShelfState();
    private final TerminalViewportState viewport = new TerminalViewportState();
    private ViewTreeObserver.OnPreDrawListener pendingLayout;
    private boolean applyingScroll;
    private boolean active;
    private boolean pending;
    private boolean pendingText;
    private boolean uncertain;
    private boolean closed;
    private long sequence;

    TerminalControlPanel(Context context, String source, EnumSet<Key> supported, Actions actions) {
        super(context);
        this.actions = actions;
        setOrientation(VERTICAL);
        int padding = dp(12);
        setPadding(padding, padding, padding, padding);
        LinearLayout appBar = new LinearLayout(context);
        appBar.setGravity(Gravity.CENTER_VERTICAL);
        TextView header = new TextView(context);
        header.setText("远程控制\n" + source);
        header.setTextSize(18);
        header.setMaxLines(3);
        header.setEllipsize(TextUtils.TruncateAt.END);
        header.setAccessibilityHeading(true);
        appBar.addView(header, new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1));
        Button stop = new Button(context);
        stop.setText("停止控制");
        stop.setOnClickListener(unused -> leave());
        appBar.addView(stop);
        addView(appBar);

        console = new ScrollView(context);
        console.setFillViewport(true);
        HorizontalScrollView horizontal = new HorizontalScrollView(context);
        output = new TextView(context);
        output.setTypeface(Typeface.MONOSPACE);
        output.setTextSize(14);
        output.setTextIsSelectable(true);
        output.setSaveEnabled(false);
        horizontal.addView(output);
        console.addView(horizontal);
        addView(console, new LayoutParams(LayoutParams.MATCH_PARENT, 0, 1));

        LinearLayout statusBar = new LinearLayout(context);
        statusBar.setGravity(Gravity.CENTER_VERTICAL);
        jumpToLatest = new Button(context);
        jumpToLatest.setText("最新输出");
        jumpToLatest.setOnClickListener(unused -> scheduleScroll(viewport.returnToLatest()));
        jumpToLatest.setEnabled(false);
        console.setOnScrollChangeListener((view, x, y, oldX, oldY) -> {
            if (!applyingScroll && y != oldY) viewport.scrolled(!console.canScrollVertically(1));
            updateLatestAction();
        });
        console.addOnLayoutChangeListener((view, l, t, r, b, oldL, oldT, oldR, oldB) -> {
            if (!closed && b - t != oldB - oldT) scheduleScroll(viewport.nextLayout());
        });

        feedback = new TextView(context);
        feedback.setText("等候电脑批准和手机确认");
        feedback.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);
        statusBar.addView(feedback, new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1));
        statusBar.addView(jumpToLatest);
        addView(statusBar, 1);

        draft = new EditText(context);
        draft.setHint("输入或粘贴单行文字，点发送；换行用回车键");
        draft.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE |
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
        draft.setSaveEnabled(false);
        draft.setImportantForAutofill(View.IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS);
        draft.setMaxLines(3);
        draft.setFilters(new InputFilter[] {new InputFilter.LengthFilter(16384)});
        addView(draft);

        send = new Button(context);
        draft.addTextChangedListener(new android.text.TextWatcher() {
            @Override public void beforeTextChanged(CharSequence text, int start, int count, int after) {}
            @Override public void onTextChanged(CharSequence text, int start, int before, int count) {
                updateEnabled();
            }
            @Override public void afterTextChanged(android.text.Editable text) {}
        });
        send.setText("发送文字");
        send.setOnClickListener(unused -> {
            String captured = draft.getText().toString();
            if (captured.isEmpty()) return;
            if (captured.codePoints().anyMatch(c -> Character.isISOControl(c) || c == 0x2028 || c == 0x2029)) {
                feedback.setText("只能发送单行文字；换行请使用回车键。");
                return;
            }
            if (captured.getBytes(java.nio.charset.StandardCharsets.UTF_8).length > 2048) {
                feedback.setText("这段文字太长，请缩短后发送。");
                return;
            }
            request(true, (completion) -> actions.send(captured, completion));
        });
        LinearLayout inputActions = new LinearLayout(context);
        inputActions.setGravity(Gravity.CENTER_VERTICAL);
        inputActions.addView(send, new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1));
        toggleKeys = new Button(context);
        toggleKeys.setOnClickListener(unused -> {
            keyShelf.toggle();
            layoutKeyShelf();
        });
        inputActions.addView(toggleKeys);
        addView(inputActions);

        controls = new HorizontalScrollView(context);
        controls.setFillViewport(false);
        controls.setHorizontalScrollBarEnabled(true);
        controls.setScrollbarFadingEnabled(false);
        keys = new LinearLayout(context);
        Key[] order = {Key.LEFT, Key.DOWN, Key.UP, Key.RIGHT, Key.TAB,
            Key.ESCAPE, Key.CTRL_C, Key.BACKSPACE, Key.ENTER};
        for (Key key : order) {
            if (!supported.contains(key)) continue;
            Button action = new Button(context);
            action.setText(keyLabel(key));
            action.setAllCaps(false);
            action.setContentDescription(keyDescription(key));
            action.setMinWidth(dp(48));
            action.setMinimumWidth(dp(48));
            action.setMinHeight(dp(48));
            action.setMinimumHeight(dp(48));
            action.setPadding(dp(6), 0, dp(6), 0);
            action.setOnClickListener(unused -> request(false, (completion) -> actions.key(key, completion)));
            keys.addView(action);
        }
        controls.addView(keys);
        addView(controls);
        toggleKeys.setVisibility(supported.isEmpty() ? GONE : VISIBLE);
        layoutKeyShelf();
        setOnApplyWindowInsetsListener((view, insets) -> {
            Insets safe = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
            Insets ime = insets.getInsets(WindowInsets.Type.ime());
            setPadding(padding + safe.left, padding + safe.top, padding + safe.right,
                padding + Math.max(safe.bottom, ime.bottom));
            keyShelf.updateIme(insets.isVisible(WindowInsets.Type.ime()));
            layoutKeyShelf();
            return WindowInsets.CONSUMED;
        });
        updateEnabled();
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    private void layoutKeyShelf() {
        boolean visible = !closed && keys.getChildCount() != 0 && keyShelf.isVisible();
        int visibility = visible ? VISIBLE : GONE;
        if (controls.getVisibility() != visibility) controls.setVisibility(visibility);
        String title = visible ? "隐藏快捷键" : "终端快捷键";
        if (!title.contentEquals(toggleKeys.getText())) toggleKeys.setText(title);
        toggleKeys.setSelected(visible);
    }

    private static String keyDescription(Key key) {
        switch (key) {
            case LEFT: return "向左方向键";
            case RIGHT: return "向右方向键";
            case UP: return "向上方向键";
            case DOWN: return "向下方向键";
            default: return keyLabel(key);
        }
    }

    void update(String text, boolean canSend, String status) {
        if (closed) return;
        if (!output.getText().toString().equals(text)) {
            output.setText(text);
            scheduleScroll(viewport.nextLayout());
        }
        if (!uncertain && !pending && (active != canSend || !canSend)
            && !status.contentEquals(feedback.getText())) feedback.setText(status);
        active = canSend && !uncertain;
        updateEnabled();
    }

    private void scheduleScroll(long exactLayout) {
        removePendingLayout();
        pendingLayout = () -> {
            removePendingLayout();
            if (viewport.follow(exactLayout) && console.getChildCount() != 0) {
                int extent = Math.max(0,
                    console.getHeight() - console.getPaddingTop() - console.getPaddingBottom());
                applyingScroll = true;
                try {
                    console.scrollTo(console.getScrollX(),
                        Math.max(0, console.getChildAt(0).getHeight() - extent));
                } finally {
                    applyingScroll = false;
                }
            }
            updateLatestAction();
            return true;
        };
        console.getViewTreeObserver().addOnPreDrawListener(pendingLayout);
        console.invalidate();
    }

    private void removePendingLayout() {
        ViewTreeObserver tree = console.getViewTreeObserver();
        if (pendingLayout != null && tree.isAlive()) tree.removeOnPreDrawListener(pendingLayout);
        pendingLayout = null;
    }

    private void updateLatestAction() {
        jumpToLatest.setEnabled(!closed && console.canScrollVertically(1));
    }

    boolean hasUnconfirmedInput() { return pending || uncertain; }

    void leave() {
        if (closed) return;
        closeLocally();
        actions.leave();
    }

    void closeLocally() {
        if (closed) return;
        closed = true;
        viewport.close();
        removePendingLayout();
        sequence++;
        active = false;
        pending = false;
        output.setText("");
        jumpToLatest.setEnabled(false);
        draft.getText().clear();
        layoutKeyShelf();
        updateEnabled();
    }

    private interface Operation { void run(Completion completion); }

    private void request(boolean text, Operation operation) {
        if (!active || pending || closed) return;
        long original = ++sequence;
        pendingText = text;
        pending = true;
        feedback.setText("正在发送…");
        updateEnabled();
        operation.run(result -> {
            if (closed || original != sequence) return;
            pending = false;
            switch (result) {
                case COMMITTED:
                    if (text) draft.getText().clear();
                    feedback.setText("已发送");
                    break;
                case REJECTED:
                    feedback.setText("未发送，请检查电脑是否仍允许控制");
                    break;
                default:
                    if (text) draft.getText().clear();
                    uncertain = true;
                    active = false;
                    feedback.setText("无法确认是否送达。请先查看电脑，不要重复发送。");
                    break;
            }
            updateEnabled();
        });
    }

    private void updateEnabled() {
        boolean enabled = active && !pending && !closed;
        // 远程按键等待回执时，不中断本地输入法的候选词编辑。
        draft.setEnabled(!closed && !uncertain && !(pending && pendingText));
        send.setEnabled(enabled && draft.length() != 0);
        toggleKeys.setEnabled(!closed);
        for (int i = 0; i < keys.getChildCount(); i++) keys.getChildAt(i).setEnabled(enabled);
    }

    private static String keyLabel(Key key) {
        switch (key) {
            case ENTER: return "回车";
            case TAB: return "Tab";
            case ESCAPE: return "Esc";
            case UP: return "↑";
            case DOWN: return "↓";
            case LEFT: return "←";
            case RIGHT: return "→";
            case CTRL_C: return "Ctrl+C";
            default: return "退格";
        }
    }
}
