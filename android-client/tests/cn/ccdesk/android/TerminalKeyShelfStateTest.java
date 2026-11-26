package cn.ccdesk.android;

/** Pure JVM verification; does not simulate Android focus or keyboard delivery. */
public final class TerminalKeyShelfStateTest {
    public static void main(String[] args) {
        TerminalKeyShelfState keys = new TerminalKeyShelfState();
        expect(false, keys, "keyboard hidden leaves room for output");
        keys.updateIme(true);
        expect(true, keys, "software keyboard reveals shortcuts");
        keys.toggle();
        keys.updateIme(true);
        expect(false, keys, "repeated insets do not undo dismissal");
        keys.toggle();
        expect(true, keys, "shortcuts can return without closing IME");
        keys.toggle();
        keys.updateIme(false);
        keys.updateIme(true);
        expect(true, keys, "reopening IME restores the default");
        keys.updateIme(false);
        expect(false, keys, "ordinary close removes the accessory");
        keys.toggle();
        expect(true, keys, "manual pin without a software keyboard");
        keys.updateIme(true);
        keys.updateIme(false);
        expect(true, keys, "manual pin remains until dismissed");
        keys.toggle();
        expect(false, keys, "unpin returns space to output");
        System.out.println("TerminalKeyShelfStateTest: 9 checks passed");
    }

    private static void expect(boolean expected, TerminalKeyShelfState keys, String why) {
        if (keys.isVisible() != expected) throw new AssertionError(why);
    }
}
