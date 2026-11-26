package cn.ccdesk.android;

import java.util.Locale;

public final class SessionTitleFilterTest {
    public static void main(String[] args) {
        Locale original = Locale.getDefault();
        try {
            Locale.setDefault(new Locale("tr", "TR"));
            expect(true, "   ", "工作电脑", null);
            expect(true, " 工作 ", "工作电脑", null);
            expect(true, "  CI  ", "家庭电脑", "CI 报告");
            expect(true, "检查", "开发机", "发布检查");
            expect(false, "发布", "开发机", null);
            expect(false, "不存在", "开发机", "CI 报告");
        } finally {
            Locale.setDefault(original);
        }
        System.out.println("SessionTitleFilterTest: 6 checks passed");
    }

    private static void expect(boolean expected, String query, String alias, String visibleTitle) {
        if (SessionTitleFilter.matches(SessionTitleFilter.normalize(query), alias, visibleTitle) != expected)
            throw new AssertionError("Unexpected title filter result");
    }
}
