package cn.ccdesk.android;

/** New output follows only this screen's newest layout and user's scroll intent. */
final class TerminalViewportState {
    private long layout;
    private boolean following = true;
    private boolean closed;

    long nextLayout() { return ++layout; }

    void scrolled(boolean atLatest) {
        if (!closed) following = atLatest;
    }

    long returnToLatest() {
        if (!closed) following = true;
        return nextLayout();
    }

    boolean follow(long exactLayout) {
        return !closed && following && layout == exactLayout;
    }

    void close() {
        closed = true;
        layout++;
    }
}
