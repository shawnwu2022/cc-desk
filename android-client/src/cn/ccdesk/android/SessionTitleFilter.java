package cn.ccdesk.android;

import java.util.Locale;

/** UI-only search. The caller supplies a local alias and an already-visible title, never full content. */
final class SessionTitleFilter {
    private SessionTitleFilter() {}

    static String normalize(CharSequence value) {
        return value == null ? "" : value.toString().trim().toLowerCase(Locale.ROOT);
    }

    static boolean matches(String normalizedQuery, String alias, String visibleTitle) {
        return normalizedQuery.isEmpty() || normalize(alias).contains(normalizedQuery)
            || (visibleTitle != null && normalize(visibleTitle).contains(normalizedQuery));
    }
}
