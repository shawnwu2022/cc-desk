package cn.ccdesk.android;

import android.content.Context;
import android.graphics.Typeface;
import android.text.InputFilter;
import android.text.InputType;
import android.view.View;
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
    private final LinearLayout keys;
    private boolean active;
    private boolean pending;
    private boolean uncertain;
    private boolean closed;
    private long sequence;

    TerminalControlPanel(Context context, String source, EnumSet<Key> supported, Actions actions) {
        super(context);
        this.actions = actions;
        setOrientation(VERTICAL);
        int padding = (int)(12 * getResources().getDisplayMetrics().density);
        setPadding(padding, padding, padding, padding);
        TextView header = new TextView(context);
        header.setText("远程控制 · " + source);
        header.setTextSize(20);
        addView(header);
        Button stop = new Button(context);
        stop.setText("停止控制");
        stop.setOnClickListener(unused -> leave());
        addView(stop);

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

        feedback = new TextView(context);
        feedback.setText("等候电脑批准和手机确认");
        feedback.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);
        addView(feedback);

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
        addView(send);

        HorizontalScrollView controls = new HorizontalScrollView(context);
        keys = new LinearLayout(context);
        for (Key key : supported) {
            Button action = new Button(context);
            action.setText(keyLabel(key));
            action.setOnClickListener(unused -> request(false, (completion) -> actions.key(key, completion)));
            keys.addView(action);
        }
        controls.addView(keys);
        addView(controls);
        updateEnabled();
    }

    void update(String text, boolean canSend, String status) {
        if (closed) return;
        boolean bottom = !console.canScrollVertically(1);
        if (!output.getText().toString().equals(text)) {
            output.setText(text);
            if (bottom) console.post(() -> console.fullScroll(View.FOCUS_DOWN));
        }
        if (!uncertain && !pending && (active != canSend || !canSend)) feedback.setText(status);
        active = canSend && !uncertain;
        updateEnabled();
    }

    void leave() {
        if (closed) return;
        closeLocally();
        actions.leave();
    }

    void closeLocally() {
        if (closed) return;
        closed = true;
        sequence++;
        active = false;
        pending = false;
        output.setText("");
        draft.getText().clear();
        updateEnabled();
    }

    private interface Operation { void run(Completion completion); }

    private void request(boolean text, Operation operation) {
        if (!active || pending || closed) return;
        long original = ++sequence;
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
        draft.setEnabled(enabled);
        send.setEnabled(enabled);
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
