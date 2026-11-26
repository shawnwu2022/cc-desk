package cn.ccdesk.android;

/** Key shelf follows the software keyboard unless the user overrides it for this screen. */
final class TerminalKeyShelfState {
    private boolean imeVisible;
    private boolean pinned;
    private boolean hiddenForCurrentIme;

    void updateIme(boolean visible) {
        if (imeVisible != visible) hiddenForCurrentIme = false;
        imeVisible = visible;
    }

    void toggle() {
        if (isVisible()) {
            pinned = false;
            hiddenForCurrentIme = imeVisible;
        } else if (imeVisible) {
            hiddenForCurrentIme = false;
        } else {
            pinned = true;
        }
    }

    boolean isVisible() {
        return pinned || (imeVisible && !hiddenForCurrentIme);
    }
}
