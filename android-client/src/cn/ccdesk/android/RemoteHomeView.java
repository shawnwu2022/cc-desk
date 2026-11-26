package cn.ccdesk.android;

import android.app.Activity;
import android.content.res.ColorStateList;
import android.content.res.Configuration;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.view.Gravity;
import android.view.WindowInsets;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

/** Android home styling and layout only. No pairing, sharing or permission decisions live here. */
final class RemoteHomeView {
    private final Activity activity;
    final LinearLayout root;
    final LinearLayout body;
    final ScrollView scroll;
    private LinearLayout bottomNavigation;
    private boolean sessionScreen;
    private final int canvas, surface, ink, subdued, outline, accent, onAccent, activeFill;

    RemoteHomeView(Activity activity) {
        this.activity = activity;
        boolean dark = (activity.getResources().getConfiguration().uiMode
            & Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES;
        canvas = dark ? 0xff1c1a17 : 0xfffaf9f6;
        surface = dark ? 0xff282522 : 0xffffffff;
        ink = dark ? 0xfff8f6f3 : 0xff1a1816;
        subdued = dark ? 0xffc3bab1 : 0xff5a5550;
        outline = dark ? 0xff4b4540 : 0xffd9d5cc;
        accent = dark ? 0xffa8caef : 0xff1e3a5f;
        onAccent = dark ? 0xff142837 : 0xffffffff;
        activeFill = dark ? 0xff263b50 : 0xffe6eef7;
        root = column();
        root.setBackgroundColor(canvas);
        root.setOnApplyWindowInsetsListener((view, insets) -> {
            android.graphics.Insets safe = insets.getInsets(
                WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
            boolean typing = sessionScreen && insets.isVisible(WindowInsets.Type.ime());
            int bottom = typing ? Math.max(safe.bottom, insets.getInsets(WindowInsets.Type.ime()).bottom) : safe.bottom;
            view.setPadding(safe.left, safe.top, safe.right, bottom);
            if (bottomNavigation != null) {
                int visibility = typing ? android.view.View.GONE : android.view.View.VISIBLE;
                if (bottomNavigation.getVisibility() != visibility) bottomNavigation.setVisibility(visibility);
            }
            if (view.getWindowInsetsController() != null) {
                int light = android.view.WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                    | android.view.WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
                view.getWindowInsetsController().setSystemBarsAppearance(dark ? 0 : light, light);
            }
            return WindowInsets.CONSUMED;
        });
        scroll = new ScrollView(activity);
        scroll.setFillViewport(true);
        body = column();
        body.setPadding(dp(20), dp(12), dp(20), dp(28));
        scroll.addView(body);
    }

    void build(boolean sessions, Runnable devices, Runnable openSessions) {
        LinearLayout header = column();
        header.setPadding(dp(20), dp(12), dp(20), dp(6));
        root.addView(header);
        TextView title = title(header, "CC Desk", 23, true);
        title.setPadding(0, 0, 0, dp(5));
        root.addView(scroll, new LinearLayout.LayoutParams(-1, 0, 1));
        sessionScreen = sessions;
        bottomNavigation = column();
        bottomNavigation.setPadding(dp(20), dp(6), dp(20), dp(10));
        LinearLayout nav = new LinearLayout(activity);
        nav.setBackground(shape(surface, 14, outline));
        nav.setPadding(dp(4), dp(4), dp(4), dp(4));
        bottomNavigation.addView(nav);
        tab(nav, "设备", !sessions, devices);
        tab(nav, "会话", sessions, openSessions);
        root.addView(bottomNavigation);
    }

    private void tab(LinearLayout nav, String label, boolean active, Runnable go) {
        Button tab = makeButton(label, false, go);
        tab.setSelected(active);
        tab.setBackground(ripple(active ? activeFill : surface, 10, 0));
        tab.setTextColor(active ? accent : subdued);
        tab.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        nav.addView(tab, new LinearLayout.LayoutParams(0, -2, 1));
    }

    LinearLayout card(LinearLayout target) {
        LinearLayout card = column();
        card.setBackground(shape(surface, 16, outline));
        card.setPadding(dp(16), dp(12), dp(16), dp(14));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-1, -2);
        lp.bottomMargin = dp(12);
        target.addView(card, lp);
        return card;
    }

    TextView title(LinearLayout target, String value, int size, boolean heading) {
        TextView view = line(target, value, size, ink);
        view.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        view.setAccessibilityHeading(heading);
        return view;
    }

    TextView body(LinearLayout target, String text, int size) { return line(target, text, size, subdued); }

    TextView status(LinearLayout target, String text, boolean engaged) {
        TextView view = new TextView(activity);
        view.setText(text);
        view.setTextSize(13);
        view.setTextColor(engaged ? accent : subdued);
        view.setPadding(dp(10), dp(3), dp(10), dp(3));
        view.setBackground(shape(engaged ? activeFill : canvas, 8, 0));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-2, -2);
        lp.topMargin = dp(3);
        lp.bottomMargin = dp(6);
        target.addView(view, lp);
        return view;
    }

    Button action(LinearLayout target, String label, boolean primary, Runnable go) {
        Button button = makeButton(label, primary, go);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-1, -2);
        lp.topMargin = dp(6);
        target.addView(button, lp);
        return button;
    }

    private Button makeButton(String label, boolean primary, Runnable go) {
        Button button = new Button(activity);
        button.setText(label);
        button.setTextSize(15);
        button.setAllCaps(false);
        button.setGravity(Gravity.CENTER);
        button.setMinWidth(0);
        button.setMinimumWidth(0);
        button.setMinHeight(dp(48));
        button.setMinimumHeight(dp(48));
        button.setPadding(dp(10), dp(6), dp(10), dp(6));
        button.setTextColor(new ColorStateList(
            new int[][] { new int[] {-android.R.attr.state_enabled}, new int[0] },
            new int[] {subdued, primary ? onAccent : accent}));
        android.graphics.drawable.StateListDrawable backgrounds = new android.graphics.drawable.StateListDrawable();
        backgrounds.addState(new int[] {-android.R.attr.state_enabled}, shape(canvas, 12, outline));
        backgrounds.addState(new int[0], ripple(primary ? accent : canvas, 12, primary ? 0 : outline));
        button.setBackground(backgrounds);
        button.setOnClickListener(unused -> go.run());
        return button;
    }

    private TextView line(LinearLayout target, String text, int size, int color) {
        TextView view = new TextView(activity);
        view.setText(text);
        view.setTextSize(size);
        view.setTextColor(color);
        view.setPadding(0, dp(3), 0, dp(4));
        target.addView(view);
        return view;
    }

    private LinearLayout column() {
        LinearLayout view = new LinearLayout(activity);
        view.setOrientation(LinearLayout.VERTICAL);
        return view;
    }

    private GradientDrawable shape(int color, int radius, int stroke) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(color);
        drawable.setCornerRadius(dp(radius));
        if (stroke != 0) drawable.setStroke(dp(1), stroke);
        return drawable;
    }

    private RippleDrawable ripple(int color, int radius, int stroke) {
        return new RippleDrawable(ColorStateList.valueOf(0x226880a0), shape(color, radius, stroke), null);
    }

    private int dp(int value) { return Math.round(value * activity.getResources().getDisplayMetrics().density); }
}
