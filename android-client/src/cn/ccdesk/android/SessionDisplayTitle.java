package cn.ccdesk.android;

final class SessionDisplayTitle {
    private SessionDisplayTitle() {}

    // 与 Rust 目录协议一致：256 UTF-8 字节，禁止控制字符但允许 ZWJ 等格式字符。
    static boolean valid(String title) {
        if (title == null || title.length() > 256) return false;
        int bytes = 0;
        boolean nonWhitespace = false;
        for (int i = 0; i < title.length();) {
            int ch = title.codePointAt(i);
            i += Character.charCount(ch);
            if (Character.isISOControl(ch) || (ch >= 0xd800 && ch <= 0xdfff)) return false;
            bytes += ch <= 0x7f ? 1 : ch <= 0x7ff ? 2 : ch <= 0xffff ? 3 : 4;
            if (bytes > 256) return false;
            nonWhitespace |= !Character.isSpaceChar(ch);
        }
        return nonWhitespace;
    }
}
