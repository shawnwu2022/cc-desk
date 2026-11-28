package cn.ccdesk.android;

public final class SessionDisplayTitleTest {
    public static void main(String[] args) {
        expect(true, "Fix 👩‍💻 login");
        expect(true, "界面");
        expect(true, repeat("界", 85) + "a");
        expect(false, repeat("界", 86));
        expect(false, " \u00a0\u3000");
        expect(false, "bad\u0085title");
        expect(false, "bad\nline");
        expect(false, "bad\ud800title");
    }

    private static String repeat(String value, int count) {
        StringBuilder result = new StringBuilder();
        for (int i = 0; i < count; i++) result.append(value);
        return result.toString();
    }

    private static void expect(boolean expected, String title) {
        if (SessionDisplayTitle.valid(title) != expected)
            throw new AssertionError("Unexpected title validation for " + title);
    }
}
